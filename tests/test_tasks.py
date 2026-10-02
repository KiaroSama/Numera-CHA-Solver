import pytest

from numera_cha_solver.store import TaskStore


def test_cancelled_task_cannot_be_overwritten_and_idempotency_is_stable(tmp_path):
    with TaskStore(tmp_path / "tasks.sqlite3") as store:
        first = store.create("same", "digest", 120)
        assert store.create("same", "digest", 120)["id"] == first["id"]
        store.cancel(first["id"])
        assert store.finish(first["id"], {"outcome": "recognized", "answer": "36"}) is False
        assert store.get(first["id"])["outcome"] == "cancelled"
        with pytest.raises(ValueError, match="different"):
            store.create("same", "other", 120)


def test_restart_interrupts_unfinished_tasks_without_replay(tmp_path):
    path = tmp_path / "tasks.sqlite3"
    with TaskStore(path) as store:
        task = store.create("once", "digest", 120)
    with TaskStore(path) as store:
        assert store.get(task["id"])["outcome"] == "failed"
        assert store.get(task["id"])["result"]["error"]["code"] == "interrupted"
