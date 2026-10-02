"""Reversible owned-fixture geometry with revision-bound actions."""

import math
from dataclasses import dataclass

from .contracts import InputError


@dataclass(frozen=True)
class Transform:
    width: float
    height: float
    scale_x: float = 1
    scale_y: float = 1
    offset_x: float = 0
    offset_y: float = 0

    def __post_init__(self) -> None:
        values = (self.width, self.height, self.scale_x, self.scale_y, self.offset_x, self.offset_y)
        if not all(math.isfinite(v) for v in values) or min(values[:4]) <= 0:
            raise InputError(
                "invalid_geometry", "Dimensions and scale must be finite and positive."
            )

    def to_viewport(self, x: float, y: float) -> tuple[float, float]:
        self.validate(x, y)
        return x * self.scale_x + self.offset_x, y * self.scale_y + self.offset_y

    def to_image(self, x: float, y: float) -> tuple[float, float]:
        point = ((x - self.offset_x) / self.scale_x, (y - self.offset_y) / self.scale_y)
        self.validate(*point)
        return point

    def validate(self, x: float, y: float) -> None:
        if (
            not math.isfinite(x)
            or not math.isfinite(y)
            or not (0 <= x < self.width and 0 <= y < self.height)
        ):
            raise InputError("invalid_geometry", "Action is outside the authorized image region.")


def validate_revision(expected: str, actual: str) -> None:
    if not expected or expected != actual:
        raise InputError(
            "stale_observation", "Image revision changed; observe and recognize again."
        )
