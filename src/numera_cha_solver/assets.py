"""Bounded image decoding shared by every interface."""

import io
import warnings
from pathlib import Path

from PIL import Image, ImageOps, UnidentifiedImageError

from .config import Settings
from .core.contracts import InputError


def read_asset(path: Path, settings: Settings) -> bytes:
    if not path.is_file() or path.is_symlink():
        raise InputError("invalid_asset", "Asset must be a regular local file.")
    with path.open("rb") as stream:
        content = stream.read(settings.max_asset_bytes + 1)
    if len(content) > settings.max_asset_bytes:
        raise InputError("asset_too_large", "Asset exceeds the byte limit.")
    return content


def image(content: bytes, settings: Settings) -> Image.Image:
    if not content or len(content) > settings.max_asset_bytes:
        raise InputError("invalid_asset", "Image is empty or exceeds the byte limit.")
    try:
        with warnings.catch_warnings():
            warnings.simplefilter("error", Image.DecompressionBombWarning)
            with Image.open(io.BytesIO(content)) as source:
                if source.format not in {"PNG", "JPEG", "WEBP", "BMP"}:
                    raise InputError(
                        "invalid_asset", "Only PNG, JPEG, WEBP and BMP still images are accepted."
                    )
                if source.width * source.height > settings.max_pixels or min(source.size) < 2:
                    raise InputError(
                        "invalid_asset", "Image dimensions exceed the supported limits."
                    )
                if getattr(source, "n_frames", 1) != 1:
                    raise InputError(
                        "unsupported_temporal",
                        "Animated images require an independently evaluated temporal engine.",
                    )
                fixed = ImageOps.exif_transpose(source).convert("RGBA")
                background = Image.new("RGBA", fixed.size, "white")
                background.alpha_composite(fixed)
                return background.convert("RGB")
    except (
        UnidentifiedImageError,
        OSError,
        Image.DecompressionBombError,
        Image.DecompressionBombWarning,
    ):
        raise InputError("invalid_asset", "Image could not be safely decoded.") from None
