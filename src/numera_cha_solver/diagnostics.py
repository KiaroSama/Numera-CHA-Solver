"""Readiness is separate from successful inference."""

import importlib.metadata
import platform
import shutil
import subprocess
from typing import Any

from .config import Settings
from .models import ModelRegistry


def doctor(settings: Settings) -> dict[str, Any]:
    versions = {}
    for name in ("onnxruntime-gpu", "onnxruntime", "numpy", "pillow", "fastapi"):
        try:
            versions[name] = importlib.metadata.version(name)
        except importlib.metadata.PackageNotFoundError:
            versions[name] = None
    gpu = []
    executable = shutil.which("nvidia-smi")
    if executable:
        try:
            flags = subprocess.CREATE_NO_WINDOW if platform.system() == "Windows" else 0
            process = subprocess.run(
                [
                    executable,
                    "--query-gpu=name,memory.total,memory.free,driver_version",
                    "--format=csv,noheader,nounits",
                ],
                capture_output=True,
                text=True,
                encoding="utf-8",
                timeout=5,
                creationflags=flags,
            )
            if process.returncode == 0:
                gpu = [line.split(", ") for line in process.stdout.strip().splitlines()]
        except (OSError, subprocess.TimeoutExpired):
            pass
    return {
        "python": platform.python_version(),
        "platform": platform.system(),
        "runtime_versions": versions,
        "configured_device": settings.device,
        "gpu_inventory": gpu,
        "models": ModelRegistry(settings).list(),
        "gpu_execution_verified": False,
        "note": "Inventory is not inference evidence. Run doctor --smoke with a "
        "licensed local image.",
    }
