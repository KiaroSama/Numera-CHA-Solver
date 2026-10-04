"""One retained integrity refresh; callers consume a timestamped snapshot."""

import asyncio
import time
from datetime import UTC, datetime
from typing import Any

from .config import Settings
from .models import ModelRegistry


class Readiness:
    def __init__(self, settings: Settings):
        self.registry = ModelRegistry(settings)
        self.task: asyncio.Task | None = None
        self.checked = 0.0
        self.snapshot: dict[str, Any] = {"availability": "checking"}
        self.checked_at: str | None = None
        self.closed = False

    async def _refresh(self) -> None:
        try:
            self.snapshot = await asyncio.to_thread(self.registry.verify, "ppocr-v6-small")
        except Exception:
            self.snapshot = {"availability": "check_failed"}
        finally:
            self.checked = time.monotonic()
            self.checked_at = datetime.now(UTC).isoformat()

    async def get(self) -> dict[str, Any]:
        if (
            not self.closed
            and (self.task is None or self.task.done())
            and (not self.checked or time.monotonic() - self.checked >= 30)
        ):
            self.task = asyncio.create_task(self._refresh())
        # No caller waits on disk; at most one refresh exists, including after timeout.
        return {
            "ready": True,
            "arithmetic_ready": True,
            "model_assets_ready": self.snapshot["availability"] == "ready",
            "model": dict(self.snapshot),
            "checked_at": self.checked_at,
            "refreshing": self.task is not None and not self.task.done(),
            "gpu_execution_verified": False,
        }

    async def close(self) -> None:
        self.closed = True
        if self.task is not None:
            try:
                await asyncio.wait_for(asyncio.shield(self.task), 5)
            except TimeoutError:
                import logging

                logging.getLogger("numera").warning(
                    "readiness refresh still draining after shutdown budget"
                )
