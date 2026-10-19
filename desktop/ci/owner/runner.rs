use super::{
    log::Log,
    platform::{self, Native},
    policy::{OWNER_TAG, Result, Source, State, Tree},
};
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::{
        fs::OpenOptionsExt,
        process::{CommandExt, ExitStatusExt},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

const OUTPUT_LIMIT: usize = 32 * 1024 * 1024;
pub struct Options {
    pub cwd: PathBuf,
    pub work: PathBuf,
    pub wall: u64,
    pub idle: u64,
    pub capture: Option<PathBuf>,
    pub command: Vec<OsString>,
}
impl Options {
    pub fn parse(args: &[OsString]) -> Result<Self> {
        let (mut cwd, mut work, mut wall, mut idle, mut capture) = (None, None, None, None, None);
        let mut index = 0;
        while index < args.len() && args[index] != "--" {
            let value = args
                .get(index + 1)
                .ok_or("Missing bounded command parameter.")?;
            match args[index].to_str() {
                Some("--cwd") if cwd.is_none() => cwd = Some(PathBuf::from(value)),
                Some("--work") if work.is_none() => work = Some(PathBuf::from(value)),
                Some("--capture") if capture.is_none() => capture = Some(PathBuf::from(value)),
                Some("--wall") if wall.is_none() => {
                    wall = value.to_str().and_then(|value| value.parse::<u64>().ok())
                }
                Some("--idle") if idle.is_none() => {
                    idle = value.to_str().and_then(|value| value.parse::<u64>().ok())
                }
                _ => return Err("Unknown or repeated bounded command parameter.".into()),
            }
            index += 2;
        }
        let wall = wall
            .filter(|value| (1..=1800).contains(value))
            .ok_or("Wall bound must be 1..1800 seconds.")?;
        let idle = idle
            .filter(|value| (1..=wall).contains(value))
            .ok_or("Idle bound must be 1..wall seconds.")?;
        let command = args
            .get(index + 1..)
            .filter(|items| !items.is_empty())
            .ok_or("Expected -- followed by a command.")?
            .to_vec();
        let cwd = cwd
            .ok_or("Expected --cwd.")?
            .canonicalize()
            .map_err(|_| "Command working directory unavailable.")?;
        if !cwd.is_dir() {
            return Err("Command working directory is not a directory.".into());
        }
        let work = work.ok_or("Expected --work.")?;
        if !work.is_absolute()
            || capture
                .as_ref()
                .is_some_and(|path: &PathBuf| !path.is_absolute())
        {
            return Err("Owner work and capture paths must be absolute.".into());
        }
        Ok(Self {
            cwd,
            work,
            wall,
            idle,
            capture,
            command,
        })
    }
}
pub fn launch(args: &[OsString]) -> Result<i32> {
    let command = args.strip_prefix(&[OsString::from("--")]).unwrap_or(args);
    if command.is_empty() {
        return Err("Owned launch command unavailable.".into());
    }
    let mut byte = [0];
    io::stdin()
        .read_exact(&mut byte)
        .map_err(|_| "Owned launch gate closed before identity capture.")?;
    if byte != [b'1'] {
        return Err("Owned launch identity gate rejected.".into());
    }
    // Command::exec replaces this gated identity and detaches stdin without a shell or interpreter.
    let error = Command::new(&command[0])
        .args(&command[1..])
        .stdin(Stdio::null())
        .exec();
    Err(format!("Owned command exec failed: {error}"))
}
fn record(path: &Path, tree: &Tree) -> Result<()> {
    let members = tree
        .members
        .values()
        .map(|identity| {
            format!(
                "{{\"pid\":{},\"created\":\"{}\"}}",
                identity.pid,
                identity.birth.encode()
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let temporary = path.with_extension("json.tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|_| "Owner identity record unavailable.")?;
    file.write_all(
        format!(
            "{{\"root_pid\":{},\"members\":[{members}]}}\n",
            tree.root.pid
        )
        .as_bytes(),
    )
    .map_err(|_| "Owner identity record write failed.")?;
    file.flush()
        .map_err(|_| "Owner identity record flush failed.")?;
    fs::rename(&temporary, path).map_err(|_| "Owner identity record publication failed.".into())
}
fn snapshot(source: &mut Native, tree: &mut Tree, path: &Path) -> Result<()> {
    tree.snapshot(source)?;
    record(path, tree)
}
fn reap(source: &mut Native, tree: &Tree, child: &mut Child) -> Result<()> {
    child.try_wait().map_err(|_| "Owned root wait failed.")?;
    for known in tree
        .members
        .values()
        .filter(|identity| identity.pid != tree.root.pid)
    {
        if let Some(current) = source
            .identity(known.pid)
            .map_err(|_| "Owned exit-state inspection failed.")?
        {
            if !known.same(current) {
                return Err("Refusing to reap a reused PID.".into());
            }
            if current.state == State::Ended {
                platform::reap(known.pid).map_err(|_| "Owned descendant wait failed.")?;
            }
        }
    }
    Ok(())
}
fn cleanup(source: &mut Native, tree: &mut Tree, child: &mut Child, path: &Path) -> Result<()> {
    let deadline = source.now() + Duration::from_secs(5);
    tree.deadline = deadline;
    let mut failure = None;
    if let Err(error) = snapshot(source, tree, path) {
        failure = Some(error);
    }
    // Stop each discovered generation before killing: late births cannot fork during teardown.
    let mut frozen = std::collections::BTreeSet::new();
    loop {
        let parents: Vec<_> = tree
            .members
            .values()
            .filter(|identity| !frozen.contains(&identity.pid))
            .copied()
            .collect();
        if parents.is_empty() {
            break;
        }
        for known in parents {
            if let Err(error) = platform::signal_identity(source, known, platform::STOP) {
                failure = Some(error.to_string());
            }
            frozen.insert(known.pid);
        }
        if let Err(error) = snapshot(source, tree, path) {
            failure = Some(error);
            break;
        }
        if source.now() >= deadline {
            failure = Some("Owned freeze/discovery deadline exceeded.".into());
            break;
        }
    }
    loop {
        for known in tree.members.values().rev().copied().collect::<Vec<_>>() {
            if let Err(error) = platform::signal_identity(source, known, platform::KILL) {
                failure = Some(error.to_string());
            }
        }
        if let Err(error) = reap(source, tree, child) {
            failure = Some(error);
        }
        if let Err(error) = snapshot(source, tree, path) {
            failure = Some(error);
        }
        match tree.live(source, false) {
            Ok(live) if live.is_empty() => {
                return if let Some(error) = failure {
                    Err(format!(
                        "Owned discovery incomplete; known children reaped but run remains failed: {error}"
                    ))
                } else {
                    Ok(())
                };
            }
            Err(error) => failure = Some(error),
            _ => {}
        }
        if source.now() >= deadline {
            return Err(format!(
                "Owned descendants survived verified identity cleanup: {}",
                failure.unwrap_or_default()
            ));
        }
        // Bounded kernel exit/reaping readiness, not a blind startup delay.
        source.pause(Duration::from_millis(20).min(deadline.saturating_sub(source.now())));
    }
}
struct Output {
    bytes: usize,
    stdout_log: Vec<u8>,
    stderr_log: Vec<u8>,
    capture: Option<File>,
    stdout_done: bool,
    stderr_done: bool,
}
impl Output {
    fn read(&mut self, reader: &mut impl Read, stdout: bool) -> Result<bool> {
        let mut chunk = [0; 65536];
        let mut progress = false;
        // Limit each turn so a continuously writing child cannot starve wall/discovery checks.
        for _ in 0..8 {
            match reader.read(&mut chunk) {
                Ok(0) => {
                    if stdout {
                        self.stdout_done = true;
                    } else {
                        self.stderr_done = true;
                    }
                    break;
                }
                Ok(size) => {
                    self.bytes += size;
                    if self.bytes > OUTPUT_LIMIT {
                        return Err(
                            "Owned command output exceeded its 32 MiB safety ceiling.".into()
                        );
                    }
                    progress = true;
                    if stdout && self.capture.is_some() {
                        self.capture
                            .as_mut()
                            .ok_or("Capture unavailable.")?
                            .write_all(&chunk[..size])
                            .map_err(|_| "Owned stdout capture failed.")?;
                    } else if stdout {
                        self.stdout_log.extend_from_slice(&chunk[..size]);
                    } else {
                        self.stderr_log.extend_from_slice(&chunk[..size]);
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => return Err("Owned command output read failed.".into()),
            }
        }
        Ok(progress)
    }
}
pub fn run(options: Options) -> Result<i32> {
    fs::create_dir_all(&options.work).map_err(|_| "Owner work directory unavailable.")?;
    let mut log = Log::new(&options.work.join("logs"));
    let mut source = Native::new();
    source
        .enable_reaping()
        .map_err(|_| "Owned descendant reaping could not be enabled.")?;
    platform::install_signals().map_err(|_| "Owned interruption handlers unavailable.")?;
    let owner = source
        .identity(std::process::id() as i32)
        .map_err(|_| "Owner process identity unavailable.")?
        .ok_or("Owner process disappeared.")?;
    if owner.uid != source.uid() {
        return Err("Owner real UID identity mismatch.".into());
    }
    let tag = platform::random_tag().map_err(|_| "Owner random identity unavailable.")?;
    let capture = options
        .capture
        .as_ref()
        .map(File::create)
        .transpose()
        .map_err(|_| "Owned stdout capture unavailable.")?;
    let mut output = Output {
        bytes: 0,
        stdout_log: Vec::new(),
        stderr_log: Vec::new(),
        capture,
        stdout_done: false,
        stderr_done: false,
    };
    let mut command =
        Command::new(std::env::current_exe().map_err(|_| "Owner executable unavailable.")?);
    command
        .arg("--launch")
        .arg("--")
        .args(&options.command)
        .current_dir(&options.cwd)
        .env(OWNER_TAG, &tag)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: pre_exec calls only async-signal-safe setsid and constructs no shared mutable state.
    unsafe {
        command.pre_exec(platform::session);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "Owned command could not start.")?;
    let mut gate = child.stdin.take();
    let path = options.work.join(format!("owner-{}.json", child.id()));
    let root = source.identity(child.id() as i32);
    let tracked = root.ok().flatten().and_then(|root| {
        Tree::new(owner, root, tag.clone(), Duration::from_secs(options.wall)).ok()
    });
    let mut tree = match tracked {
        Some(tree) => tree,
        None => {
            drop(gate.take()); // Unopened trampoline gate proves there can be no executed descendants.
            // No numeric-PID kill without an identity: EOF makes the blocked trampoline exit.
            let deadline = source.now() + Duration::from_secs(5);
            while child
                .try_wait()
                .map_err(|_| "Unopened launch gate wait failed.")?
                .is_none()
            {
                if source.now() >= deadline {
                    return Err("Unopened launch trampoline survived cleanup.".into());
                }
                source.pause(Duration::from_millis(10));
            }
            return Err("Root process identity could not be captured before exec.".into());
        }
    };
    let execution = (|| -> Result<i32> {
        // The direct Child handle and still-closed private pipe establish ownership even during
        // Darwin SIDL, where procargs can transiently deny access before exec initialization ends.
        tree.check_gated_root(&mut source)?;
        snapshot(&mut source, &mut tree, &path)?;
        tree.check_gated_root(&mut source)?;
        gate.as_mut()
            .ok_or("Owned identity gate unavailable.")?
            .write_all(b"1")
            .map_err(|_| "Owned identity gate release failed.")?;
        drop(gate.take());
        let mut stdout = child.stdout.take().ok_or("Owned stdout unavailable.")?;
        let mut stderr = child.stderr.take().ok_or("Owned stderr unavailable.")?;
        platform::nonblocking(&stdout).map_err(|_| "Owned stdout nonblocking mode unavailable.")?;
        platform::nonblocking(&stderr).map_err(|_| "Owned stderr nonblocking mode unavailable.")?;
        let mut progress = Duration::ZERO;
        loop {
            snapshot(&mut source, &mut tree, &path)?;
            if platform::interrupted()
                || source.now() >= tree.deadline
                || source.now().saturating_sub(progress) >= Duration::from_secs(options.idle)
            {
                log.event(
                    "ERROR",
                    "deadline",
                    "Owned command interrupted or exceeded wall/idle deadline.",
                );
                return Ok(124);
            }
            let mut advanced = false;
            if !output.stdout_done {
                advanced |= output.read(&mut stdout, true)?;
            }
            if !output.stderr_done {
                advanced |= output.read(&mut stderr, false)?;
            }
            if advanced {
                progress = source.now();
            }
            let status = child
                .try_wait()
                .map_err(|_| "Owned root exit observation failed.")?;
            if let Some(status) = status {
                snapshot(&mut source, &mut tree, &path)?;
                reap(&mut source, &tree, &mut child)?;
                let executing = tree.live(&mut source, true)?;
                if !executing.is_empty() {
                    for identity in executing {
                        log.event(
                            "ERROR",
                            "discovery",
                            &format!(
                                "Executing descendant after root exit pid={} birth={}",
                                identity.pid,
                                identity.birth.encode()
                            ),
                        );
                    }
                    return Ok(1);
                }
                if output.stdout_done && output.stderr_done {
                    return Ok(status
                        .code()
                        .unwrap_or_else(|| 128 + status.signal().unwrap_or(1)));
                }
            }
            // Nonblocking pipes plus bounded kernel readiness polling avoid helper threads that can leak.
            let remaining = tree.deadline.saturating_sub(source.now()).min(
                Duration::from_secs(options.idle)
                    .saturating_sub(source.now().saturating_sub(progress)),
            );
            source.pause(Duration::from_millis(20).min(remaining));
        }
    })();
    drop(gate);
    // Teardown precedes potentially slow diagnostic sinks; deadlines never depend on stderr readers.
    let cleaned = cleanup(&mut source, &mut tree, &mut child, &path);
    // Redact complete streams, not individual read chunks: credentials may straddle reads.
    for (component, bytes) in [
        ("stdout", &output.stdout_log),
        ("stderr", &output.stderr_log),
    ] {
        if !bytes.is_empty() {
            log.event("INFO", component, &String::from_utf8_lossy(bytes));
        }
    }
    let capture_flushed = output
        .capture
        .as_mut()
        .map(|file| file.flush())
        .transpose()
        .map_err(|_| "Owned capture flush failed.");
    if cleaned.is_ok() {
        fs::remove_file(&path).map_err(|_| "Completed owner record removal failed.")?;
    }
    match fs::remove_file(path.with_extension("json.tmp")) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err("Owner temporary record cleanup failed.".into()),
    }
    cleaned?;
    capture_flushed?;
    let code = match execution {
        Ok(code) => code,
        Err(error)
            if error.contains("output exceeded")
                || error.contains("exceeded its deadline")
                || error.contains("observation budget") =>
        {
            log.event("ERROR", "output", &error);
            124
        }
        Err(error) => return Err(error),
    };
    log.event(
        if code == 0 { "INFO" } else { "ERROR" },
        "shutdown",
        &format!(
            "Owned command complete exit_code={code} output_bytes={}",
            output.bytes
        ),
    );
    Ok(code)
}
