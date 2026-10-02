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


def test_graceful_shutdown_cancels_queued_tasks_without_replay(tmp_path):
    import asyncio

    from numera_cha_solver.config import Settings
    from numera_cha_solver.core.contracts import Input
    from numera_cha_solver.sdk import Numera
    from numera_cha_solver.tasks import TaskService

    async def run():
        path = tmp_path / "tasks.sqlite3"
        service = TaskService(Numera(Settings(root=tmp_path)), TaskStore(path))
        task = await service.create("queued", Input("text", text="7+5"))
        await service.close()
        with TaskStore(path) as reopened:
            assert reopened.get(task["id"])["outcome"] == "cancelled"

    asyncio.run(run())


def test_task_expiry_and_ttl_use_persisted_metadata(tmp_path, monkeypatch):
    now = [1000.0]
    monkeypatch.setattr("numera_cha_solver.store.time.time", lambda: now[0])
    with TaskStore(tmp_path / "tasks.sqlite3", ttl=60) as store:
        task = store.create("one", "digest", 1)
        assert store.transition(task["id"], "validating") is True
        with pytest.raises(ValueError, match="backward"):
            store.transition(task["id"], "queued")
        now[0] = 1002
        assert store.get(task["id"])["outcome"] == "expired"
        assert store.finish(task["id"], {"outcome": "recognized"}) is False
        now[0] = 1061
        with pytest.raises(KeyError):
            store.get(task["id"])
        assert store.list() == []
        replacement = store.create("one", "new-digest", 1)
        assert replacement["id"] != task["id"]


def test_queue_full_is_terminal_and_duplicate_is_not_enqueued_twice(tmp_path):
    import asyncio

    from numera_cha_solver.config import Settings
    from numera_cha_solver.core.contracts import Input
    from numera_cha_solver.sdk import Numera
    from numera_cha_solver.tasks import TaskService

    async def run():
        store = TaskStore(tmp_path / "tasks.sqlite3")
        service = TaskService(Numera(Settings(root=tmp_path, queue_size=1)), store)
        try:
            request = Input("text", text="1+2")
            first = await service.create("one", request)
            assert (await service.create("one", request))["id"] == first["id"]
            rejected = await service.create("two", request)
            assert rejected["state"] == "terminal"
            assert rejected["result"]["error"]["code"] == "queue_full"
            assert (await service.cancel(first["id"]))["outcome"] == "cancelled"
        finally:
            await service.close()

    asyncio.run(run())


def test_restart_interrupts_unfinished_tasks_without_replay(tmp_path):
    path = tmp_path / "tasks.sqlite3"
    with TaskStore(path) as store:
        task = store.create("once", "digest", 120)
    with TaskStore(path) as store:
        assert store.get(task["id"])["outcome"] == "failed"
        assert store.get(task["id"])["result"]["error"]["code"] == "interrupted"
