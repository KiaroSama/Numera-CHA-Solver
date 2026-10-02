"""Evidence-driven inventory; registry entries are never support claims."""

import json
from importlib.resources import files
from typing import Any

from .config import Settings
from .models import ModelRegistry


def capabilities(settings: Settings) -> dict[str, Any]:
    document = json.loads(
        files("numera_cha_solver")
        .joinpath("resources/capabilities.json")
        .read_text(encoding="utf-8")
    )
    status = ModelRegistry(settings).verify("ppocr-v6-small")
    for row in document["capabilities"]:
        if row.get("model_id") == "ppocr-v6-small":
            row["availability"] = status["availability"]
    return document
