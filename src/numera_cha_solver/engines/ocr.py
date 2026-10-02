"""Bounded process ownership and a single cached inference lease."""

import asyncio
import multiprocessing as mp
import threading
import time
from typing import Any

from ..config import Settings
from ..core.contracts import InputError
from ..models import ModelRegistry
from .ocr_worker import worker


class OCREngine:
    def __init__(self, settings: Settings):
        self.settings = settings
        self.process = None
        self.connection = None
        self.lock = threading.Lock()
        self.async_lock = asyncio.Lock()
        self.state_lock = threading.RLock()

    def _receive(self, deadline: float) -> dict[str, Any]:
        remaining = deadline - time.monotonic()
        connection = self.connection
        if connection is None:
            raise InputError("worker_failed", "Inference worker is not connected.")
        if remaining <= 0 or not connection.poll(remaining):
            raise InputError("expired", "Inference exceeded the inherited task deadline.")
        response = connection.recv()
        if "error" in response:
            raise InputError(response["error"], response["message"])
        return response

    def _run(self, content: bytes, deadline: float) -> dict[str, Any]:
        if not self.lock.acquire(timeout=max(0, deadline - time.monotonic())):
            raise InputError("expired", "Inference queue exceeded its deadline.")
        try:
            if self.process is None or not self.process.is_alive():
                self.close()
                status = ModelRegistry(self.settings).verify("ppocr-v6-small")
                if status["availability"] != "ready":
                    raise InputError(
                        status["availability"],
                        "Reviewed model assets are missing or corrupt; use models fetch or verify.",
                    )
                context = mp.get_context("spawn")
                with self.state_lock:
                    self.connection, child = context.Pipe()
                    self.process = context.Process(
                        target=worker, args=(child, self.settings), daemon=True
                    )
                    self.process.start()
                    child.close()
                self._receive(deadline)
            connection = self.connection
            if connection is None:
                raise InputError("worker_failed", "Inference worker is not connected.")
            connection.send(content)
            return self._receive(deadline)
        except (EOFError, BrokenPipeError, OSError):
            self.close()
            raise InputError(
                "worker_failed", "Owned inference worker exited unexpectedly."
            ) from None
        except InputError:
            self.close()
            raise
        finally:
            self.lock.release()

    async def recognize(self, content: bytes, deadline: float) -> dict[str, Any]:
        async with asyncio.timeout(max(0.001, deadline - time.monotonic())):
            async with self.async_lock:
                job = asyncio.create_task(asyncio.to_thread(self._run, content, deadline))
                try:
                    return await asyncio.shield(job)
                except (asyncio.CancelledError, TimeoutError):
                    self.close()
                    try:
                        await asyncio.wait_for(asyncio.shield(job), 5)
                    except (InputError, OSError, TimeoutError, EOFError):
                        pass
                    raise

    def close(self) -> None:
        with self.state_lock:
            process, connection = self.process, self.connection
            self.process = None
            self.connection = None
            if process is not None:
                if process.is_alive():
                    process.terminate()
                process.join(timeout=3)
                if process.is_alive():
                    process.kill()
                    process.join(timeout=2)
                process.close()
            if connection is not None:
                connection.close()
