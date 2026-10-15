import pytest

from pointhand.keys import clamp_region, normalise_abs, parse_combo


def test_parse_simple_named_key():
    assert parse_combo("enter").key == 0x0D
    assert parse_combo("enter").modifiers == ()


def test_parse_combo_with_modifiers():
    combo = parse_combo("Ctrl+Shift+S")
    assert combo.modifiers == (0x11, 0x10)
    assert combo.key == ord("S")


def test_parse_function_key():
    assert parse_combo("alt+f4").key == 0x73


def test_lone_modifier_is_a_key():
    assert parse_combo("shift").key == 0x10


@pytest.mark.parametrize("bad", ["", "ctrl+", "hyper+a", "ctrl+nonsense"])
def test_parse_rejects_bad_input(bad):
    with pytest.raises(ValueError):
        parse_combo(bad)


def test_normalise_corners():
    assert normalise_abs(0, 0, (0, 0), (1920, 1080)) == (0, 0)
    assert normalise_abs(1919, 1079, (0, 0), (1920, 1080)) == (65535, 65535)


def test_normalise_negative_origin_multi_monitor():
    assert normalise_abs(-1920, 0, (-1920, 0), (3840, 1080)) == (0, 0)


def test_normalise_clamps_out_of_range():
    assert normalise_abs(99999, -50, (0, 0), (1920, 1080)) == (65535, 0)


def test_clamp_region_ok_and_empty():
    assert clamp_region([-10, -10, 100, 100], (0, 0), (1920, 1080)) == (0, 0, 100, 100)
    with pytest.raises(ValueError):
        clamp_region([2000, 0, 2100, 100], (0, 0), (1920, 1080))


def test_interpolate_path_densifies_and_keeps_endpoints():
    from pointhand.keys import interpolate_path

    dense = interpolate_path([(0, 0), (100, 0)], step_px=10)
    assert dense[0] == (0, 0) and dense[-1] == (100, 0)
    assert len(dense) == 11
    assert all(abs(b[0] - a[0]) <= 10 for a, b in zip(dense, dense[1:]))


def test_interpolate_path_rejects_short_or_bad_step():
    from pointhand.keys import interpolate_path

    with pytest.raises(ValueError):
        interpolate_path([(0, 0)])
    with pytest.raises(ValueError):
        interpolate_path([(0, 0), (1, 1)], step_px=0)


def test_gradient_is_seamless_and_in_palette():
    from pointhand.gradient import gradient_hex

    assert gradient_hex(0.0, 0.0) == gradient_hex(1.0, 0.0)
    assert gradient_hex(0.0, 0.0) == "#3b2fd9"
    assert gradient_hex(0.5, 0.0) == "#22d3ee"
    assert all(len(gradient_hex(i / 17, 0.3)) == 7 for i in range(18))


def test_glass_frame_is_transparent_inside_and_glows_at_edges():
    from pointhand.glass import render_frame

    frame = render_frame((640, 360), phase=0.1)
    assert frame.size == (640, 360) and frame.mode == "RGBA"
    assert frame.getpixel((320, 180))[3] == 0          # centre stays click-through clear
    assert max(frame.getpixel((x, 4))[3] for x in range(100, 540, 20)) > 100  # edge is lit


def test_pill_and_cursor_sprites_render():
    from pointhand.glass import CURSOR_PAD, render_cursor_sprite, render_pill

    pill = render_pill("hello")
    assert pill.mode == "RGBA" and pill.getpixel((pill.width // 2, pill.height // 2))[3] > 0
    assert render_cursor_sprite().getpixel((CURSOR_PAD + 2, CURSOR_PAD + 8))[3] >= 250


def test_composite_at_clips_negative_and_outside():
    from PIL import Image
    from pointhand.glass import composite_at

    base = Image.new("RGBA", (50, 50), (0, 0, 0, 0))
    sprite = Image.new("RGBA", (20, 20), (255, 0, 0, 255))
    composite_at(base, sprite, -10, -10)
    composite_at(base, sprite, 500, 500)  # fully outside: must not raise
    assert base.getpixel((5, 5)) == (255, 0, 0, 255)
    assert base.getpixel((15, 15)) == (0, 0, 0, 0)


def test_cursor_surface_does_not_render_desktop_frame(monkeypatch):
    from PIL import Image
    from pointhand import glass, overlay
    def unexpected_frame(*args, **kwargs):
        raise AssertionError("cursor update must not render the desktop frame")
    monkeypatch.setattr(glass, "render_frame", unexpected_frame)
    background = Image.new("RGBA", (160, 160))
    scene = overlay.compose_scene((160, 160), 0.1, (80, 80), [(78, 79)], None, background)
    assert scene.size == (160, 160)
    assert scene.getpixel((0, 0))[3] == 0
    assert scene.getpixel((82, 88))[3] > 200
    assert background.getbbox() is None
