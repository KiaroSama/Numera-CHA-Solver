import asyncio
from io import BytesIO

from PIL import Image

from numera_cha_solver.config import Settings
from numera_cha_solver.core.contracts import InputError
from numera_cha_solver.engines.ocr import OCREngine
from numera_cha_solver.sdk import Numera


def test_worker_deadline_has_expired_public_outcome(tmp_path, monkeypatch):
    async def expired(self, content, deadline):
        raise InputError("expired", "Controlled worker deadline.")

    monkeypatch.setattr(OCREngine, "recognize", expired)
    content = BytesIO()
    Image.new("RGB", (20, 20), "white").save(content, "PNG")
    sdk = Numera(Settings(root=tmp_path))
    try:
        result = asyncio.run(
            sdk.recognize(
                {"kind": "image", "asset": content.getvalue(), "subtype": "generic_text_line"}
            )
        )
        assert result.outcome == "expired"
        assert result.subtype == "generic_text_line"
        assert result.error.code == "expired"
    finally:
        sdk.close()
