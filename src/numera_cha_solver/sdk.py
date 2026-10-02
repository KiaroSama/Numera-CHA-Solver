"""One async entry point shared by command line and HTTP interfaces."""

import asyncio
import time
from typing import Any

from .config import Settings
from .core.arithmetic import evaluate
from .core.contracts import Error, Input, InputError, Result


class Numera:
    def __init__(self, settings: Settings | None = None):
        self.settings = settings or Settings.from_env()
        from .engines.ocr import OCREngine

        self.engine = OCREngine(self.settings)

    async def detect(self, value: Input | dict[str, Any]) -> dict[str, object]:
        from .detection import detect

        return detect(value if isinstance(value, Input) else Input.from_dict(value))

    async def solve(self, value: Input | dict[str, Any], deadline: float | None = None) -> Result:
        """Local recognition only; no browser action or implied owner acceptance."""
        return await self.recognize(value, deadline)

    async def capabilities(self) -> dict[str, Any]:
        from .capabilities import capabilities

        return await asyncio.to_thread(capabilities, self.settings)

    async def models(self) -> list[dict[str, Any]]:
        from .models import ModelRegistry

        return await asyncio.to_thread(ModelRegistry(self.settings).list)

    async def doctor(self) -> dict[str, Any]:
        from .diagnostics import doctor

        return await asyncio.to_thread(doctor, self.settings)

    async def recognize(
        self, value: Input | dict[str, Any], deadline: float | None = None
    ) -> Result:
        try:
            request = value if isinstance(value, Input) else Input.from_dict(value)
            if deadline is not None and deadline <= time.monotonic():
                return Result("expired", error=Error("expired", "Task deadline exhausted."))
            supported = {
                "text": {"auto", "arithmetic"},
                "image": {"auto", "generic_text_line"},
            }
            if request.kind in supported and request.subtype not in supported[request.kind]:
                return Result(
                    "unsupported",
                    subtype=request.subtype,
                    error=Error(
                        "unsupported_subtype", "No evaluated local engine for this subtype."
                    ),
                )
            if request.kind == "text":
                answer = evaluate(request.text)
                return Result(
                    "recognized",
                    answer=answer,
                    device="cpu",
                    evidence={"grammar": "integer-binary-v1"},
                )
            if request.kind == "image":
                from .assets import image
                from .models import ModelRegistry

                image(request.asset, self.settings)
                response = await self.engine.recognize(
                    request.asset, deadline or (time.monotonic() + self.settings.deadline_seconds)
                )
                text, score = response["text"], response["confidence"]
                abstained = not text or score < self.settings.confidence_threshold
                if request.allowed_characters and any(
                    char not in request.allowed_characters for char in text
                ):
                    abstained = True
                revision = ModelRegistry(self.settings).get("ppocr-v6-small")["revision"]
                return Result(
                    "needs_user_action" if abstained else "recognized",
                    answer=None if abstained else text,
                    confidence=score,
                    abstained=abstained,
                    mode="native_local_ai",
                    subtype="generic_text_line",
                    model_revision=revision,
                    device=response["device"],
                    evidence=response["evidence"],
                )
            return Result(
                "unsupported",
                error=Error(
                    "unsupported_modality",
                    "This input requires an available declared perception engine.",
                ),
            )
        except InputError as exc:
            unavailable = {
                "model_missing",
                "model_corrupt",
                "gpu_unavailable",
                "runtime_unavailable",
            }
            return Result(
                "unsupported" if exc.code in unavailable else "failed",
                error=Error(exc.code, str(exc)),
            )
        except TimeoutError:
            return Result("expired", error=Error("expired", "Task deadline exhausted."))

    def close(self) -> None:
        self.engine.close()

    def recognize_sync(self, value: Input | dict[str, Any]) -> Result:
        try:
            asyncio.get_running_loop()
        except RuntimeError:
            try:
                return asyncio.run(self.recognize(value))
            finally:
                self.close()
        raise RuntimeError("An event loop is already running; await Numera.recognize(...) instead.")
