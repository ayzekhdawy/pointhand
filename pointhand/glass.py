"""Glass-style rendering for the activity overlay (pure Pillow, no Windows calls).

Everything returns straight-alpha RGBA images; the overlay converts to
premultiplied BGRA only at the last step. The soft glow is drawn at 1/4
resolution and blurred, which is both cheap and exactly the look we want.
"""

from __future__ import annotations

import math
from functools import lru_cache
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

from .gradient import gradient_rgb

GLOW_DIV = 4          # glow layer resolution divisor
RING_DIV = 2          # crisp ring resolution divisor
CURSOR_PAD = 36       # sprite margin around the cursor tip (glow room)
PILL_SIZE = (580, 46)
FONT_PATHS = (r"C:\Windows\Fonts\segoeuib.ttf", r"C:\Windows\Fonts\arialbd.ttf")

Point = tuple[float, float]


def rounded_rect_points(x0: float, y0: float, x1: float, y1: float, radius: float, per_corner: int = 8) -> list[Point]:
    """Clockwise outline of a rounded rectangle, starting at the top-left of the top edge."""
    corners = ((x1 - radius, y0 + radius, -90), (x1 - radius, y1 - radius, 0),
               (x0 + radius, y1 - radius, 90), (x0 + radius, y0 + radius, 180))
    points: list[Point] = []
    for cx, cy, start in corners:
        for i in range(per_corner + 1):
            angle = math.radians(start + 90 * i / per_corner)
            points.append((cx + radius * math.cos(angle), cy + radius * math.sin(angle)))
    return points


def _draw_gradient_loop(draw: ImageDraw.ImageDraw, points: list[Point], phase: float, alpha: int, width: int) -> None:
    count = len(points)
    for i in range(count):
        color = gradient_rgb(i / count, phase)
        draw.line([points[i], points[(i + 1) % count]], fill=(*color, alpha), width=width, joint="curve")


def composite_at(base: Image.Image, sprite: Image.Image, x: int, y: int) -> None:
    """Alpha-composite `sprite` onto `base` at (x, y), clipping at the edges."""
    left, top = max(0, x), max(0, y)
    right, bottom = min(base.width, x + sprite.width), min(base.height, y + sprite.height)
    if right <= left or bottom <= top:
        return
    region = base.crop((left, top, right, bottom))
    piece = sprite.crop((left - x, top - y, right - x, bottom - y))
    base.paste(Image.alpha_composite(region, piece), (left, top))


def render_frame(size: tuple[int, int], phase: float, intensity: float = 1.0) -> Image.Image:
    """The animated glowing frame: soft outer glow plus a crisp gradient ring."""
    width, height = size
    lw, lh = max(8, width // GLOW_DIV), max(8, height // GLOW_DIV)
    glow = Image.new("RGBA", (lw, lh), (0, 0, 0, 0))
    glow_points = rounded_rect_points(2, 2, lw - 3, lh - 3, radius=max(4, lw // 40))
    _draw_gradient_loop(ImageDraw.Draw(glow), glow_points, phase, int(235 * intensity), width=5)
    glow = glow.filter(ImageFilter.GaussianBlur(3.4)).resize((width, height), Image.Resampling.BICUBIC)

    rw, rh = max(8, width // RING_DIV), max(8, height // RING_DIV)
    ring = Image.new("RGBA", (rw, rh), (0, 0, 0, 0))
    ring_points = rounded_rect_points(4, 4, rw - 5, rh - 5, radius=max(8, rw // 20), per_corner=14)
    _draw_gradient_loop(ImageDraw.Draw(ring), ring_points, phase, 255, width=2)
    ring = ring.resize((width, height), Image.Resampling.BILINEAR)
    return Image.alpha_composite(glow, ring)


@lru_cache(maxsize=4)
def _font(size: int) -> ImageFont.ImageFont:
    for path in FONT_PATHS:
        if Path(path).exists():
            return ImageFont.truetype(path, size)
    return ImageFont.load_default()


@lru_cache(maxsize=4)
def render_pill(text: str, size: tuple[int, int] = PILL_SIZE) -> Image.Image:
    """Frosted-glass label: translucent body, top light edge, gradient hairline, text."""
    scale = 3  # supersample for smooth edges
    width, height = size[0] * scale, size[1] * scale
    pill = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    draw = ImageDraw.Draw(pill)
    radius = height // 2
    draw.rounded_rectangle((0, 0, width - 1, height - 1), radius=radius, fill=(26, 24, 58, 150))
    # soft inner highlight on the upper half gives the glass sheen
    sheen = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    ImageDraw.Draw(sheen).rounded_rectangle((scale * 2, scale * 2, width - scale * 2, height // 2),
                                            radius=radius, fill=(255, 255, 255, 34))
    pill = Image.alpha_composite(pill, sheen)
    border = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    ImageDraw.Draw(border).rounded_rectangle((0, 0, width - 1, height - 1), radius=radius,
                                             outline=(160, 170, 255, 120), width=scale)
    pill = Image.alpha_composite(pill, border)
    draw = ImageDraw.Draw(pill)
    dot = height // 6
    cy = height // 2
    draw.ellipse((radius, cy - dot, radius + 2 * dot, cy + dot), fill=(34, 211, 238, 255))
    font = _font(15 * scale)
    draw.text((radius + 4 * dot, cy), text, font=font, fill=(240, 238, 255, 245), anchor="lm")
    return pill.resize(size, Image.Resampling.LANCZOS)


@lru_cache(maxsize=1)
def render_cursor_sprite() -> Image.Image:
    """The agent's own pointer: indigo arrow, white edge, cyan halo. Tip at (CURSOR_PAD, CURSOR_PAD)."""
    scale = 4
    side = (CURSOR_PAD * 2 + 28) * scale
    sprite = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    tip = CURSOR_PAD * scale
    halo = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    ImageDraw.Draw(halo).ellipse((tip - 16 * scale, tip - 16 * scale, tip + 16 * scale, tip + 16 * scale),
                                 fill=(34, 211, 238, 150))
    halo = halo.filter(ImageFilter.GaussianBlur(8 * scale))
    sprite = Image.alpha_composite(sprite, halo)
    shape = [(0, 0), (0, 24), (6, 18), (11, 29), (15, 27), (10, 17), (18, 17)]
    pts = [(tip + px * scale * 1.1, tip + py * scale * 1.1) for px, py in shape]
    draw = ImageDraw.Draw(sprite)
    draw.polygon(pts, fill=(59, 47, 217, 255), outline=(255, 255, 255, 255), width=2 * scale)
    return sprite.resize((side // scale, side // scale), Image.Resampling.LANCZOS)


@lru_cache(maxsize=64)
def render_dot(radius: int, alpha: int) -> Image.Image:
    """A soft cyan dot used for the cursor halo and the motion trail."""
    side = radius * 2 + 12
    dot = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    center = side / 2
    ImageDraw.Draw(dot).ellipse((center - radius, center - radius, center + radius, center + radius),
                                fill=(34, 211, 238, max(0, min(255, alpha))))
    return dot.filter(ImageFilter.GaussianBlur(max(1.0, radius / 3)))


def render_ripple(progress: float) -> Image.Image:
    """Expanding, fading click ring. `progress` runs 0..1."""
    progress = min(1.0, max(0.0, progress))
    radius = 10 + 40 * progress
    side = int(radius * 2 + 16)
    ripple = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    alpha = int(220 * (1.0 - progress))
    center = side / 2
    ImageDraw.Draw(ripple).ellipse((center - radius, center - radius, center + radius, center + radius),
                                   outline=(34, 211, 238, alpha), width=3)
    return ripple.filter(ImageFilter.GaussianBlur(0.8))


def to_premultiplied_bgra(image: Image.Image) -> bytes:
    """Bytes in the layout UpdateLayeredWindow expects (premultiplied BGRA)."""
    return image.convert("RGBa").tobytes("raw", "BGRa")
