"""Pure helpers: key aliases, accessibility text, and the app allow-list."""

import pytest

from pointhand.a11y import canonical_action, format_accessibility
from pointhand.keys import parse_combo, rect_center, wheel_ticks, window_to_screen
from pointhand.policy import app_allowed, ensure, enforced


@pytest.mark.parametrize(
    ("text", "modifiers", "key"),
    [
        ("Return", (), 0x0D),
        ("Control_L+a", (0x11,), ord("A")),
        ("ctrl_r+shift_l+alt_r+super+Tab", (0x11, 0x10, 0x12, 0x5B), 0x09),
        ("KP_0", (), 0x60),
        ("numpad_9", (), 0x69),
        ("page_up", (), 0x21),
        ("page_down", (), 0x22),
        ("period", (), 0xBE),
        ("plus", (), 0xBB),
    ],
)
def test_key_aliases(text, modifiers, key):
    combo = parse_combo(text)
    assert combo.modifiers == modifiers
    assert combo.key == key


def test_window_point_and_element_center():
    assert window_to_screen(12, 40, 100, 200) == (112, 240)
    assert rect_center(10, 20, 30, 50) == (20, 35)
    with pytest.raises(ValueError):
        rect_center(10, 20, 10, 40)


def test_wheel_ticks_keep_direction():
    assert wheel_ticks(0) == 0
    assert wheel_ticks(120) == 1
    assert wheel_ticks(-240) == -2
    assert wheel_ticks(10) == 1
    assert wheel_ticks(-10) == -1


def test_canonical_actions_ignore_case_and_underscores():
    assert canonical_action("Scroll_Up") == "scroll_up"
    assert canonical_action("  Expand ") == "expand"
    with pytest.raises(ValueError):
        canonical_action("double click")


def test_accessibility_tree_keeps_indexes_and_quotes():
    text = format_accessibility([
        {"index": 0, "depth": 0, "role": "Window", "name": 'Notepad "draft"'},
        {
            "index": 3,
            "depth": 1,
            "role": "Edit",
            "name": "line",
            "value": "x" * 130,
            "focused": True,
            "editable": True,
            "enabled": False,
            "actions": ["invoke"],
        },
    ])
    lines = text.splitlines()
    assert lines[0] == "[0] Window \"Notepad 'draft'\""
    assert lines[1].startswith("  [3] Edit \"line\" value=")
    assert lines[1].endswith("focused editable disabled actions=invoke")
    assert "..." in lines[1]


def test_allow_list_matches_path_name_or_stem(monkeypatch):
    assert app_allowed(r"C:\Windows\notepad.exe", "") is True
    allow = r"C:\Windows\System32\notepad.exe, calc, notepad.exe"
    assert app_allowed(r"C:\Windows\System32\notepad.exe", allow) is True
    assert app_allowed("notepad.exe", allow) is True
    assert app_allowed("NOTEPAD", allow) is True
    assert app_allowed("calc.exe", allow) is True
    assert app_allowed(r"C:\Windows\explorer.exe", allow) is False

    monkeypatch.delenv("POINTHAND_ALLOW", raising=False)
    assert enforced() is False
    ensure("anything")

    monkeypatch.setenv("POINTHAND_ALLOW", "notepad")
    assert enforced() is True
    ensure(r"C:\Windows\notepad.exe")
    with pytest.raises(PermissionError):
        ensure(r"C:\Windows\explorer.exe")


@pytest.mark.parametrize("name, expected", [
    ("Return", "{Enter}"), ("Control_R", "{Ctrl}"), ("Shift_L", "{Shift}"),
    ("Alt_L", "{Alt}"), ("super", "{Win}"), ("KP_0", "{Numpad0}"),
    ("page_up", "{PageUp}"), ("period", "."), ("slash", "/"),
    ("KP_Add", "{NumpadAdd}"), ("capslock", "{CapsLock}"),
])
def test_brace_alias_compatibility(name, expected):
    from pointhand.keys import brace_chord
    assert brace_chord(parse_combo(name)) == expected


def test_parses_sky_style_chord():
    from pointhand.keys import brace_chord
    assert brace_chord(parse_combo("Control_L+a")) == "{Ctrl}a"
    assert brace_chord(parse_combo("Control_L+Shift_L+period")) == "{Ctrl}{Shift}."
    assert parse_combo("ctrl+Control_L+a").modifiers == (0x11,)


@pytest.mark.parametrize("bad", ["+a", "ctrl++a", "+"])
def test_empty_chord_components_rejected(bad):
    with pytest.raises(ValueError):
        parse_combo(bad)


def test_full_path_allow_entry_does_not_allow_another_binary_with_the_same_name():
    allow = r"C:\Windows\System32\notepad.exe"
    assert app_allowed(r"C:\Windows\System32\notepad.exe", allow)
    assert not app_allowed(r"C:\Unapproved\notepad.exe", allow)
    assert not app_allowed("notepad.exe", allow)
    assert app_allowed(r"C:\Unapproved\notepad.exe", "notepad")
