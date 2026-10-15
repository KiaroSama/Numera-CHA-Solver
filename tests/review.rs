use numera_cha_solver::review::ReviewStore;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[test]
fn human_review_is_revision_bound_correctable_and_terminal() {
    let root = tempfile::tempdir().unwrap();
    let store = ReviewStore::new(&root.path().join("reviews.sqlite3"), 60).unwrap();
    let review = store.create("frame-1", "suggested").unwrap();
    let id = review["id"].as_str().unwrap();
    assert_eq!(review["state"], "pending");
    assert_eq!(review["server_verified"], false);
    assert!(store.finish(id, "frame-2", Some("corrected")).is_err());
    assert_eq!(store.get(id).unwrap()["state"], "pending");
    let confirmed = store.finish(id, "frame-1", Some("corrected")).unwrap();
    assert_eq!(confirmed["answer"], "corrected");
    assert_eq!(confirmed["state"], "confirmed");
    assert_eq!(confirmed["server_verified"], false);
    assert!(store.finish(id, "frame-1", None).is_err());
    drop(store);
    let reopened = ReviewStore::new(&root.path().join("reviews.sqlite3"), 60).unwrap();
    assert_eq!(reopened.get(id).unwrap()["state"], "confirmed");
}

#[test]
fn cancelled_expired_and_invalid_reviews_cannot_be_confirmed() {
    let root = tempfile::tempdir().unwrap();
    let clock = Arc::new(AtomicU64::new(100));
    let now = clock.clone();
    let store = ReviewStore::with_clock(
        &root.path().join("reviews.sqlite3"),
        60,
        Arc::new(move || now.load(Ordering::SeqCst) as f64),
    )
    .unwrap();
    for (revision, answer) in [("", "answer"), ("valid", ""), ("valid", "\0")] {
        assert!(store.create(revision, answer).is_err());
    }
    assert!(store.create("valid", &"x".repeat(4097)).is_err());
    let review = store.create("v1", "owned").unwrap();
    let id = review["id"].as_str().unwrap();
    assert_eq!(store.finish(id, "v1", None).unwrap()["state"], "cancelled");
    assert!(store.finish(id, "v1", Some("answer")).is_err());
    let next = store.create("v2", "owned").unwrap();
    clock.store(161, Ordering::SeqCst);
    assert!(store.get(next["id"].as_str().unwrap()).is_err());
    assert!(
        store
            .finish(next["id"].as_str().unwrap(), "v2", Some("answer"))
            .is_err()
    );
}

#[test]
fn review_bounds_count_unicode_scalars_and_reject_control_characters() {
    let root = tempfile::tempdir().unwrap();
    let store = ReviewStore::new(&root.path().join("reviews.sqlite3"), 60).unwrap();
    let revision = "ر".repeat(128);
    let answer = "🦀".repeat(4096);
    let review = store.create(&revision, &answer).unwrap();
    let id = review["id"].as_str().unwrap();
    assert_eq!(review["suggestion"], answer);
    assert_eq!(
        store.create(&"ر".repeat(129), "valid").unwrap_err().code,
        "invalid_review"
    );
    assert_eq!(
        store.create("valid", &"🦀".repeat(4097)).unwrap_err().code,
        "invalid_review"
    );
    for invalid in [" ", "line\nfeed", "tab\t", "nul\0", "del\u{7f}"] {
        assert_eq!(
            store.create("valid", invalid).unwrap_err().code,
            "invalid_review"
        );
        assert_eq!(
            store.create(invalid, "valid").unwrap_err().code,
            "invalid_review"
        );
        assert_eq!(
            store.finish(id, &revision, Some(invalid)).unwrap_err().code,
            "invalid_review"
        );
        assert_eq!(store.get(id).unwrap()["state"], "pending");
    }
    assert_eq!(
        store.finish(id, &revision, Some(&answer)).unwrap()["answer"],
        answer
    );
}

#[test]
fn ttl_endpoints_expire_at_the_exact_boundary() {
    let root = tempfile::tempdir().unwrap();
    for ttl in [0, 59, 86_401, u64::MAX] {
        assert_eq!(
            ReviewStore::new(&root.path().join("invalid.sqlite3"), ttl)
                .err()
                .unwrap()
                .code,
            "invalid_review"
        );
    }
    for ttl in [60, 86_400] {
        let clock = Arc::new(AtomicU64::new(100));
        let now = clock.clone();
        let store = ReviewStore::with_clock(
            &root.path().join(format!("ttl-{ttl}.sqlite3")),
            ttl,
            Arc::new(move || now.load(Ordering::SeqCst) as f64),
        )
        .unwrap();
        let review = store.create("v1", "owned").unwrap();
        let id = review["id"].as_str().unwrap();
        clock.store(100 + ttl - 1, Ordering::SeqCst);
        assert_eq!(store.get(id).unwrap()["state"], "pending");
        clock.store(100 + ttl, Ordering::SeqCst);
        assert_eq!(store.get(id).unwrap_err().code, "review_missing");
        assert_eq!(
            store.finish(id, "v1", Some("answer")).unwrap_err().code,
            "review_conflict"
        );
    }
}

#[test]
fn capacity_retains_terminal_records_until_expiry_then_reclaims_them() {
    let root = tempfile::tempdir().unwrap();
    let clock = Arc::new(AtomicU64::new(100));
    let now = clock.clone();
    let store = ReviewStore::with_clock(
        &root.path().join("reviews.sqlite3"),
        60,
        Arc::new(move || now.load(Ordering::SeqCst) as f64),
    )
    .unwrap();
    let first = store.create("v1", "owned").unwrap();
    let first_id = first["id"].as_str().unwrap();
    store.finish(first_id, "v1", None).unwrap();
    for _ in 1..256 {
        store.create("v1", "owned").unwrap();
    }
    assert_eq!(
        store.create("v1", "overflow").unwrap_err().code,
        "review_busy"
    );
    assert_eq!(store.get(first_id).unwrap()["state"], "cancelled");
    clock.store(160, Ordering::SeqCst);
    assert_eq!(store.create("v2", "successor").unwrap()["state"], "pending");
    assert_eq!(store.get(first_id).unwrap_err().code, "review_missing");
}

#[tokio::test]
async fn independent_connections_cannot_both_commit_a_terminal_decision() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("reviews.sqlite3");
    let store = ReviewStore::new(&path, 60).unwrap();
    let other = ReviewStore::new(&path, 60).unwrap();
    let review = store.create("v1", "owned").unwrap();
    let id = review["id"].as_str().unwrap().to_owned();
    let (start_first, ready_first) = std::sync::mpsc::sync_channel(1);
    let (start_second, ready_second) = std::sync::mpsc::sync_channel(1);
    let first_id = id.clone();
    let mut first = tokio::task::spawn_blocking(move || {
        ready_first
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        store.finish(&first_id, "v1", Some("corrected"))
    });
    let second_id = id.clone();
    let mut second = tokio::task::spawn_blocking(move || {
        ready_second
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        other.finish(&second_id, "v1", None)
    });
    start_first.send(()).unwrap();
    start_second.send(()).unwrap();
    let joined = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(&mut first, &mut second)
    })
    .await;
    let (first, second) = match joined {
        Ok((first, second)) => (first.unwrap(), second.unwrap()),
        Err(_) => {
            let _ = tokio::join!(first, second);
            panic!("Bounded review decisions did not finish within their budget.");
        }
    };
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    let loser = if first.is_err() {
        first.unwrap_err()
    } else {
        second.unwrap_err()
    };
    assert_eq!(loser.code, "review_conflict");
    let reopened = ReviewStore::new(&path, 60).unwrap();
    let result = reopened.get(&id).unwrap();
    assert_eq!(result["server_verified"], false);
    if result["state"] == "confirmed" {
        assert_eq!(result["answer"], "corrected");
    } else {
        assert_eq!(result["state"], "cancelled");
        assert!(result["answer"].is_null());
    }
}
