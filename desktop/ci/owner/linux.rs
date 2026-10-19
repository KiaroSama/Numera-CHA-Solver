use super::super::metadata::{linux_stat, tagged_bytes};
use super::super::policy::{Identity, Pid, vanished};
use super::bounded_file;
use std::{fs, io};
unsafe extern "C" {
    fn prctl(option: i32, ...) -> i32;
}
pub fn enable_reaping() -> io::Result<()> {
    // SAFETY: PR_SET_CHILD_SUBREAPER's four unsigned-long arguments match the Linux ABI.
    if unsafe { prctl(36, 1_usize, 0_usize, 0_usize, 0_usize) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
pub fn list() -> io::Result<Vec<Pid>> {
    let mut pids = Vec::new();
    for item in fs::read_dir("/proc")? {
        if let Some(pid) = item?
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<Pid>().ok())
        {
            if pid > 0 {
                pids.push(pid);
            }
        }
        if pids.len() > 1_000_000 {
            return Err(io::Error::other("Process enumeration exceeded its bound."));
        }
    }
    Ok(pids)
}
pub fn identity(pid: Pid) -> io::Result<Option<Identity>> {
    let read = || {
        let stat = bounded_file(&format!("/proc/{pid}/stat"), 1024 * 1024)?;
        let first = linux_stat(pid, &stat, 0)?;
        let status = bounded_file(&format!("/proc/{pid}/status"), 1024 * 1024)?;
        let status = std::str::from_utf8(&status)
            .map_err(|_| io::Error::other("Invalid process status."))?;
        let uid = status
            .lines()
            .find_map(|line| {
                line.strip_prefix("Uid:")
                    .and_then(|rest| rest.split_whitespace().next())
                    .and_then(|value| value.parse::<u32>().ok())
            })
            .ok_or_else(|| io::Error::other("Real process UID unavailable."))?;
        // Re-read stat after status to reject a PID recycled between the two files.
        let stat = bounded_file(&format!("/proc/{pid}/stat"), 1024 * 1024)?;
        let second = linux_stat(pid, &stat, uid)?;
        if !first.same(second) {
            return Err(io::Error::other(
                "Process identity changed during inspection.",
            ));
        }
        Ok(Some(second))
    };
    match read() {
        Err(error) if vanished(&error) => Ok(None),
        result => result,
    }
}
pub fn tagged(pid: Pid, tag: &str) -> io::Result<bool> {
    bounded_file(&format!("/proc/{pid}/environ"), 8 * 1024 * 1024)
        .map(|bytes| tagged_bytes(&bytes, tag))
}
