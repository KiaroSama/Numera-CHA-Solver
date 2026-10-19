use super::super::metadata::{procargs_environment, tagged_bytes};
use super::super::policy::{Birth, Identity, Pid, State, vanished};
use std::{
    ffi::c_void,
    io,
    mem::{MaybeUninit, size_of},
    ptr,
};

// Apple's proc_bsdinfo ABI, including exact microseconds (not ps/whole-second timestamps).
#[repr(C)]
struct BsdInfo {
    flags: u32,
    status: u32,
    xstatus: u32,
    pid: u32,
    ppid: u32,
    uid: u32,
    gid: u32,
    ruid: u32,
    rgid: u32,
    svuid: u32,
    svgid: u32,
    reserved: u32,
    comm: [u8; 16],
    name: [u8; 32],
    nfiles: u32,
    pgid: u32,
    jobc: u32,
    tdev: u32,
    tpgid: u32,
    nice: i32,
    start_seconds: u64,
    start_microseconds: u64,
}
const _: () = assert!(size_of::<BsdInfo>() == 136);
#[link(name = "proc")]
unsafe extern "C" {
    fn proc_pidinfo(pid: i32, flavor: i32, argument: u64, buffer: *mut c_void, size: i32) -> i32;
    fn proc_listpids(kind: u32, info: u32, buffer: *mut c_void, size: i32) -> i32;
}
unsafe extern "C" {
    fn sysctl(
        name: *mut i32,
        count: u32,
        old: *mut c_void,
        length: *mut usize,
        new: *mut c_void,
        new_length: usize,
    ) -> i32;
}
pub fn enable_reaping() -> io::Result<()> {
    Ok(())
}
pub fn list() -> io::Result<Vec<Pid>> {
    // Enumerate real-UID peers directly: full BSD info intentionally denies other users.
    // SAFETY: getuid has no arguments; null/zero requests the PROC_RUID_ONLY byte count.
    let uid = unsafe { super::getuid() };
    let bytes = unsafe { proc_listpids(5, uid, ptr::null_mut(), 0) };
    if bytes <= 0 {
        return Err(io::Error::last_os_error());
    }
    let mut capacity = bytes as usize / size_of::<Pid>() + 256;
    for _ in 0..8 {
        if capacity > 1_000_000 {
            return Err(io::Error::other("Process enumeration exceeded its bound."));
        }
        let mut pids = vec![0_i32; capacity];
        // SAFETY: vector is initialized/aligned for Pid; byte size matches allocation.
        let actual = unsafe {
            proc_listpids(
                5,
                uid,
                pids.as_mut_ptr().cast(),
                (capacity * size_of::<Pid>()) as i32,
            )
        };
        if actual <= 0 {
            return Err(io::Error::last_os_error());
        }
        if actual as usize >= capacity * size_of::<Pid>() {
            capacity *= 2;
            continue;
        }
        if actual as usize % size_of::<Pid>() != 0 {
            return Err(io::Error::other("Truncated PID enumeration."));
        }
        pids.truncate(actual as usize / size_of::<Pid>());
        pids.retain(|pid| *pid > 0);
        return Ok(pids);
    }
    Err(io::Error::other(
        "Process enumeration did not settle within its bound.",
    ))
}
pub fn identity(pid: Pid) -> io::Result<Option<Identity>> {
    let mut info = MaybeUninit::<BsdInfo>::zeroed();
    // SAFETY: kernel receives exact repr(C) buffer size; assume_init occurs only after full write.
    let actual = unsafe {
        proc_pidinfo(
            pid,
            3,
            1, // PROC_PIDTBSDINFO arg=1 includes zombies in exact identity inspection.
            info.as_mut_ptr().cast(),
            size_of::<BsdInfo>() as i32,
        )
    };
    if actual <= 0 {
        let error = io::Error::last_os_error();
        return if vanished(&error) {
            Ok(None)
        } else {
            Err(error)
        };
    }
    if actual as usize != size_of::<BsdInfo>() {
        return Err(io::Error::other("Truncated BSD process identity."));
    }
    // SAFETY: successful call returned the complete BsdInfo buffer.
    let info = unsafe { info.assume_init() };
    if info.pid != pid as u32 || info.start_microseconds >= 1_000_000 {
        return Err(io::Error::other("Invalid BSD process identity."));
    }
    let state = match info.status {
        1 => State::Idle,
        5 => State::Ended,
        2..=4 => State::Running,
        _ => State::Unknown,
    };
    Ok(Some(Identity {
        pid,
        birth: Birth(info.start_seconds, info.start_microseconds),
        parent: info.ppid as Pid,
        uid: info.ruid,
        state,
    }))
}
pub fn tagged(pid: Pid, tag: &str) -> io::Result<bool> {
    let mut argmax = 0_i32;
    let mut length = size_of::<i32>();
    let mut mib = [1, 8]; // CTL_KERN, KERN_ARGMAX.
    // SAFETY: output points to initialized i32; MIB length and output length match their allocations.
    if unsafe {
        sysctl(
            mib.as_mut_ptr(),
            2,
            (&mut argmax as *mut i32).cast(),
            &mut length,
            ptr::null_mut(),
            0,
        )
    } < 0
    {
        return Err(io::Error::last_os_error());
    }
    if length != size_of::<i32>() || !(4..=8 * 1024 * 1024).contains(&argmax) {
        return Err(io::Error::other("Invalid Darwin argument-space bound."));
    }
    let mut bytes = vec![0_u8; argmax as usize];
    let mut length = bytes.len();
    let mut mib = [1, 49, pid]; // CTL_KERN, KERN_PROCARGS2, pid.
    // SAFETY: output capacity and writable length are valid; no new data is passed.
    if unsafe {
        sysctl(
            mib.as_mut_ptr(),
            3,
            bytes.as_mut_ptr().cast(),
            &mut length,
            ptr::null_mut(),
            0,
        )
    } < 0
    {
        let error = io::Error::last_os_error();
        // A live fork-initializing process may return EINVAL/EIO, not merely EACCES.
        return Err(
            if matches!(error.raw_os_error(), Some(1 | 5 | 13 | 22) | Some(0)) {
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Darwin process arguments unavailable.",
                )
            } else {
                error
            },
        );
    }
    if length > bytes.len() {
        return Err(io::Error::other("Oversized Darwin procargs response."));
    }
    bytes.truncate(length);
    procargs_environment(&bytes).map(|environment| tagged_bytes(environment, tag))
}
