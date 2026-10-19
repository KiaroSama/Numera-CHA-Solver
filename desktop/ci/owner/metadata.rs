//! Pure parsers for exact OS identity and environment metadata.
use super::policy::{Birth, Identity, OWNER_TAG, Pid, State};
use std::io;
/// libproc maps a failed syscall to zero and leaves errno set; zero alone is not ESRCH.
pub fn proc_info_result(
    pid: Pid,
    flavor: i32,
    returned: i32,
    expected: usize,
    errno: i32,
) -> io::Result<bool> {
    if returned <= 0 && errno == 3 {
        return Ok(false);
    }
    if returned as usize == expected {
        return Ok(true);
    }
    let kind = if returned <= 0 && errno != 0 {
        io::Error::from_raw_os_error(errno).kind()
    } else {
        io::ErrorKind::InvalidData
    };
    Err(io::Error::new(
        kind,
        format!(
            "proc_pidinfo pid={pid} flavor={flavor} argument=1 returned={returned} expected={expected} errno={errno} kind={kind:?}"
        ),
    ))
}
pub fn tagged_bytes(bytes: &[u8], tag: &str) -> bool {
    let expected = format!("{OWNER_TAG}={tag}");
    bytes
        .split(|byte| *byte == 0)
        .any(|entry| entry == expected.as_bytes())
}
pub fn procargs_environment(bytes: &[u8]) -> io::Result<&[u8]> {
    let count: [u8; 4] = bytes
        .get(..4)
        .ok_or_else(|| io::Error::other("Truncated procargs count."))?
        .try_into()
        .map_err(|_| io::Error::other("Invalid procargs count."))?;
    let count = i32::from_ne_bytes(count);
    if !(0..=1_000_000).contains(&count) {
        return Err(io::Error::other("Invalid argument count."));
    }
    let advance = |offset: usize| -> io::Result<usize> {
        bytes
            .get(offset..)
            .and_then(|tail| tail.iter().position(|byte| *byte == 0))
            .map(|length| offset + length + 1)
            .ok_or_else(|| io::Error::other("Unterminated procargs string."))
    };
    let mut offset = advance(4)?;
    while bytes.get(offset) == Some(&0) {
        offset += 1;
    }
    for _ in 0..count {
        offset = advance(offset)?;
    }
    bytes
        .get(offset..)
        .ok_or_else(|| io::Error::other("Missing procargs environment."))
}
pub fn linux_stat(pid: Pid, bytes: &[u8], uid: u32) -> io::Result<Identity> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| io::Error::other("Invalid proc stat encoding."))?;
    let (prefix, fields) = text
        .rsplit_once(") ")
        .ok_or_else(|| io::Error::other("Malformed proc stat."))?;
    if prefix
        .split_once(' ')
        .and_then(|(value, _)| value.parse::<Pid>().ok())
        != Some(pid)
    {
        return Err(io::Error::other("Proc stat PID mismatch."));
    }
    let fields: Vec<_> = fields.split_whitespace().collect();
    let parent = fields
        .get(1)
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| io::Error::other("Missing parent PID."))?;
    let birth = fields
        .get(19)
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| io::Error::other("Missing birth ticks."))?;
    let state = match fields.first().copied() {
        Some("Z" | "X" | "x") => State::Ended,
        Some("R" | "S" | "D" | "T" | "t" | "W" | "I" | "P") => State::Running,
        _ => State::Unknown,
    };
    Ok(Identity {
        pid,
        birth: Birth(birth, 0),
        parent,
        uid,
        state,
    })
}
