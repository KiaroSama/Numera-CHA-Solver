"""Lifecycle regressions use a synthetic worker, never model-accuracy evidence."""

import asyncio
import multiprocessing as mp
import threading
import time

import pytest

from numera_cha_solver.config import Settings
from numera_cha_solver.engines.ocr import OCREngine
from numera_cha_solver.models import ModelRegistry


def synthetic_worker(connection, settings):
    try:
        connection.send({"ready": True})
        while connection.recv() is not None:
            connection.send({"text": "owned", "confidence": 1, "device": "cpu", "evidence": {}})
    except (EOFError, OSError):
        pass
    finally:
        connection.close()


def test_cancel_during_verification_cannot_publish_late_worker(tmp_path, monkeypatch):
    entered, release = threading.Event(), threading.Event()

    def verify(self, model):
        entered.set()
        assert release.wait(4)
        return {"availability": "ready"}

    monkeypatch.setattr(ModelRegistry, "verify", verify)
    monkeypatch.setattr("numera_cha_solver.engines.ocr.worker", synthetic_worker)

    async def run():
        engine = OCREngine(Settings(root=tmp_path, device="cpu"))
        baseline = {p.pid for p in mp.active_children()}
        task = asyncio.create_task(engine.recognize(b"owned", time.monotonic() + 10))
        try:
            assert await asyncio.to_thread(entered.wait, 3)
            task.cancel()
            # Deliver cancellation before releasing the external verification boundary.
            await asyncio.sleep(0)
            release.set()
            with pytest.raises(asyncio.CancelledError):
                await asyncio.wait_for(task, 6)
            assert not [p for p in mp.active_children() if p.pid not in baseline]
            result = await engine.recognize(b"owned", time.monotonic() + 10)
            assert result["text"] == "owned"
        finally:
            release.set()
            task.cancel()
            await asyncio.gather(task, return_exceptions=True)
            engine.close()

    asyncio.run(run())


def test_cancel_during_handshake_reaps_owned_process(tmp_path, monkeypatch):
    entered, release = threading.Event(), threading.Event()
    original_receive = OCREngine._receive

    def paused_receive(self, connection, deadline):
        entered.set()
        release.wait(4)
        return original_receive(self, connection, deadline)

    monkeypatch.setattr(ModelRegistry, "verify", lambda self, model: {"availability": "ready"})
    monkeypatch.setattr("numera_cha_solver.engines.ocr.worker", synthetic_worker)
    monkeypatch.setattr(OCREngine, "_receive", paused_receive)

    async def run():
        engine = OCREngine(Settings(root=tmp_path, device="cpu"))
        baseline = {p.pid for p in mp.active_children()}
        task = asyncio.create_task(engine.recognize(b"owned", time.monotonic() + 10))
        try:
            assert await asyncio.to_thread(entered.wait, 4)
            task.cancel()
            await asyncio.sleep(0)
            release.set()
            with pytest.raises(asyncio.CancelledError):
                await task
            assert not [p for p in mp.active_children() if p.pid not in baseline]
        finally:
            release.set()
            task.cancel()
            await asyncio.gather(task, return_exceptions=True)
            engine.close()

    asyncio.run(run())
