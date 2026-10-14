use crate::core::contracts::Error;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

type Clock = Arc<dyn Fn() -> f64 + Send + Sync>;
#[derive(Clone)]
pub struct ReviewStore {
    connection: Arc<Mutex<Connection>>,
    ttl: u64,
    clock: Clock,
}
fn failure() -> Error {
    Error::new("review_store_failed", "Local review storage failed.")
}
fn invalid() -> Error {
    Error::new(
        "invalid_review",
        "Review requires a revision and a nonempty bounded answer.",
    )
}
fn missing() -> Error {
    Error::new("review_missing", "Review is missing or expired.")
}
fn conflict() -> Error {
    Error::new(
        "review_conflict",
        "Review revision changed or the review is already terminal.",
    )
}
fn valid(value: &str, max: usize) -> bool {
    !value.trim().is_empty()
        && value.chars().count() <= max
        && !value.chars().any(|c| c.is_control())
}
impl ReviewStore {
    pub fn new(path: &Path, ttl: u64) -> Result<Self, Error> {
        Self::with_clock(path, ttl, Arc::new(crate::store::now))
    }
    pub fn with_clock(path: &Path, ttl: u64, clock: Clock) -> Result<Self, Error> {
        if !(60..=86_400).contains(&ttl) {
            return Err(invalid());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| failure())?;
        }
        if std::fs::symlink_metadata(path).is_ok_and(|m| crate::assets::is_link(&m) || !m.is_file())
        {
            return Err(failure());
        }
        let connection = Connection::open(path).map_err(|_| failure())?;
        connection
            .busy_timeout(std::time::Duration::from_secs(3))
            .map_err(|_| failure())?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS reviews (
            id TEXT PRIMARY KEY, revision TEXT NOT NULL, suggestion TEXT NOT NULL,
            answer TEXT, state TEXT NOT NULL, expires REAL NOT NULL);",
            )
            .map_err(|_| failure())?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            ttl,
            clock,
        })
    }
    pub fn create(&self, revision: &str, suggestion: &str) -> Result<Value, Error> {
        if !valid(revision, 128) || !valid(suggestion, 4096) {
            return Err(invalid());
        }
        let id: String = rand::random::<[u8; 16]>()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let expires = (self.clock)() + self.ttl as f64;
        let connection = self.connection.lock().map_err(|_| failure())?;
        connection
            .execute("DELETE FROM reviews WHERE expires<=?", [(self.clock)()])
            .map_err(|_| failure())?;
        let count: i64 = connection
            .query_row("SELECT count(*) FROM reviews", [], |r| r.get(0))
            .map_err(|_| failure())?;
        if count >= 256 {
            return Err(Error::new(
                "review_busy",
                "Review capacity reached; finish or expire existing reviews.",
            ));
        }
        connection
            .execute(
                "INSERT INTO reviews VALUES (?,?,?,NULL,'pending',?)",
                params![id, revision, suggestion, expires],
            )
            .map_err(|_| failure())?;
        read(&connection, &id, (self.clock)())
    }
    pub fn get(&self, id: &str) -> Result<Value, Error> {
        if !valid(id, 64) {
            return Err(missing());
        }
        let connection = self.connection.lock().map_err(|_| failure())?;
        read(&connection, id, (self.clock)())
    }
    pub fn finish(&self, id: &str, revision: &str, answer: Option<&str>) -> Result<Value, Error> {
        if !valid(id, 64) || !valid(revision, 128) || answer.is_some_and(|v| !valid(v, 4096)) {
            return Err(invalid());
        }
        let connection = self.connection.lock().map_err(|_| failure())?;
        let changed = connection.execute("UPDATE reviews SET state=?,answer=? WHERE id=? AND revision=? AND state='pending' AND expires>?",
            params![if answer.is_some(){"confirmed"}else{"cancelled"},answer,id,revision,(self.clock)()]).map_err(|_| failure())?;
        if changed != 1 {
            return Err(conflict());
        }
        read(&connection, id, (self.clock)())
    }
}
fn read(connection: &Connection, id: &str, now: f64) -> Result<Value, Error> {
    connection.query_row("SELECT * FROM reviews WHERE id=? AND expires>?", params![id,now], |row| {
        Ok(json!({"id":row.get::<_,String>("id")?,"revision":row.get::<_,String>("revision")?,
            "suggestion":row.get::<_,String>("suggestion")?,"answer":row.get::<_,Option<String>>("answer")?,
            "state":row.get::<_,String>("state")?,"expires":row.get::<_,f64>("expires")?,
            "server_verified":false,"note":"Manual review is not third-party challenge acceptance."}))
    }).optional().map_err(|_| failure())?.ok_or_else(missing)
}
