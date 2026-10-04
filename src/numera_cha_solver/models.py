"""Manifest-bound local assets; no arbitrary model URL or remote code."""

import hashlib
import json
import logging
import os
import tempfile
import time
from importlib.resources import files
from pathlib import Path
from typing import Any
from urllib.parse import urlsplit

import httpx

from .config import Settings
from .core.contracts import InputError

logger = logging.getLogger("numera.model")


class ModelRegistry:
    def __init__(self, settings: Settings):
        self.settings = settings
        manifest = json.loads(
            files("numera_cha_solver").joinpath("resources/models.json").read_text(encoding="utf-8")
        )
        self.models = {model["id"]: model for model in manifest["models"]}

    def get(self, model_id: str) -> dict[str, Any]:
        if model_id not in self.models:
            raise InputError("unknown_model", "Model is not in the reviewed manifest.")
        return self.models[model_id]

    def directory(self, model_id: str) -> Path:
        model = self.get(model_id)
        return self.settings.model_dir / model_id / model["revision"]

    def verify(self, model_id: str) -> dict[str, Any]:
        model = self.get(model_id)
        failures = []
        deadline = time.monotonic() + self.settings.deadline_seconds
        directory = self.directory(model_id)
        for asset in model["assets"]:
            path = directory / asset["file"]
            if not path.is_file():
                failures.append({"file": asset["file"], "reason": "missing"})
                continue
            if path.is_symlink() or path.stat().st_size != asset["size"]:
                failures.append({"file": asset["file"], "reason": "size_or_path_mismatch"})
                continue
            digest = hashlib.sha256()
            with path.open("rb") as stream:
                while chunk := stream.read(1024 * 1024):
                    if time.monotonic() >= deadline:
                        raise InputError(
                            "expired", "Model integrity verification deadline exhausted."
                        )
                    digest.update(chunk)
            if digest.hexdigest() != asset["sha256"]:
                failures.append({"file": asset["file"], "reason": "checksum_mismatch"})
        availability = (
            "ready"
            if not failures
            else (
                "model_missing"
                if all(f["reason"] == "missing" for f in failures)
                else "model_corrupt"
            )
        )
        return {
            "id": model_id,
            "revision": model["revision"],
            "license": model["license"],
            "availability": availability,
            "failures": failures,
        }

    def list(self) -> list[dict[str, Any]]:
        return [self.verify(model_id) for model_id in self.models]

    def fetch(self, model_id: str) -> dict[str, Any]:
        model = self.get(model_id)
        directory = self.directory(model_id)
        directory.mkdir(parents=True, exist_ok=True)
        from .ownership import ServerOwnership

        lease = ServerOwnership(directory / ".download.lock")
        try:
            lease.acquire()
        except RuntimeError:
            raise InputError(
                "model_busy", "Another model download owns this revision; retry after it completes."
            ) from None
        deadline = time.monotonic() + self.settings.deadline_seconds
        try:
            with httpx.Client(timeout=30, follow_redirects=False, trust_env=False) as client:
                for asset in model["assets"]:
                    if time.monotonic() >= deadline:
                        raise InputError("expired", "Model download exceeded its overall deadline.")
                    path = directory / asset["file"]
                    if path.is_file() and path.stat().st_size == asset["size"]:
                        with path.open("rb") as stream:
                            if hashlib.file_digest(stream, "sha256").hexdigest() == asset["sha256"]:
                                continue
                    self._download(client, asset, path, deadline)
            logger.info(
                "model acquisition completed id=%s revision=%s", model_id, model["revision"]
            )
            return self.verify(model_id)
        except httpx.HTTPError:
            raise InputError(
                "download_failed", "Model download failed; no incomplete asset was published."
            ) from None
        finally:
            lease.close()

    @staticmethod
    def _download(
        client: httpx.Client, asset: dict[str, Any], destination: Path, deadline: float
    ) -> None:
        url = asset["url"]
        allowed = {
            "huggingface.co",
            "raw.githubusercontent.com",
            "cdn-lfs.huggingface.co",
            "cdn-lfs-us-1.hf.co",
            "cas-bridge.xethub.hf.co",
            "us.aws.cdn.hf.co",
        }
        redirects = 0
        temporary: Path | None = None
        try:
            while True:
                parsed = urlsplit(url)
                if (
                    parsed.scheme != "https"
                    or parsed.hostname not in allowed
                    or parsed.username
                    or parsed.password
                ):
                    raise InputError(
                        "download_origin", "Model redirect is outside reviewed asset origins."
                    )
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise InputError("expired", "Model acquisition deadline exhausted.")
                with client.stream("GET", url, timeout=min(30, remaining)) as response:
                    if response.is_redirect:
                        redirects += 1
                        if redirects > 3:
                            raise InputError("download_redirects", "Too many model redirects.")
                        url = str(response.url.join(response.headers["location"]))
                        continue
                    response.raise_for_status()
                    if "text/html" in response.headers.get("content-type", ""):
                        raise InputError("invalid_model", "An HTML page is not a model asset.")
                    digest = hashlib.sha256()
                    size = 0
                    with tempfile.NamedTemporaryFile(
                        dir=destination.parent, delete=False
                    ) as stream:
                        temporary = Path(stream.name)
                        for chunk in response.iter_bytes(64 * 1024):
                            if time.monotonic() >= deadline:
                                raise InputError("expired", "Model download deadline exhausted.")
                            size += len(chunk)
                            if size > asset["size"]:
                                raise InputError(
                                    "invalid_model", "Downloaded asset exceeds manifest size."
                                )
                            digest.update(chunk)
                            stream.write(chunk)
                        stream.flush()
                        os.fsync(stream.fileno())
                    if size != asset["size"] or digest.hexdigest() != asset["sha256"]:
                        raise InputError(
                            "invalid_model",
                            "Downloaded asset does not match its immutable manifest.",
                        )
                    temporary.replace(destination)
                    temporary = None
                    return
        finally:
            if temporary is not None:
                temporary.unlink(missing_ok=True)
