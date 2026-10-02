import asyncio

from numera_cha_solver.config import Settings
from numera_cha_solver.sdk import Numera


def test_image_without_weights_is_not_fake_recognition(tmp_path):
    from io import BytesIO

    from PIL import Image

    stream = BytesIO()
    Image.new("RGB", (120, 48), "white").save(stream, format="PNG")
    result = asyncio.run(
        Numera(Settings(root=tmp_path)).recognize({"kind": "image", "asset": stream.getvalue()})
    )
    assert result.outcome == "unsupported"
    assert result.error.code == "model_missing"
    assert result.server_verified is False
