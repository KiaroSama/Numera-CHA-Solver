use crate::core::contracts::Error;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
fn database_error() -> Error {
    Error::new("store_failed", "Local task storage operation failed.")
}
fn missing() -> Error {
    Error::new("task_missing", "Task is missing or expired.")
}
fn expired() -> Value {
    json!({"outcome":"expired","error":{"code":"expired","message":"Task deadline exhausted."}})
}
fn identifier() -> String {
    let bytes: [u8; 16] = rand::random();
    let mut text: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    text.replace_range(12..13, "4");
    text.replace_range(16..17, "8");
    format!(
        "{}-{}-{}-{}-{}",
        &text[..8],
        &text[8..12],
        &text[12..16],
        &text[16..20],
        &text[20..]
    )
}
fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let result: Option<String> = row.get("result")?;
    let events: String = row.get("events")?;
    Ok(
        json!({"id":row.get::<_,String>("id")?,"created":row.get::<_,f64>("created")?,
        "deadline":row.get::<_,f64>("deadline")?,"expires":row.get::<_,f64>("expires")?,
        "state":row.get::<_,String>("state")?,"outcome":row.get::<_,Option<String>>("outcome")?,
        "result":result.filter(|s|!s.is_empty()).map(|s|serde_json::from_str::<Value>(&s).map_err(|_|rusqlite::Error::InvalidQuery)).transpose()?,
        "events":serde_json::from_str::<Value>(&events).map_err(|_| rusqlite::Error::InvalidQuery)?}),
    )
}
fn get_locked(connection: &Connection, id: &str, timestamp: f64) -> Result<Value, Error> {
    let mut value = connection
        .query_row("SELECT * FROM tasks WHERE id=?", [id], read_row)
        .optional()
        .map_err(|_| database_error())?
        .ok_or_else(missing)?;
    if value["expires"].as_f64().unwrap_or(0.0) <= timestamp {
        return Err(missing());
    }
    if value["state"] != "terminal" && value["deadline"].as_f64().unwrap_or(0.0) <= timestamp {
        finish_locked(connection, id, expired(), timestamp)?;
        value = connection
            .query_row("SELECT * FROM tasks WHERE id=?", [id], read_row)
            .map_err(|_| database_error())?;
    }
    Ok(value)
}
fn finish_locked(
    connection: &Connection,
    id: &str,
    mut result: Value,
    timestamp: f64,
) -> Result<bool, Error> {
    if !matches!(
        result["outcome"].as_str(),
        Some(
            "recognized"
                | "challenge_completed"
                | "server_verified"
                | "needs_user_action"
                | "unsupported"
                | "failed"
                | "cancelled"
                | "expired"
        )
    ) {
        return Err(Error::new("invalid_task", "Unknown task outcome."));
    }
    let deadline: Option<f64> = connection
        .query_row("SELECT deadline FROM tasks WHERE id=?", [id], |row| {
            row.get(0)
        })
        .optional()
        .map_err(|_| database_error())?;
    if deadline.is_some_and(|d| d <= timestamp)
        && result["outcome"] != "cancelled"
        && result["outcome"] != "expired"
    {
        result = expired();
    }
    let encoded = serde_json::to_string(&result).map_err(|_| database_error())?;
    Ok(connection
        .execute(
            "UPDATE tasks SET state='terminal',outcome=?,result=? WHERE id=? AND state!='terminal'",
            params![result["outcome"].as_str(), encoded, id],
        )
        .map_err(|_| database_error())?
        == 1)
}

pub struct TaskStore {
    connection: Mutex<Connection>,
    ttl: u64,
    clock: Arc<dyn Fn() -> f64 + Send + Sync>,
}
impl TaskStore {
    pub fn new(path: &Path, ttl: u64) -> Result<Self, Error> {
        Self::with_clock(path, ttl, Arc::new(now))
    }
    /// Supplies the persisted wall clock; the default constructor uses the real UTC clock.
    pub fn with_clock(
        path: &Path,
        ttl: u64,
        clock: Arc<dyn Fn() -> f64 + Send + Sync>,
    ) -> Result<Self, Error> {
        std::fs::create_dir_all(path.parent().ok_or_else(database_error)?)
            .map_err(|_| database_error())?;
        let connection = Connection::open(path).map_err(|_| database_error())?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|_| database_error())?;
        connection.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS tasks (id TEXT PRIMARY KEY,key TEXT UNIQUE NOT NULL,digest TEXT NOT NULL,created REAL NOT NULL,deadline REAL NOT NULL,expires REAL NOT NULL,state TEXT NOT NULL,outcome TEXT,result TEXT,events TEXT NOT NULL);").map_err(|_|database_error())?;
        connection.execute("UPDATE tasks SET state='terminal',outcome='failed',result=? WHERE state!='terminal'",[json!({"outcome":"failed","error":{"code":"interrupted","message":"Application restarted; unfinished actions were not replayed."}}).to_string()]).map_err(|_|database_error())?;
        Ok(Self {
            connection: Mutex::new(connection),
            ttl,
            clock,
        })
    }
    pub fn create(&self, key: &str, digest: &str, budget: f64) -> Result<Value, Error> {
        if key.is_empty()
            || key.chars().count() > 128
            || !budget.is_finite()
            || !(1.0..=120.0).contains(&budget)
        {
            return Err(Error::new("invalid_task", "Invalid task key or budget."));
        }
        let mut connection = self.connection.lock().map_err(|_| database_error())?;
        let transaction = connection.transaction().map_err(|_| database_error())?;
        let now = (self.clock)();
        if !now.is_finite() {
            return Err(database_error());
        }
        transaction
            .execute(
                "DELETE FROM tasks WHERE expires<=? AND state='terminal'",
                [now],
            )
            .map_err(|_| database_error())?;
        let existing: Option<(String, String)> = transaction
            .query_row("SELECT id,digest FROM tasks WHERE key=?", [key], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()
            .map_err(|_| database_error())?;
        let id = if let Some((id, existing_digest)) = existing {
            if existing_digest != digest {
                return Err(Error::new(
                    "idempotency_conflict",
                    "Idempotency key was used for a different request.",
                ));
            }
            let result = transaction
                .query_row("SELECT * FROM tasks WHERE id=?", [&id], read_row)
                .map_err(|_| database_error())?;
            transaction.commit().map_err(|_| database_error())?;
            return Ok(result);
        } else {
            let id = identifier();
            transaction
                .execute(
                    "INSERT INTO tasks VALUES(?,?,?,?,?,?,?,NULL,NULL,?)",
                    params![
                        id,
                        key,
                        digest,
                        now,
                        now + budget,
                        now + self.ttl as f64,
                        "queued",
                        "[{\"state\": \"queued\"}]"
                    ],
                )
                .map_err(|_| database_error())?;
            id
        };
        let result = get_locked(&transaction, &id, now)?;
        transaction.commit().map_err(|_| database_error())?;
        Ok(result)
    }
    pub fn get(&self, id: &str) -> Result<Value, Error> {
        get_locked(
            &*self.connection.lock().map_err(|_| database_error())?,
            id,
            (self.clock)(),
        )
    }
    pub fn list(&self, limit: usize) -> Result<Vec<Value>, Error> {
        let connection = self.connection.lock().map_err(|_| database_error())?;
        let mut statement = connection
            .prepare("SELECT id FROM tasks WHERE expires>? ORDER BY created DESC LIMIT ?")
            .map_err(|_| database_error())?;
        let ids = statement
            .query_map(params![(self.clock)(), limit.clamp(1, 100) as i64], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|_| database_error())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| database_error())?;
        ids.iter()
            .map(|id| get_locked(&connection, id, (self.clock)()))
            .collect()
    }
    pub fn transition(&self, id: &str, state: &str, reason: Option<&str>) -> Result<bool, Error> {
        let order = [
            "queued",
            "validating",
            "detecting",
            "recognizing",
            "acting",
            "verifying",
        ];
        let target = order
            .iter()
            .position(|s| *s == state)
            .ok_or_else(|| Error::new("invalid_task", "Unknown nonterminal state."))?;
        let connection = self.connection.lock().map_err(|_| database_error())?;
        let value = connection
            .query_row("SELECT * FROM tasks WHERE id=?", [id], read_row)
            .optional()
            .map_err(|_| database_error())?;
        let Some(value) = value else { return Ok(false) };
        if value["state"] == "terminal"
            || value["deadline"].as_f64().unwrap_or(0.0) <= (self.clock)()
        {
            return Ok(false);
        }
        let current = value["state"].as_str().unwrap_or("");
        if target
            <= order
                .iter()
                .position(|s| *s == current)
                .ok_or_else(database_error)?
        {
            return Err(Error::new(
                "invalid_task",
                "Task state cannot move backward or repeat.",
            ));
        }
        let mut events = value["events"]
            .as_array()
            .cloned()
            .ok_or_else(database_error)?;
        let mut event = json!({"state":state});
        if let Some(reason) = reason.filter(|s| !s.is_empty()) {
            event["reason"] = json!(reason)
        }
        events.push(event);
        Ok(connection
            .execute(
                "UPDATE tasks SET state=?,events=? WHERE id=? AND state=?",
                params![
                    state,
                    serde_json::to_string(&events).map_err(|_| database_error())?,
                    id,
                    current
                ],
            )
            .map_err(|_| database_error())?
            == 1)
    }
    pub fn finish(&self, id: &str, result: Value) -> Result<bool, Error> {
        finish_locked(
            &*self.connection.lock().map_err(|_| database_error())?,
            id,
            result,
            (self.clock)(),
        )
    }
    pub fn cancel(&self, id: &str) -> Result<Value, Error> {
        let connection = self.connection.lock().map_err(|_| database_error())?;
        get_locked(&connection, id, (self.clock)())?;
        finish_locked(
            &connection,
            id,
            json!({"outcome":"cancelled","server_verified":false}),
            (self.clock)(),
        )?;
        get_locked(&connection, id, (self.clock)())
    }
}
