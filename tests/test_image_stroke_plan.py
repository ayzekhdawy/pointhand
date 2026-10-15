"""Reference pixels must survive conversion to independent pen strokes."""
import importlib.util
from pathlib import Path
from PIL import Image

spec = importlib.util.spec_from_file_location("image_stroke_plan", Path(__file__).parents[1] / "scripts" / "image-stroke-plan.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def test_strokes_cover_each_reference_cell_once_and_keep_its_color(tmp_path):
    source = tmp_path / "reference.png"
    image = Image.new("RGB", (3, 2))
    expected = [(0, 0, 0), (0, 0, 0), (255, 255, 255),
                (255, 255, 255), (0, 0, 0), (255, 255, 255)]
    image.putdata(expected)
    image.save(source)
    plan = module.make_plan(source, width=3, colors=2, pixel_size=2, blur=0)
    assert plan["canvas"] == [6, 4]
    reconstructed = [None] * 6
    for group in plan["colors"]:
        color = tuple(bytes.fromhex(group["hex"][1:]))
        for x1, y1, x2, y2 in group["strokes"]:
            assert y1 == y2
            row = (y1 - 1) // 2
            for column in range((x1 - 1) // 2, (x2 - 1) // 2 + 1):
                index = row * 3 + column
                assert reconstructed[index] is None
                reconstructed[index] = color
    assert reconstructed == expected
    assert sum(group["pixels"] for group in plan["colors"]) == 6


def test_flat_image_merges_to_one_stroke_per_row(tmp_path):
    source = tmp_path / "flat.png"
    Image.new("RGB", (8, 4), (20, 30, 40)).save(source)
    plan = module.make_plan(source, width=8, colors=2, pixel_size=2, blur=0)
    assert plan["stroke_count"] == 4
    assert plan["colors"][0]["hex"] == "#141E28"
    assert all(stroke[0] == 1 and stroke[2] == 15 for stroke in plan["colors"][0]["strokes"])
