//! Hosted-only real process regressions, compiled into the std-only bootstrap once.
use super::{
    platform::{self, Native},
    policy::{Birth, Identity, Result, Source, State},
    runner::{self, Options},
};
use std::{
    ffi::OsString,
    fs,
    io::{self, Read, Write},
    os::unix::process::CommandExt,
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};
unsafe extern "C" {
    fn fork() -> i32;
    fn _exit(status: i32) -> !;
}

fn block() -> Result<i32> {
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .map_err(|_| "Idle fixture socket unavailable.")?;
    listener
        .accept()
        .map_err(|_| "Idle fixture socket failed.")?;
    Err("Idle fixture unexpectedly received a connection.".into())
}
fn gate() -> Result<()> {
    let mut byte = [0];
    io::stdin()
        .read_exact(&mut byte)
        .map_err(|_| "Fixture identity gate closed.")?;
    if byte != [b'1'] {
        return Err("Fixture identity gate rejected.".into());
    }
    Ok(())
}
pub fn fixture(args: &[OsString]) -> Result<i32> {
    if args.len() != 1 {
        return Err("Expected one owner fixture mode.".into());
    }
    let mode = args[0].to_str().ok_or("Invalid fixture mode.")?;
    match mode {
        "success" => {
            println!("owned-success");
            Ok(0)
        }
        "nonzero" => Ok(7),
        "idle" => block(),
        "idle-gated" => {
            gate()?;
            block()
        }
        "orphan" | "detached" | "detached-live" => {
            let mut command = Command::new(
                std::env::current_exe().map_err(|_| "Fixture executable unavailable.")?,
            );
            command
                .args(["--fixture", "idle-gated"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            if mode != "orphan" {
                // SAFETY: pre_exec only invokes the async-signal-safe setsid syscall.
                unsafe {
                    command.pre_exec(platform::session);
                }
            }
            let mut child = command
                .spawn()
                .map_err(|_| "Descendant fixture could not start.")?;
            let mut source = Native::new();
            let identity = source
                .identity(child.id() as i32)
                .map_err(|_| "Descendant fixture identity unavailable.")?
                .ok_or("Descendant fixture disappeared before gate release.")?;
            println!("{} {}", identity.pid, identity.birth.encode());
            io::stdout()
                .flush()
                .map_err(|_| "Descendant fixture capture flush failed.")?;
            child
                .stdin
                .take()
                .ok_or("Descendant fixture gate unavailable.")?
                .write_all(b"1")
                .map_err(|_| "Descendant fixture gate release failed.")?;
            if mode == "detached-live" {
                block()
            } else {
                Ok(0)
            }
        }
        "ended" => {
            // SAFETY: fixture is single-threaded; child invokes only _exit and never touches Rust state.
            let pid = unsafe { fork() };
            if pid < 0 {
                return Err("Ended-child fixture fork failed.".into());
            }
            if pid == 0 {
                unsafe { _exit(0) }
            }
            let mut source = Native::new();
            let deadline = source.now() + Duration::from_secs(2);
            let identity = loop {
                let identity = source
                    .identity(pid)
                    .map_err(|_| "Ended fixture identity inspection failed.")?
                    .ok_or("Ended fixture vanished before zombie observation.")?;
                if identity.state == State::Ended {
                    break identity;
                }
                if source.now() >= deadline {
                    return Err("Ended-child fixture did not reach zombie state.".into());
                }
                // Observe kernel exit without reaping; the owner must not count a zombie as executing.
                source.pause(Duration::from_millis(10));
            };
            println!("{} {}", identity.pid, identity.birth.encode());
            Ok(0)
        }
        _ => Err("Unknown owner fixture mode.".into()),
    }
}
fn captured_identity(path: &Path) -> Result<Option<Identity>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path).map_err(|_| "Fixture capture unreadable.")?;
    let fields: Vec<_> = content.split_whitespace().collect();
    if fields.len() != 2 {
        return Err("Fixture identity capture malformed.".into());
    }
    let pid = fields[0]
        .parse::<i32>()
        .ok()
        .filter(|pid| *pid > 0)
        .ok_or("Fixture captured invalid PID.")?;
    let birth = Birth::decode(fields[1]).ok_or("Fixture captured invalid birth identity.")?;
    Ok(Some(Identity {
        pid,
        birth,
        parent: 0,
        uid: 0,
        state: State::Running,
    }))
}
fn sweep(path: &Path, require_gone: bool) -> Result<()> {
    let Some(known) = captured_identity(path)? else {
        return Ok(());
    };
    let mut source = Native::new();
    let mut survived = false;
    if let Some(current) = source
        .identity(known.pid)
        .map_err(|_| "Fixture survivor identity unavailable.")?
    {
        if known.same(current) {
            survived = true;
            platform::signal_identity(&mut source, known, platform::KILL)
                .map_err(|_| "Fixture survivor cleanup signal failed.")?;
            let deadline = source.now() + Duration::from_secs(5);
            loop {
                if let Some(current) = source
                    .identity(known.pid)
                    .map_err(|_| "Fixture survivor cleanup observation failed.")?
                {
                    if !known.same(current) {
                        break;
                    }
                    if current.state == State::Ended {
                        platform::reap(known.pid).map_err(|_| "Fixture survivor wait failed.")?;
                    }
                } else {
                    break;
                }
                if source.now() >= deadline {
                    return Err("Fixture descendant survived emergency cleanup.".into());
                }
                source.pause(Duration::from_millis(10));
            }
        }
    }
    if require_gone && survived {
        return Err("Owned detached child survived normal cleanup.".into());
    }
    Ok(())
}
pub fn verify(work: &Path) -> Result<()> {
    fs::create_dir(work).map_err(|_| "Owner contract requires a new, unused work directory.")?;
    let work = work
        .canonicalize()
        .map_err(|_| "Contract work directory unavailable.")?;
    let executable =
        std::env::current_exe().map_err(|_| "Owner contract executable unavailable.")?;
    for (mode, expected) in [
        ("success", 0),
        ("idle", 124),
        ("orphan", 1),
        ("detached", 1),
        ("detached-live", 124),
        ("ended", 0),
        ("nonzero", 7),
    ] {
        let capture = work.join(format!("{mode}.txt"));
        let result = runner::run(Options {
            cwd: work.clone(),
            work: work.clone(),
            wall: 5,
            idle: 2,
            capture: Some(capture.clone()),
            command: vec![
                executable.clone().into_os_string(),
                "--fixture".into(),
                mode.into(),
            ],
        });
        let descendant = matches!(mode, "orphan" | "detached" | "detached-live" | "ended");
        let swept = if descendant {
            sweep(&capture, result.is_ok()).and_then(|()| {
                if captured_identity(&capture)?.is_none() {
                    return Err("Descendant fixture did not publish its exact identity.".into());
                }
                Ok(())
            })
        } else {
            Ok(())
        };
        let actual = match (result, swept) {
            (Ok(code), Ok(())) => code,
            (Err(error), Ok(())) => return Err(error),
            (Ok(_), Err(cleanup)) => return Err(cleanup),
            (Err(error), Err(cleanup)) => {
                return Err(format!("{error} Emergency cleanup also failed: {cleanup}"));
            }
        };
        if actual != expected {
            return Err(format!(
                "Owner mode={mode} returned={actual}, expected={expected}."
            ));
        }
        if mode == "success"
            && fs::read_to_string(&capture)
                .map_err(|_| "Success fixture capture unavailable.")?
                .trim()
                != "owned-success"
        {
            return Err("Owner success capture mismatch.".into());
        }
        for entry in fs::read_dir(&work).map_err(|_| "Owner record sweep unavailable.")? {
            let entry = entry.map_err(|_| "Owner record sweep failed.")?;
            if entry.file_name().to_string_lossy().starts_with("owner-") {
                return Err("Completed owner identity record survived cleanup.".into());
            }
        }
        eprintln!("POSIX owner mode={mode} exit_code={actual} cleanup=verified");
    }
    eprintln!(
        "POSIX owner success/nonzero/idle/orphan/detached-session/ended-child cleanup regression passed."
    );
    Ok(())
}
