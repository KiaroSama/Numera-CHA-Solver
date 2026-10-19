//! Exact-identity attribution shared by the hosted adapter and deterministic regressions.
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    time::Duration,
};

pub type Pid = i32;
pub type Result<T> = std::result::Result<T, String>;
pub const OWNER_TAG: &str = "NUMERA_CI_PROCESS_OWNER";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Birth(pub u64, pub u64);
impl Birth {
    pub fn encode(self) -> String {
        format!("{}:{}", self.0, self.1)
    }
    pub fn decode(value: &str) -> Option<Self> {
        let (seconds, fraction) = value.split_once(':')?;
        Some(Self(seconds.parse().ok()?, fraction.parse().ok()?))
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Running,
    Idle,
    Ended,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Identity {
    pub pid: Pid,
    pub birth: Birth,
    pub parent: Pid,
    pub uid: u32,
    pub state: State,
}
impl Identity {
    pub fn same(self, other: Self) -> bool {
        self.pid == other.pid && self.birth == other.birth
    }
}

pub trait Source {
    fn list(&mut self) -> io::Result<Vec<Pid>>;
    fn identity(&mut self, pid: Pid) -> io::Result<Option<Identity>>;
    fn tagged(&mut self, pid: Pid, tag: &str) -> io::Result<bool>;
    fn now(&self) -> Duration;
    fn pause(&mut self, duration: Duration);
    fn darwin(&self) -> bool;
    fn subreaper(&self) -> bool;
}

pub struct Tree {
    pub owner: Identity,
    pub root: Identity,
    pub members: BTreeMap<Pid, Identity>,
    pub tag: String,
    pub deadline: Duration,
}
impl Tree {
    pub fn new(owner: Identity, root: Identity, tag: String, deadline: Duration) -> Result<Self> {
        let mut tree = Self {
            owner,
            root,
            members: BTreeMap::new(),
            tag,
            deadline,
        };
        tree.remember(root)?;
        Ok(tree)
    }
    /// Only for the direct Child handle still blocked on this owner's private launch pipe.
    /// The inherited tag is discovery evidence for unknown peers, not a prerequisite for this child.
    pub fn check_gated_root<S: Source>(&self, source: &mut S) -> Result<()> {
        let current =
            inspect(source, self.root.pid)?.ok_or("Root identity disappeared before exec.")?;
        if !self.root.same(current)
            || current.parent != self.owner.pid
            || current.uid != self.owner.uid
        {
            return Err("Root identity changed before exec gate release.".into());
        }
        if matches!(current.state, State::Ended | State::Unknown) {
            return Err("Root launch identity is not executing or initializing.".into());
        }
        Ok(())
    }
    pub fn remember(&mut self, identity: Identity) -> Result<()> {
        if identity.pid <= 0 || identity.pid == self.owner.pid {
            return Err("Process owner cannot own itself or an invalid PID.".into());
        }
        if self
            .members
            .get(&identity.pid)
            .is_some_and(|old| !old.same(identity))
        {
            return Err("Tracked process PID was reused before cleanup.".into());
        }
        self.members.insert(identity.pid, identity);
        Ok(())
    }
    pub fn attribute<S: Source>(&mut self, source: &mut S, candidate: Identity) -> Result<bool> {
        let mut chain = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = candidate;
        for _ in 0..64 {
            if source.now() >= self.deadline {
                return Err("Owned ancestor discovery exceeded its deadline.".into());
            }
            if !seen.insert(current.pid) {
                return Ok(false);
            }
            let known = self
                .members
                .get(&current.pid)
                .is_some_and(|old| old.same(current));
            let adopted = source.subreaper() && self.owner.same(current);
            if known || adopted {
                let Some(fresh) = inspect(source, candidate.pid)? else {
                    return Ok(false);
                };
                if !candidate.same(fresh) {
                    return Ok(false);
                }
                for child in chain {
                    self.remember(child)?;
                }
                return Ok(true);
            }
            chain.push(current);
            // Parent disappearance or restricted metadata is not proof that the candidate ended;
            // its own tag must still be inspected (a detached process can outlive that ancestor).
            if current.parent <= 0 {
                return Ok(false);
            }
            let Some(parent) = source.identity(current.parent).ok().flatten() else {
                return Ok(false);
            };
            // A reused parent cannot confer ownership, even if its PID still matches.
            if parent.birth > current.birth {
                return Ok(false);
            }
            current = parent;
        }
        Err("Owned ancestor discovery exceeded its depth bound.".into())
    }
    pub fn settle<S: Source>(&self, source: &mut S, candidate: Identity) -> Result<bool> {
        if !source.darwin() || candidate.state != State::Idle {
            return Ok(false);
        }
        let deadline = self
            .deadline
            .min(source.now().saturating_add(Duration::from_secs(1)));
        loop {
            let Some(current) = inspect(source, candidate.pid)? else {
                return Ok(false);
            };
            if !candidate.same(current) {
                return Err("Darwin newborn identity changed during observation.".into());
            }
            if current.state == State::Unknown {
                return Err("Darwin newborn status unavailable during observation.".into());
            }
            if current.state != State::Idle {
                return Ok(true);
            }
            let remaining = deadline.saturating_sub(source.now());
            if remaining.is_zero() {
                return Err("Darwin process creation exceeded its observation budget.".into());
            }
            source.pause(remaining.min(Duration::from_millis(10)));
        }
    }
    pub fn snapshot<S: Source>(&mut self, source: &mut S) -> Result<()> {
        for pid in source
            .list()
            .map_err(|_| "Process discovery enumeration failed.")?
        {
            if source.now() >= self.deadline {
                return Err("Owned process discovery exceeded its deadline.".into());
            }
            let Some(candidate) = inspect(source, pid)? else {
                continue;
            };
            if pid == self.owner.pid {
                continue;
            }
            if let Some(known) = self.members.get(&pid) {
                if !known.same(candidate) {
                    return Err("Tracked process PID was reused before cleanup.".into());
                }
                if candidate.state == State::Unknown {
                    return Err(
                        "Owned process status unavailable; identity-checked cleanup required."
                            .into(),
                    );
                }
                continue; // An owned identity never needs another restricted environment read.
            }
            if candidate.uid != self.owner.uid || candidate.birth < self.owner.birth {
                continue;
            }
            if candidate.state == State::Unknown {
                return Err(
                    "Discovery process status unavailable; refusing an unverified run.".into(),
                );
            }
            // Attribute ended descendants too: Linux subreaper must reap adopted zombies.
            if self.attribute(source, candidate)? || candidate.state == State::Ended {
                continue;
            }
            match source.tagged(pid, &self.tag) {
                Ok(tagged) => {
                    if tagged {
                        self.remember_if_same(source, candidate)?;
                    }
                }
                Err(error) if vanished(&error) => continue,
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                    // Re-read state and ancestry: exit, late birth and subreaper adoption race procargs.
                    let Some(current) = inspect(source, pid)? else {
                        continue;
                    };
                    if !candidate.same(current) {
                        return Err("Discovery candidate PID was reused.".into());
                    }
                    let settled = self.settle(source, current)?;
                    let Some(current) = inspect(source, pid)? else {
                        continue;
                    };
                    if !candidate.same(current) {
                        return Err("Discovery candidate PID was reused.".into());
                    }
                    if current.state == State::Unknown {
                        return Err(
                            "Discovery process status unavailable after denied environment read."
                                .into(),
                        );
                    }
                    if self.attribute(source, current)? || current.state == State::Ended {
                        continue;
                    }
                    if settled {
                        match source.tagged(pid, &self.tag) {
                            Ok(true) => {
                                self.remember_if_same(source, candidate)?;
                                continue;
                            }
                            Ok(false) => continue,
                            Err(error) if vanished(&error) => continue,
                            Err(_) => {}
                        }
                    }
                    return Err(format!(
                        "Owned discovery cannot inspect new live same-user process pid={pid}; refusing an unverified run."
                    ));
                }
                Err(_) => return Err("Owned environment discovery failed.".into()),
            }
        }
        Ok(())
    }
    fn remember_if_same<S: Source>(&mut self, source: &mut S, candidate: Identity) -> Result<()> {
        if let Some(current) = inspect(source, candidate.pid)? {
            if !candidate.same(current) {
                return Err("Tagged process PID was reused during discovery.".into());
            }
            self.remember(current)?;
        }
        Ok(())
    }
    pub fn live<S: Source>(&self, source: &mut S, executing: bool) -> Result<Vec<Identity>> {
        let mut live = Vec::new();
        for known in self.members.values() {
            if let Some(current) = inspect(source, known.pid)? {
                if !known.same(current) {
                    return Err("Tracked process PID was reused before inspection.".into());
                }
                if !executing || current.state != State::Ended {
                    live.push(current);
                }
            }
        }
        Ok(live)
    }
}
pub fn signal_allowed(known: Identity, current: Option<Identity>) -> Result<bool> {
    if known.pid <= 0 {
        return Err("Refusing to signal an invalid PID.".into());
    }
    let Some(current) = current else {
        return Ok(false);
    };
    if !known.same(current) {
        return Err("Refusing to signal a reused PID.".into());
    }
    Ok(current.state != State::Ended)
}
pub fn vanished(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound || error.raw_os_error() == Some(3)
}
fn inspect<S: Source>(source: &mut S, pid: Pid) -> Result<Option<Identity>> {
    if pid <= 0 {
        return Ok(None);
    }
    source
        .identity(pid)
        .map_err(|error| format!("Process identity inspection unavailable pid={pid}: {error}"))
}
