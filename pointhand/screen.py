"""Screen capture: one grab of the virtual desktop, cropped and downscaled."""

from __future__ import annotations

import io

from PIL import Image, ImageGrab

from . import native
from .keys import clamp_region

MAX_EDGE = 1568


def capture_png(region: list[int] | None = None, scale: float = 0.5) -> tuple[bytes, float]:
    """Return (png_bytes, effective_scale).

    Coordinates in `region` are virtual-desktop pixels. The returned scale is
    what was actually applied, so callers can map image pixels back with
    `pixel / effective_scale`.
    """
    if not 0.05 <= scale <= 1.0:
        raise ValueError("scale must be between 0.05 and 1.0")
    origin, size = native.virtual_screen()
    frame = ImageGrab.grab(all_screens=True)
    if region is not None:
        left, top, right, bottom = clamp_region(region, origin, size)
        frame = frame.crop((left - origin[0], top - origin[1], right - origin[0], bottom - origin[1]))
    longest = max(frame.size)
    effective = min(scale, MAX_EDGE / longest) if longest else scale
    if effective < 1.0:
        resized = (max(1, round(frame.width * effective)), max(1, round(frame.height * effective)))
        frame = frame.resize(resized, Image.Resampling.LANCZOS)
    buffer = io.BytesIO()
    frame.convert("RGB").save(buffer, format="PNG", optimize=False)
    return buffer.getvalue(), effective
