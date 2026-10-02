"""One bounded consumer; SQLite owns outcomes, not late worker callbacks."""

import asyncio
import hashlib
import json
import logging
import time
from typing import Any

from .core.contracts import Input
from .sdk import Numera
from .store import TaskStore

logger = logging.getLogger("numera.task")


class TaskService:
    def __init__(self, sdk: Numera, store: TaskStore):
        self.sdk = sdk
        self.store = store
        self.queue: asyncio.Queue[tuple[str, Input, float]] = asyncio.Queue(
            maxsize=sdk.settings.queue_size
        )
        self.active: dict[str, asyncio.Task[None]] = {}
        self.pending: set[str] = set()
        self.consumer: asyncio.Task[None] | None = None

    async def start(self) -> None:
        self.consumer = asyncio.create_task(self._consume())

    async def create(self, key: str, request: Input) -> dict[str, Any]:
        digest = hashlib.sha256(
            json.dumps(
                {
                    "kind": request.kind,
                    "text": request.text,
                    "asset_sha256": hashlib.sha256(request.asset).hexdigest(),
                    "subtype": request.subtype,
                    "allowed_characters": request.allowed_characters,
                    "revision": request.revision,
                },
                sort_keys=True,
            ).encode("utf-8")
        ).hexdigest()
        task = self.store.create(key, digest, self.sdk.settings.deadline_seconds)
        if task["state"] != "queued":
            return task
        if task["id"] in self.pending or task["id"] in self.active:
            return task
        try:
            self.queue.put_nowait(
                (task["id"], request, time.monotonic() + max(0, task["deadline"] - time.time()))
            )
            self.pending.add(task["id"])
        except asyncio.QueueFull:
            self.store.finish(
                task["id"],
                {
                    "outcome": "failed",
                    "error": {"code": "queue_full", "message": "Task queue is full."},
                },
            )
        return self.store.get(task["id"])

    async def _consume(self) -> None:
        while True:
            identifier, request, deadline = await self.queue.get()
            self.pending.discard(identifier)
            job = asyncio.create_task(self._execute(identifier, request, deadline))
            self.active[identifier] = job
            try:
                await job
            except asyncio.CancelledError:
                consumer = asyncio.current_task()
                if consumer is not None and consumer.cancelling():
                    raise
            finally:
                self.active.pop(identifier, None)
                self.queue.task_done()

    async def _execute(self, identifier: str, request: Input, deadline: float) -> None:
        try:
            if not self.store.transition(identifier, "validating"):
                return
            self.store.transition(identifier, "detecting")
            self.store.transition(identifier, "recognizing")
            async with asyncio.timeout_at(
                asyncio.get_running_loop().time() + max(0, deadline - time.monotonic())
            ):
                result = await self.sdk.recognize(request, deadline=deadline)
            self.store.transition(identifier, "acting", "Local recognition has no browser action.")
            self.store.transition(
                identifier, "verifying", "Recognition does not provide owner-side acceptance."
            )
            self.store.finish(identifier, result.to_dict())
            logger.info("task terminal id=%s outcome=%s", identifier, result.outcome)
        except TimeoutError:
            self.store.finish(
                identifier,
                {
                    "outcome": "expired",
                    "error": {"code": "expired", "message": "Task deadline exhausted."},
                },
            )
        except asyncio.CancelledError:
            self.store.cancel(identifier)
            raise
        except Exception:
            logger.error("task failed id=%s; sensitive exception details suppressed", identifier)
            self.store.finish(
                identifier,
                {
                    "outcome": "failed",
                    "error": {
                        "code": "internal_error",
                        "message": "Task failed; no external action was repeated.",
                    },
                },
            )

    async def cancel(self, identifier: str) -> dict[str, Any]:
        result = self.store.cancel(identifier)
        if identifier in self.active:
            self.active[identifier].cancel()
        return result

    async def close(self) -> None:
        if self.consumer is not None:
            self.consumer.cancel()
            await asyncio.gather(self.consumer, return_exceptions=True)
        self.sdk.close()
        self.store.close()
