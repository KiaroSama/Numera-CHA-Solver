"""SQLite task state; terminal results use one atomic compare-and-set."""

import json
import sqlite3
import threading
import time
from pathlib import Path
from typing import Any
from uuid import uuid4


class TaskStore:
    def __init__(self, path: Path, ttl: int = 86_400):
        path.parent.mkdir(parents=True, exist_ok=True)
        self.connection = sqlite3.connect(path, timeout=5, check_same_thread=False)
        self.connection.row_factory = sqlite3.Row
        self.lock = threading.RLock()
        self.ttl = ttl
        self.connection.execute("PRAGMA journal_mode=WAL")
        self.connection.execute(
            "CREATE TABLE IF NOT EXISTS tasks (id TEXT PRIMARY KEY, key TEXT "
            "UNIQUE NOT NULL, digest TEXT NOT NULL, created REAL NOT NULL, "
            "deadline REAL NOT NULL, expires REAL NOT NULL, state TEXT NOT NULL, "
            "outcome TEXT, result TEXT, events TEXT NOT NULL)"
        )
        self.connection.execute(
            "UPDATE tasks SET state='terminal', outcome='failed', result=? WHERE "
            "state != 'terminal'",
            (
                json.dumps(
                    {
                        "outcome": "failed",
                        "error": {
                            "code": "interrupted",
                            "message": "Application restarted; unfinished actions "
                            "were not replayed.",
                        },
                    }
                ),
            ),
        )
        self.connection.commit()

    def create(self, key: str, digest: str, budget: float) -> dict[str, Any]:
        if not key or len(key) > 128 or not 1 <= budget <= 120:
            raise ValueError("Invalid task key or budget.")
        now = time.time()
        with self.lock, self.connection:
            self.connection.execute(
                "DELETE FROM tasks WHERE expires <= ? AND state='terminal'", (now,)
            )
            existing = self.connection.execute("SELECT * FROM tasks WHERE key=?", (key,)).fetchone()
            if existing:
                if existing["digest"] != digest:
                    raise ValueError("Idempotency key was used for a different request.")
                return self._decode(existing)
            identifier = str(uuid4())
            self.connection.execute(
                "INSERT INTO tasks VALUES(?,?,?,?,?,?,?,NULL,NULL,?)",
                (
                    identifier,
                    key,
                    digest,
                    now,
                    now + budget,
                    now + self.ttl,
                    "queued",
                    json.dumps([{"state": "queued"}]),
                ),
            )
            return self.get(identifier)

    @staticmethod
    def _decode(row: sqlite3.Row) -> dict[str, Any]:
        value = dict(row)
        value["result"] = json.loads(value["result"]) if value["result"] else None
        value["events"] = json.loads(value["events"])
        value.pop("key")
        value.pop("digest")
        return value

    def get(self, identifier: str) -> dict[str, Any]:
        with self.lock:
            row = self.connection.execute(
                "SELECT * FROM tasks WHERE id=?", (identifier,)
            ).fetchone()
            if row is None or row["expires"] <= time.time():
                raise KeyError(identifier)
            if row["state"] != "terminal" and row["deadline"] <= time.time():
                self.finish(
                    identifier,
                    {
                        "outcome": "expired",
                        "error": {"code": "expired", "message": "Task deadline exhausted."},
                    },
                )
                row = self.connection.execute(
                    "SELECT * FROM tasks WHERE id=?", (identifier,)
                ).fetchone()
            return self._decode(row)

    def list(self, limit: int = 50) -> list[dict[str, Any]]:
        with self.lock:
            rows = self.connection.execute(
                "SELECT id FROM tasks WHERE expires>? ORDER BY created DESC LIMIT ?",
                (time.time(), min(100, max(1, limit))),
            ).fetchall()
            return [self.get(row["id"]) for row in rows]

    def transition(self, identifier: str, state: str, reason: str | None = None) -> bool:
        order = ["queued", "validating", "detecting", "recognizing", "acting", "verifying"]
        if state not in order:
            raise ValueError("Unknown nonterminal state.")
        with self.lock, self.connection:
            row = self.connection.execute(
                "SELECT state,events,deadline FROM tasks WHERE id=?", (identifier,)
            ).fetchone()
            if row is None or row["state"] == "terminal" or row["deadline"] <= time.time():
                return False
            if order.index(state) <= order.index(row["state"]):
                raise ValueError("Task state cannot move backward or repeat.")
            events = json.loads(row["events"])
            events.append({"state": state, **({"reason": reason} if reason else {})})
            return (
                self.connection.execute(
                    "UPDATE tasks SET state=?, events=? WHERE id=? AND state=?",
                    (state, json.dumps(events), identifier, row["state"]),
                ).rowcount
                == 1
            )

    def finish(self, identifier: str, result: dict[str, Any]) -> bool:
        allowed = {
            "recognized",
            "challenge_completed",
            "server_verified",
            "needs_user_action",
            "unsupported",
            "failed",
            "cancelled",
            "expired",
        }
        if result.get("outcome") not in allowed:
            raise ValueError("Unknown task outcome.")
        with self.lock, self.connection:
            row = self.connection.execute(
                "SELECT deadline FROM tasks WHERE id=?", (identifier,)
            ).fetchone()
            if (
                row is not None
                and row["deadline"] <= time.time()
                and result["outcome"] not in {"cancelled", "expired"}
            ):
                result = {
                    "outcome": "expired",
                    "error": {"code": "expired", "message": "Task deadline exhausted."},
                }
            return (
                self.connection.execute(
                    "UPDATE tasks SET state='terminal',outcome=?,result=? WHERE id=? AND "
                    "state!='terminal'",
                    (result["outcome"], json.dumps(result, ensure_ascii=False), identifier),
                ).rowcount
                == 1
            )

    def cancel(self, identifier: str) -> dict[str, Any]:
        self.get(identifier)
        self.finish(identifier, {"outcome": "cancelled", "server_verified": False})
        return self.get(identifier)

    def close(self) -> None:
        self.connection.close()

    def __enter__(self) -> "TaskStore":
        return self

    def __exit__(self, *args: object) -> None:
        self.close()
