"""Owned ASCII smoke: real local weights, not real CAPTCHA accuracy."""

import asyncio
import json
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

from numera_cha_solver.assets import read_asset
from numera_cha_solver.config import Settings
from numera_cha_solver.logging import close, initialize
from numera_cha_solver.models import ModelRegistry
from numera_cha_solver.sdk import Numera

ROOT = Path(__file__).resolve().parents[1]


async def main() -> int:
    settings = Settings.from_env(root=ROOT)
    initialize(settings.log_dir, "model-smoke")
    sdk = Numera(settings)
    try:
        registry = ModelRegistry(settings)
        status = await asyncio.to_thread(registry.fetch, "ppocr-v6-small")
        if status["availability"] != "ready":
            print(json.dumps(status))
            return 3
        fixture = ROOT / "tests/fixtures/ocr/ascii-smoke.png"
        if not fixture.exists():
            fixture.parent.mkdir(parents=True, exist_ok=True)
            picture = Image.new("RGB", (400, 80), "white")
            draw = ImageDraw.Draw(picture)
            font = ImageFont.load_default(size=46)
            draw.text((15, 8), "NUMERA 123", font=font, fill="black")
            picture.save(fixture, "PNG")
        result = await sdk.recognize({"kind": "image", "asset": read_asset(fixture, settings)})
        evidence = {
            "fixture_kind": "owned_synthetic_ascii_smoke",
            "expected": "NUMERA 123",
            "result": result.to_dict(),
            "captcha_accuracy_measured": False,
        }
        target = ROOT / "data/evidence/model-smoke.json"
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(evidence, indent=2))
        return 0 if result.answer == "NUMERA 123" else 4
    finally:
        sdk.close()
        close()


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
