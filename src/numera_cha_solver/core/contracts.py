"""Recognition never implies owner-side acceptance."""

from dataclasses import asdict, dataclass, field
from typing import Any, Literal

Outcome = Literal[
    "recognized",
    "challenge_completed",
    "server_verified",
    "needs_user_action",
    "unsupported",
    "failed",
    "cancelled",
    "expired",
]


@dataclass(frozen=True)
class Error:
    code: str
    message: str


@dataclass(frozen=True)
class Result:
    outcome: Outcome
    answer: str | None = None
    confidence: float | None = None
    abstained: bool = False
    server_verified: bool = False
    mode: str = "local_classical"
    provider: str = "generic"
    subtype: str = "arithmetic"
    model_revision: str | None = None
    device: str | None = None
    error: Error | None = None
    evidence: dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


class InputError(ValueError):
    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code


@dataclass(frozen=True)
class Input:
    kind: Literal["text", "image", "audio", "browser"]
    text: str = ""
    asset: bytes = b""
    subtype: str = "auto"
    allowed_characters: str = ""
    revision: str = ""

    def __post_init__(self) -> None:
        if not isinstance(self.kind, str) or self.kind not in {"text", "image", "audio", "browser"}:
            raise InputError("invalid_input", "Unsupported input kind.")
        for name in ("text", "subtype", "allowed_characters", "revision"):
            if not isinstance(getattr(self, name), str):
                raise InputError("invalid_input", f"{name} must be a string.")
        if not isinstance(self.asset, bytes):
            raise InputError("invalid_input", "Asset must contain local bytes.")
        if len(self.text) > 4096 or len(self.allowed_characters) > 256:
            raise InputError("invalid_input", "Text or character set exceeds its limit.")
        if len(self.subtype) > 64 or len(self.revision) > 128 or len(self.asset) > 8 * 1024 * 1024:
            raise InputError("invalid_input", "Observation exceeds its size limit.")

    @classmethod
    def from_dict(cls, value: dict[str, Any]) -> "Input":
        if not isinstance(value, dict):
            raise InputError("invalid_input", "Input must be an observation object.")
        allowed = {"kind", "text", "asset", "subtype", "allowed_characters", "revision"}
        if set(value) - allowed:
            raise InputError("invalid_input", "Unknown input fields are not permitted.")
        if not isinstance(value.get("kind"), str) or value["kind"] not in {
            "text",
            "image",
            "audio",
            "browser",
        }:
            raise InputError("invalid_input", "Input kind must be text, image, audio or browser.")
        for name in allowed - {"kind", "asset"}:
            if not isinstance(value.get(name, ""), str):
                raise InputError("invalid_input", f"{name} must be a string.")
        if not isinstance(value.get("asset", b""), bytes):
            raise InputError("invalid_input", "Asset must contain local bytes, not a URL.")
        if len(value.get("text", "")) > 4096 or len(value.get("allowed_characters", "")) > 256:
            raise InputError("invalid_input", "Text or character set exceeds its size limit.")
        return cls(**value)
