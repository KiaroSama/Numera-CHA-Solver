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
        self.generation = 0
        self.closed = False
        self.jobs: set[asyncio.Task] = set()

    def _check(self, generation: int, deadline: float) -> None:
        if self.closed or generation != self.generation:
            raise InputError("cancelled", "Inference operation was invalidated.")
        if time.monotonic() >= deadline:
            raise InputError("expired", "Inference deadline exhausted.")

    def _receive(self, connection, deadline: float) -> dict[str, Any]:
        remaining = deadline - time.monotonic()
        if remaining <= 0 or not connection.poll(remaining):
            raise InputError("expired", "Inference exceeded the inherited task deadline.")
        response = connection.recv()
        if "error" in response:
            raise InputError(response["error"], response["message"])
        return response

    def _dispose(self) -> None:
        # Caller owns state_lock; no model verification happens under this lock.
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

    def _invalidate(self, generation: int) -> None:
        with self.state_lock:
            if generation == self.generation:
                self.generation += 1
                self._dispose()

    def _run(self, content: bytes, deadline: float, generation: int) -> dict[str, Any]:
        if not self.lock.acquire(timeout=max(0, deadline - time.monotonic())):
            raise InputError("expired", "Inference queue exceeded its deadline.")
        try:
            with self.state_lock:
                self._check(generation, deadline)
                needs_worker = self.process is None or not self.process.is_alive()
            if needs_worker:
                status = ModelRegistry(self.settings).verify("ppocr-v6-small")
                with self.state_lock:
                    self._check(generation, deadline)
                    if status["availability"] != "ready":
                        raise InputError(
                            status["availability"], "Reviewed model assets are missing or corrupt."
                        )
                    self._dispose()
                    context = mp.get_context("spawn")
                    self.connection, child = context.Pipe()
                    self.process = context.Process(
                        target=worker, args=(child, self.settings), daemon=True
                    )
                    try:
                        self.process.start()
                    except Exception:
                        self.process.close()
                        self.process = None
                        self.connection.close()
                        self.connection = None
                        raise
                    finally:
                        child.close()
                    connection = self.connection
                self._receive(connection, deadline)
            with self.state_lock:
                self._check(generation, deadline)
                connection = self.connection
                if connection is None:
                    raise InputError("worker_failed", "Inference worker is not connected.")
            connection.send(content)
            response = self._receive(connection, deadline)
            with self.state_lock:
                self._check(generation, deadline)
            return response
        except InputError:
            self._invalidate(generation)
            raise
        except (EOFError, BrokenPipeError, OSError, ValueError):
            self._invalidate(generation)
            raise InputError(
                "worker_failed", "Owned inference worker exited unexpectedly."
            ) from None
        finally:
            self.lock.release()

    def _job_done(self, job: asyncio.Task) -> None:
        self.jobs.discard(job)
        if not job.cancelled():
            job.exception()

    async def recognize(self, content: bytes, deadline: float) -> dict[str, Any]:
        if self.jobs:
            raise InputError("inference_busy", "Previous inference cleanup is still draining.")
        async with asyncio.timeout(max(0.001, deadline - time.monotonic())):
            async with self.async_lock:
                with self.state_lock:
                    self._check(self.generation, deadline)
                    generation = self.generation
                job = asyncio.create_task(
                    asyncio.to_thread(self._run, content, deadline, generation)
                )
                self.jobs.add(job)
                job.add_done_callback(self._job_done)
                try:
                    return await asyncio.shield(job)
                except (asyncio.CancelledError, TimeoutError):
                    self._invalidate(generation)
                    try:
                        await asyncio.wait_for(asyncio.shield(job), 5)
                    except (InputError, OSError, TimeoutError, EOFError):
                        pass
                    raise

    def close(self) -> None:
        with self.state_lock:
            self.closed = True
            self.generation += 1
            self._dispose()
