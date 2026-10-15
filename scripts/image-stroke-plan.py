"""Convert reference pixels to colored horizontal strokes; never imports an image into Paint."""
import argparse
import json
from collections import Counter
from pathlib import Path
from PIL import Image, ImageFilter


def make_plan(source, width=215, colors=64, pixel_size=2, blur=0.9):
    with Image.open(source) as opened:
        height = round(width * opened.height / opened.width)
        image = opened.convert("RGB").resize((width, height), Image.Resampling.LANCZOS)
    if blur:
        image = image.filter(ImageFilter.GaussianBlur(blur))
    indexed = image.quantize(colors=colors, dither=Image.Dither.NONE)
    pixels = list(indexed.get_flattened_data())
    palette = indexed.getpalette()
    groups = {index: [] for index in set(pixels)}
    counts = Counter(pixels)
    for y in range(height):
        start = 0
        for x in range(1, width + 1):
            color = pixels[y * width + start]
            if x == width or pixels[y * width + x] != color:
                groups[color].append([start * pixel_size + pixel_size // 2,
                                      y * pixel_size + pixel_size // 2,
                                      (x - 1) * pixel_size + pixel_size // 2,
                                      y * pixel_size + pixel_size // 2])
                start = x
    ordered = sorted(groups, key=lambda index: -counts[index])
    return {"source": Path(source).name, "grid": [width, height],
            "canvas": [width * pixel_size, height * pixel_size], "pen_size": pixel_size,
            "stroke_count": sum(map(len, groups.values())),
            "colors": [{"hex": "#" + "".join(f"{channel:02X}" for channel in palette[index * 3:index * 3 + 3]),
                        "pixels": counts[index], "strokes": groups[index]} for index in ordered]}


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("source")
    parser.add_argument("output")
    parser.add_argument("--width", type=int, default=215)
    parser.add_argument("--colors", type=int, default=64)
    parser.add_argument("--pixel-size", type=int, default=2)
    parser.add_argument("--blur", type=float, default=0.9)
    args = parser.parse_args()
    if not (1 <= args.width <= 1024 and 2 <= args.colors <= 256 and 1 <= args.pixel_size <= 16 and 0 <= args.blur <= 5):
        parser.error("invalid drawing resolution, palette, pen size or blur")
    plan = make_plan(args.source, args.width, args.colors, args.pixel_size, args.blur)
    Path(args.output).write_text(json.dumps(plan), encoding="utf-8")
    print(json.dumps({key: plan[key] for key in ("grid", "canvas", "pen_size", "stroke_count")}))
