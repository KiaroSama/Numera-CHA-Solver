use super::super::metadata::{proc_info_result, procargs_environment, tagged_bytes};
use super::super::policy::{Birth, Identity, Pid, State};
use std::{
    ffi::c_void,
    io,
    mem::{MaybeUninit, size_of},
    ptr,
};

// Short BSD metadata and kernel generation IDs do not require full BSD-info privileges.
#[repr(C)]
struct ShortInfo {
    pid: u32,
    ppid: u32,
    pgid: u32,
    status: u32,
    comm: [u8; 16],
    flags: u32,
    uid: u32,
    gid: u32,
    ruid: u32,
    rgid: u32,
    svuid: u32,
    svgid: u32,
    reserved: u32,
}
#[repr(C)]
struct UniqueInfo {
    uuid: [u8; 16],
    unique: u64,
    parent_unique: u64,
    version: i32,
    original_parent_version: i32,
    reserved: [u64; 2],
}
const _: () = assert!(size_of::<ShortInfo>() == 64 && size_of::<UniqueInfo>() == 56);
#[link(name = "proc")]
unsafe extern "C" {
    fn proc_pidinfo(pid: i32, flavor: i32, argument: u64, buffer: *mut c_void, size: i32) -> i32;
    fn proc_listpids(kind: u32, info: u32, buffer: *mut c_void, size: i32) -> i32;
}
unsafe extern "C" {
    fn __error() -> *mut i32;
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
fn info<T>(pid: Pid, flavor: i32) -> io::Result<Option<T>> {
    let mut info = MaybeUninit::<T>::zeroed();
    // SAFETY: __error returns this thread's writable errno slot. Each caller selects the matching repr(C) ABI.
    let (actual, errno) = unsafe {
        *__error() = 0;
        let actual = proc_pidinfo(
            pid,
            flavor,
            1,
            info.as_mut_ptr().cast(),
            size_of::<T>() as i32,
        );
        (actual, *__error())
    };
    if !proc_info_result(pid, flavor, actual, size_of::<T>(), errno)? {
        return Ok(None);
    }
    // SAFETY: successful call wrote the complete ABI record, containing only integer/byte fields.
    Ok(Some(unsafe { info.assume_init() }))
}
pub fn identity(pid: Pid) -> io::Result<Option<Identity>> {
    let Some(before) = info::<UniqueInfo>(pid, 17)? else {
        return Ok(None);
    };
    let Some(short) = info::<ShortInfo>(pid, 13)? else {
        return Ok(None);
    };
    let Some(after) = info::<UniqueInfo>(pid, 17)? else {
        return Ok(None);
    };
    if short.pid != pid as u32 || before.unique == 0 || before.unique != after.unique {
        return Err(io::Error::other(
            "Darwin process identity changed during metadata inspection.",
        ));
    }
    let state = match short.status {
        1 => State::Idle,
        5 => State::Ended,
        2..=4 => State::Running,
        _ => State::Unknown,
    };
    Ok(Some(Identity {
        pid,
        // Kernel monotonic birth generation survives exec and distinguishes PID reuse exactly.
        birth: Birth(after.unique, 0),
        parent: short.ppid as Pid,
        uid: short.ruid,
        state,
    }))
}
pub fn tagged(pid: Pid, tag: &str) -> io::Result<bool> {
    let mut argmax = 0_i32;
    let mut length = size_of::<i32>();
    let mut mib = [1, 8];
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
    let mut mib = [1, 49, pid];
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
