"""Pure colour math for the activity overlay (no Windows or Tk calls)."""

from __future__ import annotations

INDIGO = (0x3B, 0x2F, 0xD9)
CYAN = (0x22, 0xD3, 0xEE)


def _mix(a: int, b: int, t: float) -> int:
    return round(a + (b - a) * t)


def triangle_wave(value: float) -> float:
    """Map any real number to 0..1 as a smooth-looping triangle wave."""
    phase = value % 1.0
    return 1.0 - abs(phase * 2.0 - 1.0)


def gradient_rgb(position: float, phase: float) -> tuple[int, int, int]:
    """Colour at `position` (0..1 around the frame) for animation `phase`.

    The triangle wave guarantees the colour at 0.0 and 1.0 match, so the frame
    has no visible seam while it rotates.
    """
    t = triangle_wave(position + phase)
    return (_mix(INDIGO[0], CYAN[0], t), _mix(INDIGO[1], CYAN[1], t), _mix(INDIGO[2], CYAN[2], t))


def gradient_hex(position: float, phase: float) -> str:
    r, g, b = gradient_rgb(position, phase)
    return f"#{r:02x}{g:02x}{b:02x}"
