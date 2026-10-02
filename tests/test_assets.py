import io

import pytest
from PIL import Image

from numera_cha_solver.assets import image, read_asset
from numera_cha_solver.config import Settings
from numera_cha_solver.core.contracts import InputError


@pytest.mark.parametrize("content", [b"", b"not an image", b"<html>error</html>"])
def test_invalid_asset_is_rejected(tmp_path, content):
    with pytest.raises(InputError, match="Image|image"):
        image(content, Settings(root=tmp_path))


def test_transparency_dimensions_and_unicode_path(tmp_path):
    content = io.BytesIO()
    Image.new("RGBA", (30, 20), (0, 0, 0, 0)).save(content, "PNG")
    path = tmp_path / "local sample فارسی.png"
    path.write_bytes(content.getvalue())
    settings = Settings(root=tmp_path)
    result = image(read_asset(path, settings), settings)
    assert result.getpixel((0, 0)) == (255, 255, 255)
    with pytest.raises(InputError, match="dimensions"):
        image(content.getvalue(), Settings(root=tmp_path, max_pixels=100))
