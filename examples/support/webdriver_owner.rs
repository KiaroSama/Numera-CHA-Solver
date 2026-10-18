use std::{
    collections::BTreeMap,
    fs::{self, File},
    io,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::process::CommandExt,
    },
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio::time::{Instant, sleep};

type Check<T> = Result<T, String>;

#[derive(Clone, Debug)]
pub struct Identity {
    pub pid: i32,
    pub birth: u64,
    parent: i32,
    group: i32,
    state: char,
}
impl Identity {
    fn read(pid: i32) -> io::Result<Self> {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
        let fields: Vec<_> = stat
            .rsplit_once(") ")
            .ok_or_else(|| io::Error::other("Invalid process identity."))?
            .1
            .split_whitespace()
            .collect();
        let number = |index: usize| -> io::Result<u64> {
            fields
                .get(index)
                .ok_or_else(|| io::Error::other("Incomplete process identity."))?
                .parse()
                .map_err(|_| io::Error::other("Invalid process identity field."))
        };
        Ok(Self {
            pid,
            birth: number(19)?,
            parent: i32::try_from(number(1)?)
                .map_err(|_| io::Error::other("Invalid parent PID."))?,
            group: i32::try_from(number(2)?).map_err(|_| io::Error::other("Invalid group PID."))?,
            state: fields
                .first()
                .and_then(|value| value.chars().next())
                .ok_or_else(|| io::Error::other("Missing process state."))?,
        })
    }
    pub fn executing(&self) -> Check<bool> {
        match Self::read(self.pid) {
            Ok(current) => Ok(current.birth == self.birth && !matches!(current.state, 'Z' | 'X')),
            Err(error) if vanished(&error) => Ok(false),
            Err(_) => Err("Owned process state could not be verified.".into()),
        }
    }
    pub fn verify_executable(&self, executable: &Path) -> Check<()> {
        let current =
            Self::read(self.pid).map_err(|_| "Owned application identity disappeared.")?;
        let actual = fs::read_link(format!("/proc/{}/exe", self.pid))
            .map_err(|_| "Owned application executable could not be verified.")?;
        if current.birth != self.birth || actual != executable || !self.executing()? {
            return Err("Owned application identity changed before native close.".into());
        }
        Ok(())
    }
    fn signal(&self, signal: i32) -> Check<()> {
        // SAFETY: pidfd_open takes a positive captured PID; OwnedFd closes the resulting descriptor.
        let descriptor = unsafe { libc::syscall(libc::SYS_pidfd_open, self.pid, 0) };
        if descriptor < 0 {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(libc::ESRCH) {
                Ok(())
            } else {
                Err("Owned process pidfd could not be opened.".into())
            };
        }
        // SAFETY: a successful pidfd_open returns a new descriptor owned only by this scope.
        let descriptor = unsafe { OwnedFd::from_raw_fd(descriptor as i32) };
        match Self::read(self.pid) {
            Ok(current) if current.birth == self.birth => {}
            Ok(_) => return Ok(()), // PID reuse never grants ownership of its successor.
            Err(error) if vanished(&error) => return Ok(()),
            Err(_) => return Err("Owned process identity could not be rechecked.".into()),
        }
        // SAFETY: the pidfd pins the rechecked process, so a recycled numeric PID cannot be signalled.
        let status = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                descriptor.as_raw_fd(),
                signal,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        if status != 0 && io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH) {
            return Err("Owned process signal failed.".into());
        }
        Ok(())
    }
}
fn vanished(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::InvalidInput
    ) || error.raw_os_error() == Some(libc::ESRCH)
}

pub struct DriverOwner {
    process: Child,
    members: BTreeMap<i32, Identity>,
    stopped: bool,
}
impl DriverOwner {
    pub async fn spawn(command: &mut Command, capture: &File) -> Check<Self> {
        // SAFETY: this process deliberately adopts/reaps only its own orphaned descendants.
        if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } != 0 {
            return Err("Owned descendant reaping could not be enabled.".into());
        }
        let process = command
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(
                capture
                    .try_clone()
                    .map_err(|_| "Driver capture clone failed.")?,
            )
            .stderr(
                capture
                    .try_clone()
                    .map_err(|_| "Driver capture clone failed.")?,
            )
            .spawn()
            .map_err(|_| "Native driver could not start.")?;
        let mut owner = Self {
            process,
            members: BTreeMap::new(),
            stopped: false,
        };
        let capture_identity = || -> Check<Identity> {
            let pid =
                i32::try_from(owner.process.id()).map_err(|_| "Driver PID exceeded its limit.")?;
            let identity =
                Identity::read(pid).map_err(|_| "Native driver identity could not be captured.")?;
            if identity.group != pid {
                return Err("Native driver did not receive its owned process group.".into());
            }
            Ok(identity)
        };
        let identity = match capture_identity() {
            Ok(identity) => identity,
            Err(error) => {
                let _ = owner.process.kill();
                let deadline = Instant::now() + Duration::from_secs(2);
                loop {
                    if owner
                        .process
                        .try_wait()
                        .map_err(|_| "Unidentified driver could not be reaped.")?
                        .is_some()
                    {
                        return Err(error);
                    }
                    if Instant::now() >= deadline {
                        return Err("Unidentified native driver survived bounded cleanup.".into());
                    }
                    sleep(Duration::from_millis(20)).await;
                }
            }
        };
        owner.members.insert(identity.pid, identity);
        if let Err(error) = owner.snapshot() {
            let cleanup = owner.stop().await;
            return Err(match cleanup {
                Ok(()) => error,
                Err(cleanup) => format!("{error} Cleanup: {cleanup}"),
            });
        }
        Ok(owner)
    }
    pub fn running(&mut self) -> Check<bool> {
        self.snapshot()?;
        self.process
            .try_wait()
            .map(|status| status.is_none())
            .map_err(|_| "Native driver exit state could not be inspected.".into())
    }
    fn snapshot(&mut self) -> Check<()> {
        let mut observed = BTreeMap::new();
        for entry in fs::read_dir("/proc").map_err(|_| "Owned process discovery is unavailable.")? {
            let entry = entry.map_err(|_| "Owned process discovery failed.")?;
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|value| value.parse::<i32>().ok())
            else {
                continue;
            };
            match Identity::read(pid) {
                Ok(identity) => {
                    observed.insert(pid, identity);
                }
                Err(error) if vanished(&error) => {}
                Err(_) => {
                    return Err(
                        "Owned process discovery could not inspect a process identity.".into(),
                    );
                }
            }
        }
        // An existing process group member keeps its group identity alive even if the root has exited.
        let root_group = self.process.id() as i32;
        let group_owned = observed.values().any(|identity| {
            identity.group == root_group
                && self
                    .members
                    .get(&identity.pid)
                    .is_some_and(|known| known.birth == identity.birth)
        });
        for _ in 0..64 {
            let mut added = false;
            for identity in observed.values() {
                if self.members.contains_key(&identity.pid) {
                    continue;
                }
                let parent_owned = observed.get(&identity.parent).is_some_and(|parent| {
                    self.members.get(&parent.pid).is_some_and(|known| {
                        known.birth == parent.birth && parent.birth <= identity.birth
                    })
                });
                let adopted = identity.parent == std::process::id() as i32;
                if parent_owned || adopted || (group_owned && identity.group == root_group) {
                    self.members.insert(identity.pid, identity.clone());
                    added = true;
                }
            }
            if !added {
                return Ok(());
            }
        }
        Err("Owned descendant discovery exceeded its depth bound.".into())
    }
    pub fn application(&mut self, executable: &Path) -> Check<Identity> {
        self.snapshot()?;
        let mut matches = Vec::new();
        for identity in self.members.values() {
            if !identity.executing()? {
                continue;
            }
            match fs::read_link(format!("/proc/{}/exe", identity.pid)) {
                Ok(actual) if actual == executable => matches.push(identity.clone()),
                Ok(_) => {}
                Err(error) if vanished(&error) => {}
                Err(_) => return Err("Owned descendant executable could not be inspected.".into()),
            }
        }
        if matches.len() != 1 {
            return Err("Expected exactly one owned packaged desktop process.".into());
        }
        Ok(matches.remove(0))
    }
    fn reap(&mut self) -> Check<()> {
        self.process
            .try_wait()
            .map_err(|_| "Owned driver could not be reaped.")?;
        for identity in self.members.values() {
            if identity.pid == self.process.id() as i32 {
                continue;
            }
            match Identity::read(identity.pid) {
                Ok(current)
                    if current.birth == identity.birth && matches!(current.state, 'Z' | 'X') => {}
                Ok(_) => continue,
                Err(error) if vanished(&error) => continue,
                Err(_) => {
                    return Err(
                        "Owned descendant identity could not be checked before reaping.".into(),
                    );
                }
            }
            // SAFETY: this rechecked ended child cannot be reused until its status is reaped.
            let status =
                unsafe { libc::waitpid(identity.pid, std::ptr::null_mut(), libc::WNOHANG) };
            if status < 0 && io::Error::last_os_error().raw_os_error() != Some(libc::ECHILD) {
                return Err("Owned descendant could not be reaped.".into());
            }
        }
        Ok(())
    }
    pub async fn stop(&mut self) -> Check<()> {
        let mut failure = self.snapshot().err();
        for identity in self.members.values().rev() {
            if let Err(error) = identity.signal(libc::SIGTERM) {
                failure.get_or_insert(error);
            }
        }
        let graceful = Instant::now() + Duration::from_secs(5);
        let deadline = graceful + Duration::from_secs(5);
        loop {
            if let Err(error) = self.snapshot() {
                failure.get_or_insert(error);
            }
            if Instant::now() >= graceful {
                // Freeze captured parents before the final snapshot so teardown cannot create an unseen child.
                for identity in self.members.values().rev() {
                    if let Err(error) = identity.signal(libc::SIGSTOP) {
                        failure.get_or_insert(error);
                    }
                }
                if let Err(error) = self.snapshot() {
                    failure.get_or_insert(error);
                }
                for identity in self.members.values().rev() {
                    if let Err(error) = identity.signal(libc::SIGKILL) {
                        failure.get_or_insert(error);
                    }
                }
            }
            if let Err(error) = self.reap() {
                failure.get_or_insert(error);
            }
            let mut remaining = false;
            for identity in self.members.values() {
                match Identity::read(identity.pid) {
                    Ok(current) if current.birth == identity.birth => remaining = true,
                    Ok(_) => {}
                    Err(error) if vanished(&error) => {}
                    Err(_) => {
                        remaining = true;
                        failure.get_or_insert("Owned cleanup identity inspection failed.".into());
                    }
                }
            }
            if !remaining {
                self.stopped = true;
                return failure.map_or(Ok(()), Err);
            }
            if Instant::now() >= deadline {
                return Err("Owned native driver descendants survived cleanup.".into());
            }
            sleep(Duration::from_millis(20)).await; // Bounded kernel exit/reaping readiness.
        }
    }
}
impl Drop for DriverOwner {
    fn drop(&mut self) {
        if !self.stopped {
            for identity in self.members.values().rev() {
                let _ = identity.signal(libc::SIGKILL);
            }
            let _ = self.process.kill();
            let _ = self.process.try_wait();
            // The outer hosted owner remains responsible for interruption and escaped descendants.
        }
    }
}
