//! Std-only counterpart of the project's UTF-8 per-run diagnostic sink.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
pub struct Log {
    file: Option<File>,
    bytes: usize,
}
impl Log {
    pub fn new(directory: &Path) -> Self {
        let open = || -> io::Result<File> {
            fs::create_dir_all(directory)?;
            let stamp = timestamp().replace(' ', "_").replace(':', "-");
            for suffix in 0..100 {
                let path = directory.join(format!(
                    "owned-command_{stamp}_{}_{suffix}.log",
                    std::process::id()
                ));
                match OpenOptions::new().write(true).create_new(true).open(path) {
                    Ok(file) => return Ok(file),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error),
                }
            }
            Err(io::Error::other("Unique log filename unavailable."))
        };
        let mut log = Self {
            file: open().ok(),
            bytes: 0,
        };
        if log.file.is_none() {
            eprintln!("File logging unavailable; stderr fallback active.");
        }
        log.event("INFO", "startup", "Hosted process owner started.");
        log
    }
    pub fn event(&mut self, level: &str, component: &str, message: &str) {
        let safe = redact(message).replace('\r', "\\r").replace('\n', "\\n");
        let line = format!("[{}] [{level}] [{component}] {safe}\n", timestamp());
        let _ = io::stderr().lock().write_all(line.as_bytes());
        // Match the runtime's bounded sink without introducing rotation or dependencies into bootstrap.
        if self.bytes + line.len() > 2_000_000 {
            return;
        }
        if let Some(file) = self.file.as_mut() {
            if file
                .write_all(line.as_bytes())
                .and_then(|_| file.flush())
                .is_err()
            {
                self.file = None;
                eprintln!("File logging unavailable; stderr fallback active.");
            } else {
                self.bytes += line.len();
            }
        }
    }
}
impl Drop for Log {
    fn drop(&mut self) {
        if let Some(file) = self.file.as_mut() {
            let _ = file.flush();
        }
    }
}

pub fn redact(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let boundary = index == 0 || !bytes[index - 1].is_ascii_alphanumeric();
        if boundary {
            let tail = &bytes[index..];
            let bearer = tail
                .get(..6)
                .is_some_and(|part| part.eq_ignore_ascii_case(b"bearer"))
                && tail.get(6).is_some_and(u8::is_ascii_whitespace);
            let url = [b"http://".as_slice(), b"https://".as_slice()]
                .iter()
                .any(|key| {
                    tail.get(..key.len())
                        .is_some_and(|part| part.eq_ignore_ascii_case(key))
                });
            if bearer || url {
                if bearer {
                    output.extend_from_slice(b"Bearer [REDACTED]");
                    index += 6;
                } else {
                    output.extend_from_slice(b"[URL REDACTED]");
                }
                while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
                    index += 1;
                }
                while bytes
                    .get(index)
                    .is_some_and(|byte| !byte.is_ascii_whitespace())
                {
                    index += 1;
                }
                continue;
            }
            let mut matched = None;
            for key in [
                "token",
                "secret",
                "password",
                "authorization",
                "cookie",
                "cookies",
                "api_key",
                "api-key",
                "response",
            ] {
                if !tail
                    .get(..key.len())
                    .is_some_and(|part| part.eq_ignore_ascii_case(key.as_bytes()))
                {
                    continue;
                }
                let mut end = index + key.len();
                if matches!(bytes.get(end), Some(b'\'' | b'"')) {
                    end += 1;
                }
                while bytes.get(end).is_some_and(u8::is_ascii_whitespace) {
                    end += 1;
                }
                if !matches!(bytes.get(end), Some(b':' | b'=')) {
                    continue;
                }
                end += 1;
                while bytes.get(end).is_some_and(u8::is_ascii_whitespace) {
                    end += 1;
                }
                matched = Some(end);
                break;
            }
            if let Some(end) = matched {
                output.extend_from_slice(&bytes[index..end]);
                output.extend_from_slice(b"[REDACTED]");
                index = end;
                let quote = bytes
                    .get(index)
                    .copied()
                    .filter(|byte| matches!(byte, b'\'' | b'"'));
                if quote.is_some() && matches!(bytes.get(index), Some(b'\'' | b'"')) {
                    index += 1;
                }
                while let Some(&byte) = bytes.get(index) {
                    if quote == Some(byte) {
                        index += 1;
                        break;
                    }
                    if quote.is_none()
                        && (byte.is_ascii_whitespace() || matches!(byte, b',' | b';' | b'}'))
                    {
                        break;
                    }
                    if quote.is_some() && byte == b'\\' && index + 1 < bytes.len() {
                        index += 1;
                    }
                    index += 1;
                }
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}
fn timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = (seconds / 86400) as i64 + 719468;
    let era = days / 146097;
    let day = days - era * 146097;
    let year = (day - day / 1460 + day / 36524 - day / 146096) / 365;
    let doy = day - (365 * year + year / 4 - year / 100);
    let month = (5 * doy + 2) / 153;
    let date = doy - (153 * month + 2) / 5 + 1;
    let month = month + if month < 10 { 3 } else { -9 };
    let year = year + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{date:02} {:02}:{:02}:{:02} UTC",
        seconds / 3600 % 24,
        seconds / 60 % 60,
        seconds % 60
    )
}
