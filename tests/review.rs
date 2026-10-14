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
