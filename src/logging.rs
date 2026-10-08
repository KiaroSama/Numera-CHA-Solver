use chrono::Utc;
use regex::Regex;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

const MAX_LOG_BYTES: u64 = 2_000_000;
static NEXT_RUN: AtomicU64 = AtomicU64::new(1);
// ponytail: one process-wide sink; per-runtime sinks only if concurrent independent runtimes need them.
static LOG: Mutex<Option<Sink>> = Mutex::new(None);
struct Sink {
    run: u64,
    file: Option<File>,
    path: Option<PathBuf>,
    bytes: u64,
    debug: bool,
}

#[derive(Debug)]
pub struct LogGuard {
    run: u64,
    path: Option<PathBuf>,
}
impl LogGuard {
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
    pub fn close(&mut self) {
        if let Ok(mut state) = LOG.lock()
            && state.as_ref().is_some_and(|sink| sink.run == self.run)
            && let Some(mut sink) = state.take()
            && let Some(file) = sink.file.as_mut()
        {
            let _ = file.flush();
        }
    }
}
impl Drop for LogGuard {
    fn drop(&mut self) {
        self.close();
    }
}

pub fn initialize(directory: &Path, name: &str, debug: bool) -> LogGuard {
    let run = NEXT_RUN.fetch_add(1, Ordering::Relaxed);
    let safe_name: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .take(64)
        .collect();
    let safe_name = if safe_name.is_empty() {
        "numera-cha"
    } else {
        &safe_name
    };
    let opened = fs::create_dir_all(directory).and_then(|_| {
        let stem = format!("{safe_name}_{}_UTC", Utc::now().format("%Y-%m-%d_%H-%M-%S"));
        for suffix in 0..100 {
            let filename = if suffix == 0 {
                format!("{stem}.log")
            } else {
                format!("{stem}_{}_{run}_{suffix}.log", std::process::id())
            };
            let path = directory.join(filename);
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => return Ok((path, file)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Unique log filename unavailable.",
        ))
    });
    let (path, file) = match opened {
        Ok((path, file)) => (Some(path), Some(file)),
        Err(_) => (None, None),
    };
    let guard = LogGuard {
        run,
        path: path.clone(),
    };
    if let Ok(mut state) = LOG.lock() {
        if let Some(file) = state.as_mut().and_then(|s| s.file.as_mut()) {
            let _ = file.flush();
        }
        *state = Some(Sink {
            run,
            path,
            file,
            bytes: 0,
            debug,
        });
    }
    if guard.path.is_some() {
        event("INFO", "startup", "Diagnostic logging initialized.");
    } else {
        event(
            "WARNING",
            "startup",
            "File logging unavailable; stderr fallback active.",
        );
    }
    guard
}

pub fn event(level: &str, component: &str, message: &str) {
    let level = match level {
        "DEBUG" | "INFO" | "WARNING" | "ERROR" | "CRITICAL" => level,
        _ => "INFO",
    };
    let component = redact(component).replace(['\r', '\n'], " ");
    let message = redact(message).replace('\r', "\\r").replace('\n', "\\n");
    let line = format!(
        "[{} UTC] [{level}] [{component}] {message}\n",
        Utc::now().format("%Y-%m-%d %H:%M:%S")
    );
    let mut state = LOG.lock().ok();
    let sink = state.as_mut().and_then(|s| s.as_mut());
    if level == "DEBUG" && !sink.as_ref().is_some_and(|s| s.debug) {
        return;
    }
    // Diagnostic stderr keeps the CLI's JSON stdout machine-readable, as in the source runtime.
    let _ = io::stderr().lock().write_all(line.as_bytes());
    if let Some(sink) = sink {
        if sink.bytes + line.len() as u64 > MAX_LOG_BYTES && rotate(sink).is_err() {
            sink.file = None;
            let _ = io::stderr()
                .lock()
                .write_all(b"File logging unavailable; stderr fallback active.\n");
        }
        if let Some(file) = sink.file.as_mut() {
            if file
                .write_all(line.as_bytes())
                .and_then(|_| file.flush())
                .is_err()
            {
                sink.file = None;
                let _ = io::stderr()
                    .lock()
                    .write_all(b"File logging unavailable; stderr fallback active.\n");
            } else {
                sink.bytes += line.len() as u64;
            }
        }
    }
}

pub fn redact(text: &str) -> String {
    static PATTERNS: OnceLock<Option<(Regex, Regex, Regex)>> = OnceLock::new();
    let Some((bearer, sensitive, url)) = PATTERNS.get_or_init(|| Some((
        Regex::new(r"(?i)bearer\s+\S+").ok()?,
        Regex::new(r#"(?i)(["']?(?:token|secret|password|authorization|cookies?|api[_-]?key|response)["']?\s*[:=]\s*)(?:"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|[^\s,;}]+)"#).ok()?,
        Regex::new(r"https?://\S+").ok()?,
    ))) else { return "Diagnostic redaction unavailable; sensitive details suppressed.".into(); };
    let safe = bearer.replace_all(text, "Bearer [REDACTED]");
    let safe = sensitive.replace_all(&safe, "${1}[REDACTED]");
    url.replace_all(&safe, "[URL REDACTED]").into_owned()
}

fn rotate(sink: &mut Sink) -> io::Result<()> {
    if sink.file.is_none() {
        return Ok(());
    }
    if let Some(mut file) = sink.file.take() {
        file.flush()?;
    }
    let path = sink
        .path
        .as_ref()
        .ok_or_else(|| io::Error::other("Log path unavailable."))?;
    let numbered = |number| PathBuf::from(format!("{}.{}", path.display(), number));
    if numbered(2).exists() {
        fs::remove_file(numbered(2))?;
    }
    if numbered(1).exists() {
        fs::rename(numbered(1), numbered(2))?;
    }
    fs::rename(path, numbered(1))?;
    sink.file = Some(OpenOptions::new().create_new(true).write(true).open(path)?);
    sink.bytes = 0;
    Ok(())
}
