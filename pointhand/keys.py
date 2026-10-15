"""Pure helpers: key-combo parsing and coordinate normalisation (no Windows calls)."""

from __future__ import annotations

from typing import NamedTuple

MODIFIERS = {
    "ctrl": 0x11, "control": 0x11, "control_l": 0x11, "control_r": 0x11, "ctrl_l": 0x11, "ctrl_r": 0x11,
    "shift": 0x10, "shift_l": 0x10, "shift_r": 0x10,
    "alt": 0x12, "alt_l": 0x12, "alt_r": 0x12,
    "win": 0x5B, "meta": 0x5B, "cmd": 0x5B, "super": 0x5B, "super_l": 0x5B, "super_r": 0x5C,
}

NAMED_KEYS = {
    "enter": 0x0D, "return": 0x0D, "tab": 0x09, "esc": 0x1B, "escape": 0x1B,
    "backspace": 0x08, "delete": 0x2E, "del": 0x2E, "space": 0x20, "insert": 0x2D,
    "up": 0x26, "down": 0x28, "left": 0x25, "right": 0x27,
    "home": 0x24, "end": 0x23, "pageup": 0x21, "pagedown": 0x22, "page_up": 0x21, "page_down": 0x22,
    "capslock": 0x14, "printscreen": 0x2C,
    "slash": 0xBF, "question": 0xBF, "less": 0xBC,
    "semicolon": 0xBA, "colon": 0xBA, "apostrophe": 0xDE, "quotedbl": 0xDE,
    "bracketleft": 0xDB, "braceleft": 0xDB, "bracketright": 0xDD, "braceright": 0xDD,
    "backslash": 0xDC, "bar": 0xDC, "grave": 0xC0, "asciitilde": 0xC0,
    **{f"{prefix}_{name}": code for prefix in ("kp", "numpad") for name, code in
       (("add", 0x6B), ("subtract", 0x6D), ("multiply", 0x6A), ("divide", 0x6F), ("decimal", 0x6E))},
    "period": 0xBE, "greater": 0xBE, "comma": 0xBC, "minus": 0xBD, "equal": 0xBB, "plus": 0xBB,
    **{f"f{i}": 0x6F + i for i in range(1, 13)},
    **{f"kp_{i}": 0x60 + i for i in range(10)},
    **{f"numpad_{i}": 0x60 + i for i in range(10)},
}

VK_ALT = 0x12


class Combo(NamedTuple):
    modifiers: tuple[int, ...]
    key: int


def parse_combo(text: str) -> Combo:
    """Parse 'ctrl+shift+s' / 'enter' / 'alt+f4' into virtual-key codes."""
    if text.strip().endswith("+") and text.strip() != "+":
        raise ValueError(f"dangling '+' in key combo: {text!r}")
    parts = [p.strip().lower() for p in text.split("+")]
    if any(not p for p in parts):
        raise ValueError("empty key combo")
    *mods, last = parts
    modifier_codes: list[int] = []
    for mod in mods:
        if mod not in MODIFIERS:
            raise ValueError(f"unknown modifier: {mod!r}")
        if MODIFIERS[mod] not in modifier_codes:
            modifier_codes.append(MODIFIERS[mod])
    if last in MODIFIERS and not mods:
        return Combo((), MODIFIERS[last])
    if last in NAMED_KEYS:
        return Combo(tuple(modifier_codes), NAMED_KEYS[last])
    if len(last) == 1 and last.isalnum() and last.isascii():
        return Combo(tuple(modifier_codes), ord(last.upper()))
    raise ValueError(f"unknown key: {last!r}")


def normalise_abs(x: int, y: int, origin: tuple[int, int], size: tuple[int, int]) -> tuple[int, int]:
    """Map virtual-desktop pixels to SendInput's 0..65535 absolute space."""
    ox, oy = origin
    width, height = size
    if width < 2 or height < 2:
        raise ValueError("virtual screen too small")
    nx = round((x - ox) * 65535 / (width - 1))
    ny = round((y - oy) * 65535 / (height - 1))
    return max(0, min(65535, nx)), max(0, min(65535, ny))


def interpolate_path(points: list[tuple[int, int]], step_px: int = 8) -> list[tuple[int, int]]:
    """Densify a polyline so a drawn stroke has a point at most step_px apart."""
    if len(points) < 2:
        raise ValueError("a path needs at least two points")
    if step_px < 1:
        raise ValueError("step_px must be >= 1")
    dense: list[tuple[int, int]] = [points[0]]
    for (x1, y1), (x2, y2) in zip(points, points[1:]):
        distance = max(abs(x2 - x1), abs(y2 - y1))
        segments = max(1, -(-distance // step_px))
        for i in range(1, segments + 1):
            dense.append((x1 + (x2 - x1) * i // segments, y1 + (y2 - y1) * i // segments))
    return dense


def window_to_screen(x: int, y: int, left: int, top: int) -> tuple[int, int]:
    """Map a point inside a window screenshot to virtual-desktop pixels."""
    return left + x, top + y


def rect_center(left: int, top: int, right: int, bottom: int) -> tuple[int, int]:
    if right <= left or bottom <= top:
        raise ValueError("element rectangle is empty")
    return (left + right) // 2, (top + bottom) // 2


def wheel_ticks(delta_px: int) -> int:
    """Convert a pixel scroll delta into whole wheel notches (at least one if nonzero)."""
    if delta_px == 0:
        return 0
    ticks = int(round(delta_px / 120))
    return ticks or (1 if delta_px > 0 else -1)


def clamp_region(region: list[int], origin: tuple[int, int], size: tuple[int, int]) -> tuple[int, int, int, int]:
    """Clamp [l, t, r, b] to the virtual screen and validate it is non-empty."""
    if len(region) != 4:
        raise ValueError("region must be [left, top, right, bottom]")
    ox, oy = origin
    width, height = size
    left = max(ox, min(region[0], ox + width))
    top = max(oy, min(region[1], oy + height))
    right = max(ox, min(region[2], ox + width))
    bottom = max(oy, min(region[3], oy + height))
    if right <= left or bottom <= top:
        raise ValueError("region is empty after clamping to the screen")
    return left, top, right, bottom


# Brace syntax kept for the alias table. Input itself uses virtual keys.
BRACE_NAMES = {190: '.', 188: ',', 189: '-', 187: '=', 17: '{Ctrl}', 16: '{Shift}', 18: '{Alt}', 91: '{Win}', 92: '{RWin}', 20: '{CapsLock}', 44: '{PrintScreen}', 191: '/', 186: ';', 222: "'", 219: '{', 221: '}', 220: '\\', 192: '`', 106: '{NumpadMult}', 107: '{NumpadAdd}', 109: '{NumpadSub}', 110: '{NumpadDot}', 111: '{NumpadDiv}', 13: '{Enter}', 9: '{Tab}', 27: '{Escape}', 8: '{Backspace}', 46: '{Delete}', 32: ' ', 45: '{Insert}', 38: '{Up}', 40: '{Down}', 37: '{Left}', 39: '{Right}', 36: '{Home}', 35: '{End}', 33: '{PageUp}', 34: '{PageDown}', 65: 'a', 66: 'b', 67: 'c', 68: 'd', 69: 'e', 70: 'f', 71: 'g', 72: 'h', 73: 'i', 74: 'j', 75: 'k', 76: 'l', 77: 'm', 78: 'n', 79: 'o', 80: 'p', 81: 'q', 82: 'r', 83: 's', 84: 't', 85: 'u', 86: 'v', 87: 'w', 88: 'x', 89: 'y', 48: '0', 49: '1', 50: '2', 51: '3', 52: '4', 53: '5', 54: '6', 55: '7', 56: '8', 57: '9', 90: 'z', 96: '{Numpad0}', 97: '{Numpad1}', 98: '{Numpad2}', 99: '{Numpad3}', 100: '{Numpad4}', 101: '{Numpad5}', 102: '{Numpad6}', 103: '{Numpad7}', 104: '{Numpad8}', 105: '{Numpad9}', 112: '{F1}', 113: '{F2}', 114: '{F3}', 115: '{F4}', 116: '{F5}', 117: '{F6}', 118: '{F7}', 119: '{F8}', 120: '{F9}', 121: '{F10}', 122: '{F11}', 123: '{F12}'}

def brace_chord(combo: Combo) -> str:
    return "".join(BRACE_NAMES.get(code, "{Unknown}") for code in (*combo.modifiers, combo.key))
