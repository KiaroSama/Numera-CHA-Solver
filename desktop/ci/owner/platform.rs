//! Minimal POSIX ABI; no Cargo dependency is needed by the pre-fetch bootstrap.
use super::policy::{Identity, Pid, Source};
use std::{
    fs,
    io::{self, Read},
    os::fd::AsRawFd,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod os;
#[cfg(target_os = "macos")]
#[path = "darwin.rs"]
mod os;

pub const KILL: i32 = 9;
#[cfg(target_os = "linux")]
pub const STOP: i32 = 19;
#[cfg(target_os = "macos")]
pub const STOP: i32 = 17;
static INTERRUPTED: AtomicBool = AtomicBool::new(false);
unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
    fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
    fn setsid() -> i32;
    fn getuid() -> u32;
    fn fcntl(fd: i32, command: i32, ...) -> i32;
    fn signal(number: i32, handler: usize) -> usize;
}
extern "C" fn interrupt(_: i32) {
    INTERRUPTED.store(true, Ordering::Relaxed);
}
pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::Relaxed)
}
pub fn install_signals() -> io::Result<()> {
    for number in [1, 2, 15] {
        // SAFETY: handler has C signal ABI and only writes a lock-free atomic flag.
        if unsafe { signal(number, interrupt as *const () as usize) } == usize::MAX {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}
pub fn session() -> io::Result<()> {
    // SAFETY: called in the single-threaded child's pre_exec; setsid has no pointer arguments.
    if unsafe { setsid() } < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
pub fn nonblocking(file: &impl AsRawFd) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    const NONBLOCK: i32 = 2048;
    #[cfg(target_os = "macos")]
    const NONBLOCK: i32 = 4;
    // SAFETY: descriptor remains owned by caller; F_GETFL and F_SETFL accept integer flags.
    let flags = unsafe { fcntl(file.as_raw_fd(), 3) };
    if flags < 0 || unsafe { fcntl(file.as_raw_fd(), 4, flags | NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
pub fn reap(pid: Pid) -> io::Result<()> {
    let mut status = 0;
    // SAFETY: status is writable; positive PID prevents consuming the root's wait status.
    let result = unsafe { waitpid(pid, &mut status, 1) };
    if result < 0 {
        let error = io::Error::last_os_error();
        if !matches!(error.raw_os_error(), Some(10) | Some(3)) {
            return Err(error);
        }
    }
    Ok(())
}
pub fn signal_identity(source: &mut Native, known: Identity, number: i32) -> io::Result<()> {
    let current = source.identity(known.pid)?;
    if !super::policy::signal_allowed(known, current).map_err(io::Error::other)? {
        return Ok(());
    }
    // SAFETY: strictly positive PID and exact birth identity were checked immediately before signal.
    if unsafe { kill(known.pid, number) } < 0 {
        let error = io::Error::last_os_error();
        if !super::policy::vanished(&error) {
            return Err(error);
        }
    }
    Ok(())
}
pub struct Native {
    start: Instant,
}
impl Native {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
    pub fn enable_reaping(&self) -> io::Result<()> {
        os::enable_reaping()
    }
    pub fn uid(&self) -> u32 {
        // SAFETY: getuid takes no arguments and has no failure sentinel.
        unsafe { getuid() }
    }
}
impl Source for Native {
    fn list(&mut self) -> io::Result<Vec<Pid>> {
        os::list()
    }
    fn identity(&mut self, pid: Pid) -> io::Result<Option<Identity>> {
        os::identity(pid)
    }
    fn tagged(&mut self, pid: Pid, tag: &str) -> io::Result<bool> {
        os::tagged(pid, tag)
    }
    fn now(&self) -> Duration {
        self.start.elapsed()
    }
    fn pause(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
    fn darwin(&self) -> bool {
        cfg!(target_os = "macos")
    }
    fn subreaper(&self) -> bool {
        cfg!(target_os = "linux")
    }
}
pub fn bounded_file(path: &str, limit: usize) -> io::Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::other(
            "Process metadata exceeded its safety ceiling.",
        ));
    }
    Ok(bytes)
}
pub fn random_tag() -> io::Result<String> {
    let mut bytes = [0; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
