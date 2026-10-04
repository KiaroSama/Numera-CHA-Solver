"""Portable settings with explicit local runtime policy."""

import os
from dataclasses import dataclass
from pathlib import Path

from .core.contracts import InputError


def project_root() -> Path:
    configured = os.environ.get("NUMERA_ROOT")
    if configured:
        return Path(configured).expanduser().resolve()
    here = Path(__file__).resolve()
    for parent in here.parents:
        if (parent / "pyproject.toml").is_file():
            return parent
    return Path.home() / ".numera-cha"


@dataclass(frozen=True)
class Settings:
    root: Path
    device: str = "cuda"
    deadline_seconds: float = 120.0
    max_asset_bytes: int = 8 * 1024 * 1024
    max_pixels: int = 16_000_000
    queue_size: int = 32
    result_ttl: int = 86_400
    api_token: str = ""
    dll_directory: str = ""
    confidence_threshold: float = 0.7

    def __post_init__(self) -> None:
        if not isinstance(self.dll_directory, str) or len(self.dll_directory) > 4096:
            raise InputError(
                "invalid_config", "Runtime library directory must be a bounded path string."
            )
        if self.dll_directory and not Path(self.dll_directory).is_dir():
            raise InputError("invalid_config", "Runtime library directory is unavailable.")
        if self.device not in {"cuda", "cpu"}:
            raise InputError("invalid_config", "Device must be explicitly cuda or cpu.")
        if not 1 <= self.deadline_seconds <= 120 or not 1 <= self.queue_size <= 64:
            raise InputError(
                "invalid_config", "Deadline or queue limit is outside the supported bounds."
            )
        if not 0 <= self.confidence_threshold <= 1:
            raise InputError("invalid_config", "Confidence threshold must be between 0 and 1.")
        if (
            not 1 <= self.max_asset_bytes <= 8 * 1024 * 1024
            or not 1 <= self.max_pixels <= 16_000_000
        ):
            raise InputError(
                "invalid_config", "Asset limits cannot exceed the application safety ceiling."
            )
        if not 60 <= self.result_ttl <= 86_400:
            raise InputError("invalid_config", "Result TTL must be between 60 and 86400 seconds.")

    @classmethod
    def from_env(cls, **overrides: object) -> "Settings":
        from typing import Any

        values: dict[str, Any] = dict(
            root=project_root(),
            device=os.environ.get("NUMERA_DEVICE", "cuda"),
            api_token=os.environ.get("NUMERA_API_TOKEN", ""),
            dll_directory=os.environ.get("NUMERA_DLL_DIRECTORY", ""),
        )
        values.update(overrides)
        root = values["root"]
        if not values["dll_directory"]:
            path = root / "data" / "runtime-config.json"
            if path.is_file():
                import json

                try:
                    with path.open("rb") as stream:
                        content = stream.read(64 * 1024 + 1)
                    if len(content) > 64 * 1024:
                        raise ValueError
                    configured = json.loads(content.decode("utf-8"))
                    if not isinstance(configured, dict):
                        raise ValueError
                    directory = configured.get("dll_directory", "")
                    if not isinstance(directory, str):
                        raise ValueError
                except (OSError, ValueError, RecursionError):
                    raise InputError(
                        "invalid_config", "Saved runtime configuration is invalid or unreadable."
                    ) from None
                values["dll_directory"] = directory
        return cls(**values)

    @property
    def data_dir(self) -> Path:
        return self.root / "data"

    @property
    def model_dir(self) -> Path:
        return self.data_dir / "models"

    @property
    def log_dir(self) -> Path:
        return self.root / "logs"
