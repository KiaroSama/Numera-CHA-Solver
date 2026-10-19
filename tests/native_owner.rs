//! Pure discovery-policy contracts; real subprocess modes run once via owned-command --contract.
#[path = "../desktop/ci/owner/log.rs"]
#[allow(dead_code)]
mod log;
#[path = "../desktop/ci/owner/metadata.rs"]
#[allow(dead_code)]
mod metadata;
#[path = "../desktop/ci/owner/policy.rs"]
#[allow(dead_code)]
mod policy;
use policy::{Birth, Identity, Source, State, Tree};
use std::{
    cell::Cell,
    collections::{BTreeMap, VecDeque},
    io,
    time::Duration,
};

#[derive(Clone, Copy)]
enum Tag {
    Yes,
    No,
    Denied,
    Gone,
}
struct Fake {
    identities: BTreeMap<i32, Identity>,
    reads: BTreeMap<i32, VecDeque<Option<Identity>>>,
    tags: VecDeque<Tag>,
    tag_reads: usize,
    clock: Cell<Duration>,
    pauses: Vec<Duration>,
    darwin: bool,
    subreaper: bool,
    denied_identity: Option<i32>,
}
impl Source for Fake {
    fn list(&mut self) -> io::Result<Vec<i32>> {
        Ok(self.identities.keys().copied().collect())
    }
    fn identity(&mut self, pid: i32) -> io::Result<Option<Identity>> {
        if self.denied_identity == Some(pid) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        if let Some(value) = self.reads.get_mut(&pid).and_then(VecDeque::pop_front) {
            return Ok(value);
        }
        Ok(self.identities.get(&pid).copied())
    }
    fn tagged(&mut self, _: i32, _: &str) -> io::Result<bool> {
        self.tag_reads += 1;
        match self.tags.pop_front().unwrap_or(Tag::Denied) {
            Tag::Yes => Ok(true),
            Tag::No => Ok(false),
            Tag::Denied => Err(io::Error::from(io::ErrorKind::PermissionDenied)),
            Tag::Gone => Err(io::Error::from(io::ErrorKind::NotFound)),
        }
    }
    fn now(&self) -> Duration {
        self.clock.get()
    }
    fn pause(&mut self, duration: Duration) {
        self.pauses.push(duration);
        self.clock.set(self.clock.get() + duration);
    }
    fn darwin(&self) -> bool {
        self.darwin
    }
    fn subreaper(&self) -> bool {
        self.subreaper
    }
}
fn identity(pid: i32, parent: i32, birth: u64, state: State) -> Identity {
    Identity {
        pid,
        parent,
        birth: Birth(birth, 123456),
        uid: 501,
        state,
    }
}
fn setup() -> (Tree, Fake) {
    let owner = identity(10, 1, 99, State::Running);
    let root = identity(20, 10, 100, State::Running);
    (
        Tree::new(
            owner,
            root,
            "owned-discovery-fixture".into(),
            Duration::from_secs(3),
        )
        .unwrap(),
        Fake {
            identities: BTreeMap::from([(20, root)]),
            reads: BTreeMap::new(),
            tags: VecDeque::new(),
            tag_reads: 0,
            clock: Cell::new(Duration::ZERO),
            pauses: Vec::new(),
            darwin: false,
            subreaper: false,
            denied_identity: None,
        },
    )
}
#[test]
fn darwin_libproc_zero_is_not_disappearance_without_explicit_esrch() {
    assert!(metadata::proc_info_result(949, 17, 56, 56, 0).unwrap());
    assert!(!metadata::proc_info_result(949, 17, 0, 56, 3).unwrap());
    for (returned, errno) in [(0, 0), (0, 1), (0, 13), (32, 0)] {
        let error = metadata::proc_info_result(949, 17, returned, 56, errno).unwrap_err();
        let diagnostic = error.to_string();
        assert!(diagnostic.contains("pid=949"));
        assert!(diagnostic.contains(&format!("returned={returned}")));
        assert!(diagnostic.contains(&format!("errno={errno}")));
        assert!(diagnostic.contains("expected=56"));
        assert!(diagnostic.contains("flavor=17"));
    }
}
#[test]
fn exact_os_metadata_and_complete_stream_redaction_preserve_boundaries() {
    // Fields3..22: state, ppid,17 intervening fields, exact start ticks.
    let stat = concat!(
        "42 (odd ) executable) S 10 ",
        "0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 ",
        "9876543210 0\n"
    )
    .as_bytes();
    let parsed = metadata::linux_stat(42, stat, 501).unwrap();
    assert_eq!(parsed.birth, Birth(9_876_543_210, 0));
    assert_eq!(parsed.parent, 10);
    assert_eq!(parsed.uid, 501);
    assert!(metadata::linux_stat(43, stat, 501).is_err());
    assert!(metadata::linux_stat(42, b"42 malformed", 501).is_err());
    let mut args = 2_i32.to_ne_bytes().to_vec();
    args.extend_from_slice(
        b"/bin/fixture\0\0\0fixture\0argument\0NUMERA_CI_PROCESS_OWNER=exact\0OTHER=x\0",
    );
    let environment = metadata::procargs_environment(&args).unwrap();
    assert!(metadata::tagged_bytes(environment, "exact"));
    assert!(!metadata::tagged_bytes(environment, "exac"));
    assert!(!metadata::tagged_bytes(
        b"OTHER=NUMERA_CI_PROCESS_OWNER=exact\0",
        "exact"
    ));
    assert!(metadata::procargs_environment(&[0, 0, 0]).is_err());
    let text = log::redact(
        "Bearer synthetic-value token=synthetic-value password=\"two synthetic words\" api_key=example https://example.invalid/path",
    );
    assert!(!text.contains("synthetic"));
    assert!(!text.contains("example"));
    assert!(text.contains("[REDACTED]"));
    assert_eq!(log::redact("Owned UTF-8 فارسی"), "Owned UTF-8 فارسی");
}
#[test]
fn direct_gated_darwin_newborn_needs_exact_identity_not_procargs_permission() {
    let (mut tree, mut source) = setup();
    source.darwin = true;
    source.identities.get_mut(&20).unwrap().state = State::Idle;
    tree.check_gated_root(&mut source).unwrap();
    tree.snapshot(&mut source).unwrap();
    assert_eq!(source.tag_reads, 0);
    source
        .identities
        .insert(30, identity(30, 1, 102, State::Running));
    assert!(
        tree.snapshot(&mut source)
            .unwrap_err()
            .contains("new live same-user")
    );
    assert!(!tree.members.contains_key(&30));
    source.identities.get_mut(&20).unwrap().birth.1 += 1;
    assert!(tree.check_gated_root(&mut source).is_err());
}
#[test]
fn known_and_ended_identities_never_need_environment_permission() {
    let (mut tree, mut source) = setup();
    source
        .identities
        .insert(30, identity(30, 1, 101, State::Ended));
    tree.snapshot(&mut source).unwrap();
    assert_eq!(source.tag_reads, 0);
    assert_eq!(tree.members.len(), 1);
    source.identities.get_mut(&20).unwrap().state = State::Ended;
    assert!(tree.live(&mut source, true).unwrap().is_empty());
    assert_eq!(tree.live(&mut source, false).unwrap().len(), 1);
}
#[test]
fn live_unknown_denied_fails_closed_but_exit_race_and_vanished_reads_do_not() {
    let candidate = identity(30, 1, 101, State::Running);
    let (mut tree, mut source) = setup();
    source.identities.insert(30, candidate);
    assert!(
        tree.snapshot(&mut source)
            .unwrap_err()
            .contains("new live same-user")
    );
    assert!(!tree.members.contains_key(&30));
    source.reads.insert(
        30,
        VecDeque::from([
            Some(candidate),
            Some(candidate),
            Some(Identity {
                state: State::Ended,
                ..candidate
            }),
            Some(Identity {
                state: State::Ended,
                ..candidate
            }),
        ]),
    );
    tree.snapshot(&mut source).unwrap();
    assert!(!tree.members.contains_key(&30));
    source.reads.remove(&30);
    let reads = source.tag_reads;
    source.tags.push_back(Tag::Gone);
    tree.snapshot(&mut source).unwrap();
    assert_eq!(source.tag_reads, reads + 1);
}
#[test]
fn late_birth_and_denied_read_ancestry_refresh_still_recover_owned_children() {
    let (mut tree, mut source) = setup();
    let candidate = identity(30, 20, 102, State::Running);
    source.identities.insert(30, candidate);
    tree.snapshot(&mut source).unwrap();
    assert_eq!(source.tag_reads, 0);
    assert_eq!(tree.members.get(&30).unwrap().birth, candidate.birth);
    tree.members.remove(&30);
    let unrelated = Identity {
        parent: 1,
        ..candidate
    };
    source.reads.insert(
        30,
        VecDeque::from([Some(unrelated), Some(candidate), Some(candidate)]),
    );
    tree.snapshot(&mut source).unwrap();
    assert_eq!(source.tag_reads, 1);
    assert!(tree.members.contains_key(&30));
}
#[test]
fn reused_parent_never_confers_ownership_and_vanished_ancestor_does_not_hide_tag() {
    let (mut tree, mut source) = setup();
    let candidate = identity(30, 20, 104, State::Running);
    source.identities.insert(30, candidate);
    source.identities.get_mut(&20).unwrap().birth = Birth(103, 123456);
    assert!(!tree.attribute(&mut source, candidate).unwrap());
    assert!(!tree.members.contains_key(&30));
    source.identities.remove(&20);
    source.tags.push_back(Tag::Yes);
    tree.snapshot(&mut source).unwrap();
    assert!(tree.members.contains_key(&30));
    tree.members.remove(&30);
    source.denied_identity = Some(1); // Darwin detached ancestors may belong to root launchd.
    source.identities.get_mut(&30).unwrap().parent = 1;
    source.tags.push_back(Tag::Yes);
    tree.snapshot(&mut source).unwrap();
    assert!(tree.members.contains_key(&30));
}
#[test]
fn linux_subreaper_adoption_and_exact_birth_signal_policy_do_not_use_groups() {
    let (mut tree, mut source) = setup();
    source.subreaper = true;
    source.identities.insert(10, tree.owner);
    source
        .identities
        .insert(30, identity(30, 10, 102, State::Running));
    tree.snapshot(&mut source).unwrap();
    assert!(tree.members.contains_key(&30));
    assert_eq!(source.tag_reads, 0);
    source.identities.get_mut(&30).unwrap().birth.1 += 1;
    assert!(tree.live(&mut source, true).unwrap_err().contains("reused"));
    assert!(tree.snapshot(&mut source).unwrap_err().contains("reused"));
    let known = tree.members[&30];
    assert!(policy::signal_allowed(known, Some(source.identities[&30])).is_err());
    assert!(!policy::signal_allowed(known, None).unwrap());
    assert!(
        !policy::signal_allowed(
            known,
            Some(Identity {
                state: State::Ended,
                ..known
            })
        )
        .unwrap()
    );
    assert!(
        policy::signal_allowed(
            known,
            Some(Identity {
                state: State::Unknown,
                ..known
            })
        )
        .unwrap()
    );
}
#[test]
fn unavailable_owned_status_remains_executing_and_cleanup_remains_failed() {
    let (mut tree, mut source) = setup();
    source.identities.get_mut(&20).unwrap().state = State::Unknown;
    assert_eq!(tree.live(&mut source, true).unwrap().len(), 1);
    assert!(
        tree.snapshot(&mut source)
            .unwrap_err()
            .contains("status unavailable")
    );
    assert_eq!(source.tag_reads, 0);
    assert!(tree.members.get(&20).unwrap().same(source.identities[&20]));
}
#[test]
fn darwin_initialization_rechecks_tag_and_never_waives_denial() {
    for (tag, owned, fails) in [
        (Tag::Yes, true, false),
        (Tag::No, false, false),
        (Tag::Denied, false, true),
    ] {
        let (mut tree, mut source) = setup();
        source.darwin = true;
        let running = identity(30, 1, 102, State::Running);
        let idle = Identity {
            state: State::Idle,
            ..running
        };
        source.identities.insert(30, running);
        source.reads.insert(
            30,
            VecDeque::from([Some(running), Some(idle), Some(idle), Some(running)]),
        );
        source.tags.extend([Tag::Denied, tag]);
        assert_eq!(tree.snapshot(&mut source).is_err(), fails);
        assert_eq!(tree.members.contains_key(&30), owned);
        assert_eq!(source.pauses, [Duration::from_millis(10)]);
        assert_eq!(source.tag_reads, 2);
    }
}
#[test]
fn darwin_observation_uses_monotonic_budget_and_exact_microsecond_identity() {
    let (mut tree, mut source) = setup();
    source.darwin = true;
    let idle = identity(30, 1, 102, State::Idle);
    source.identities.insert(30, idle);
    tree.deadline = Duration::from_millis(5);
    assert!(
        tree.settle(&mut source, idle)
            .unwrap_err()
            .contains("observation budget")
    );
    assert_eq!(source.pauses, [Duration::from_millis(5)]);
    source.clock.set(Duration::ZERO);
    source.pauses.clear();
    tree.deadline = Duration::from_secs(3);
    assert!(tree.settle(&mut source, idle).is_err());
    assert_eq!(source.clock.get(), Duration::from_secs(1));
    source.identities.get_mut(&30).unwrap().birth.1 += 1;
    assert!(
        tree.settle(&mut source, idle)
            .unwrap_err()
            .contains("identity changed")
    );
    let running = Identity {
        state: State::Running,
        ..idle
    };
    let before = source.pauses.len();
    assert!(!tree.settle(&mut source, running).unwrap());
    assert_eq!(source.pauses.len(), before);
    assert_ne!(Birth(1_700_000_000, 123456), Birth(1_700_000_000, 123457));
    assert_eq!(
        Birth::decode("1700000000:123456"),
        Some(Birth(1_700_000_000, 123456))
    );
    assert!(Birth::decode("1700000000.123456").is_none());
}
