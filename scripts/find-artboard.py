"""Find the white artboard rectangle in a window screenshot.

Usage: python find-artboard.py <png> [window_width window_height]
Prints JSON {"x","y","w","h"} in window pixels (the screenshot is assumed to be
taken at scale 1, so image pixels equal window pixels; pass the real window size
to rescale a downscaled shot).
"""

import json
import sys

from PIL import Image, ImageFilter

WHITE = 248
ERODE = 9  # removes UI text and icons, keeps big white areas


def find_artboard(path: str, window_size: tuple[int, int] | None = None) -> dict:
    image = Image.open(path).convert("L")
    mask = image.point(lambda value: 255 if value >= WHITE else 0).filter(ImageFilter.MinFilter(ERODE))
    box = mask.getbbox()
    if box is None:
        raise SystemExit("no white artboard found; is a document open and zoomed to fit?")
    left, top, right, bottom = box
    # MinFilter shrinks the region by half its size on every side; grow it back.
    pad = ERODE // 2
    left, top, right, bottom = left - pad, top - pad, right + pad, bottom + pad
    scale_x = scale_y = 1.0
    if window_size is not None:
        scale_x = window_size[0] / image.width
        scale_y = window_size[1] / image.height
    return {
        "x": round(left * scale_x),
        "y": round(top * scale_y),
        "w": round((right - left) * scale_x),
        "h": round((bottom - top) * scale_y),
    }


def main() -> None:
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    size = (int(sys.argv[2]), int(sys.argv[3])) if len(sys.argv) >= 4 else None
    print(json.dumps(find_artboard(sys.argv[1], size)))


if __name__ == "__main__":
    main()
