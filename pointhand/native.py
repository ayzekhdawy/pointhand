"""Thin ctypes layer over user32: input injection, screen metrics, windows."""

from __future__ import annotations

import ctypes
import time
from ctypes import wintypes
from typing import NamedTuple

from .keys import VK_ALT, interpolate_path, normalise_abs, parse_combo

user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
ULONG_PTR = ctypes.c_size_t

INPUT_MOUSE, INPUT_KEYBOARD = 0, 1
EV_MOVE, EV_LDOWN, EV_LUP = 0x0001, 0x0002, 0x0004
EV_RDOWN, EV_RUP, EV_MDOWN, EV_MUP = 0x0008, 0x0010, 0x0020, 0x0040
EV_WHEEL, EV_HWHEEL, EV_VIRTUALDESK, EV_ABSOLUTE = 0x0800, 0x1000, 0x4000, 0x8000
KEYUP, UNICODE, EXTENDED = 0x0002, 0x0004, 0x0001
WHEEL_DELTA = 120
EXTENDED_VKS = {0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x2D, 0x2E}
ENTER_VK = 0x0D

BUTTONS = {
    "left": (EV_LDOWN, EV_LUP),
    "right": (EV_RDOWN, EV_RUP),
    "middle": (EV_MDOWN, EV_MUP),
}


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [("dx", wintypes.LONG), ("dy", wintypes.LONG), ("mouseData", wintypes.DWORD),
                ("dwFlags", wintypes.DWORD), ("time", wintypes.DWORD), ("dwExtraInfo", ULONG_PTR)]


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [("wVk", wintypes.WORD), ("wScan", wintypes.WORD), ("dwFlags", wintypes.DWORD),
                ("time", wintypes.DWORD), ("dwExtraInfo", ULONG_PTR)]


class _InputUnion(ctypes.Union):
    _fields_ = [("mi", MOUSEINPUT), ("ki", KEYBDINPUT)]


class INPUT(ctypes.Structure):
    _fields_ = [("type", wintypes.DWORD), ("u", _InputUnion)]


class LASTINPUTINFO(ctypes.Structure):
    _fields_ = [("cbSize", wintypes.UINT), ("dwTime", wintypes.DWORD)]


class WindowInfo(NamedTuple):
    hwnd: int
    title: str
    pid: int
    left: int
    top: int
    right: int
    bottom: int


_last_injection_tick = 0


def enable_dpi_awareness() -> None:
    """Use physical pixels everywhere so screenshot and input coordinates match."""
    try:
        ctypes.WinDLL("shcore").SetProcessDpiAwareness(2)
    except (OSError, AttributeError):
        user32.SetProcessDPIAware()


def virtual_screen() -> tuple[tuple[int, int], tuple[int, int]]:
    """Return ((origin_x, origin_y), (width, height)) of the whole desktop."""
    return (
        (user32.GetSystemMetrics(76), user32.GetSystemMetrics(77)),
        (user32.GetSystemMetrics(78), user32.GetSystemMetrics(79)),
    )


def cursor_position() -> tuple[int, int]:
    point = wintypes.POINT()
    user32.GetCursorPos(ctypes.byref(point))
    return point.x, point.y


def last_injection_tick() -> int:
    return _last_injection_tick


def last_user_input_tick() -> int:
    info = LASTINPUTINFO(cbSize=ctypes.sizeof(LASTINPUTINFO))
    user32.GetLastInputInfo(ctypes.byref(info))
    return int(info.dwTime)


def tick_now() -> int:
    return int(kernel32.GetTickCount())


def _send(inputs: list[INPUT]) -> None:
    global _last_injection_tick
    array = (INPUT * len(inputs))(*inputs)
    sent = user32.SendInput(len(inputs), array, ctypes.sizeof(INPUT))
    _last_injection_tick = tick_now()
    if sent != len(inputs):
        raise OSError(f"SendInput delivered {sent}/{len(inputs)} events (blocked by UIPI?)")


def _mouse(flags: int, x: int = 0, y: int = 0, data: int = 0) -> INPUT:
    return INPUT(INPUT_MOUSE, _InputUnion(mi=MOUSEINPUT(x, y, data & 0xFFFFFFFF, flags, 0, 0)))


def _key(vk: int = 0, scan: int = 0, flags: int = 0) -> INPUT:
    if vk in EXTENDED_VKS:
        flags |= EXTENDED
    return INPUT(INPUT_KEYBOARD, _InputUnion(ki=KEYBDINPUT(vk, scan, flags, 0, 0)))


def _abs_move(x: int, y: int) -> INPUT:
    origin, size = virtual_screen()
    nx, ny = normalise_abs(x, y, origin, size)
    return _mouse(EV_MOVE | EV_ABSOLUTE | EV_VIRTUALDESK, nx, ny)


def move(x: int, y: int) -> None:
    _send([_abs_move(x, y)])


def click(x: int, y: int, button: str = "left", count: int = 1) -> None:
    if button not in BUTTONS:
        raise ValueError(f"unknown button: {button!r}")
    down, up = BUTTONS[button]
    events = [_abs_move(x, y)]
    for _ in range(max(1, count)):
        events += [_mouse(down), _mouse(up)]
    _send(events)


def drag(x1: int, y1: int, x2: int, y2: int, steps: int = 12) -> None:
    _send([_abs_move(x1, y1), _mouse(EV_LDOWN)])
    for i in range(1, steps + 1):
        _send([_abs_move(x1 + (x2 - x1) * i // steps, y1 + (y2 - y1) * i // steps)])
        time.sleep(0.005)
    _send([_mouse(EV_LUP)])


def draw_path(points: list[tuple[int, int]], step_px: int = 8, delay_s: float = 0.004) -> int:
    """Hold the left button and trace a polyline (paint, signatures, canvases)."""
    dense = interpolate_path(points, step_px)
    first_x, first_y = dense[0]
    _send([_abs_move(first_x, first_y), _mouse(EV_LDOWN)])
    try:
        for x, y in dense[1:]:
            _send([_abs_move(x, y)])
            time.sleep(delay_s)
    finally:
        _send([_mouse(EV_LUP)])
    return len(dense)


def scroll(x: int, y: int, ticks: int, horizontal: bool = False) -> None:
    """Positive ticks scroll up/right, negative down/left."""
    flag = EV_HWHEEL if horizontal else EV_WHEEL
    _send([_abs_move(x, y), _mouse(flag, data=ticks * WHEEL_DELTA)])


def type_text(text: str) -> None:
    events: list[INPUT] = []
    for char in text.replace("\r\n", "\n"):
        if char == "\n":
            events += [_key(ENTER_VK), _key(ENTER_VK, flags=KEYUP)]
            continue
        encoded = char.encode("utf-16-le")
        for i in range(0, len(encoded), 2):
            code = int.from_bytes(encoded[i:i + 2], "little")
            events += [_key(scan=code, flags=UNICODE), _key(scan=code, flags=UNICODE | KEYUP)]
    for start in range(0, len(events), 200):
        _send(events[start:start + 200])


def press(combo_text: str) -> None:
    combo = parse_combo(combo_text)
    events = [_key(m) for m in combo.modifiers] + [_key(combo.key), _key(combo.key, flags=KEYUP)]
    events += [_key(m, flags=KEYUP) for m in reversed(combo.modifiers)]
    _send(events)


def list_windows(title_filter: str = "") -> list[WindowInfo]:
    found: list[WindowInfo] = []
    needle = title_filter.lower()
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

    def visit(hwnd: int, _lparam: int) -> bool:
        if not user32.IsWindowVisible(hwnd):
            return True
        length = user32.GetWindowTextLengthW(hwnd)
        if length == 0:
            return True
        buffer = ctypes.create_unicode_buffer(length + 1)
        user32.GetWindowTextW(hwnd, buffer, length + 1)
        title = buffer.value
        if needle and needle not in title.lower():
            return True
        rect, pid = wintypes.RECT(), wintypes.DWORD()
        user32.GetWindowRect(hwnd, ctypes.byref(rect))
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        found.append(WindowInfo(int(hwnd), title, int(pid.value), rect.left, rect.top, rect.right, rect.bottom))
        return True

    user32.EnumWindows(callback_type(visit), 0)
    return found


def focus_window(title_filter: str) -> WindowInfo:
    matches = list_windows(title_filter)
    if not matches:
        raise LookupError(f"no visible window matches {title_filter!r}")
    return focus_hwnd(matches[0].hwnd)


def describe_hwnd(hwnd: int) -> WindowInfo:
    """Read title, pid, and screen rectangle for one window handle."""
    user32.IsWindow.argtypes = [wintypes.HWND]
    user32.IsWindow.restype = wintypes.BOOL
    if not hwnd or not user32.IsWindow(hwnd):
        raise LookupError(f"window id {hwnd} is not an open window")
    length = user32.GetWindowTextLengthW(hwnd)
    buffer = ctypes.create_unicode_buffer(max(length, 0) + 1)
    user32.GetWindowTextW(hwnd, buffer, length + 1)
    rect, pid = wintypes.RECT(), wintypes.DWORD()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    return WindowInfo(int(hwnd), buffer.value, int(pid.value), rect.left, rect.top, rect.right, rect.bottom)


def focus_hwnd(hwnd: int) -> WindowInfo:
    target = describe_hwnd(hwnd)
    if user32.IsIconic(target.hwnd):
        user32.ShowWindow(target.hwnd, 9)
    # A synthetic Alt press lifts the foreground lock that would otherwise block this call.
    _send([_key(VK_ALT), _key(VK_ALT, flags=KEYUP)])
    user32.SetForegroundWindow(target.hwnd)
    return target


def process_image(pid: int) -> str:
    """Full executable path for a process, or '' when the process cannot be queried."""
    if not pid:
        return ""
    access = 0x1000  # PROCESS_QUERY_LIMITED_INFORMATION
    kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel32.OpenProcess.restype = wintypes.HANDLE
    kernel32.QueryFullProcessImageNameW.argtypes = [
        wintypes.HANDLE, wintypes.DWORD, wintypes.LPWSTR, ctypes.POINTER(wintypes.DWORD),
    ]
    kernel32.QueryFullProcessImageNameW.restype = wintypes.BOOL
    kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
    handle = kernel32.OpenProcess(access, False, pid)
    if not handle:
        return ""
    try:
        size = wintypes.DWORD(32768)
        buffer = ctypes.create_unicode_buffer(size.value)
        if not kernel32.QueryFullProcessImageNameW(handle, 0, buffer, ctypes.byref(size)):
            return ""
        return buffer.value
    finally:
        kernel32.CloseHandle(handle)


def foreground_hwnd() -> int:
    user32.GetForegroundWindow.restype = wintypes.HWND
    return int(user32.GetForegroundWindow() or 0)


def root_at(x: int, y: int) -> int:
    """Top-level window that contains a screen point."""
    user32.WindowFromPoint.argtypes = [wintypes.POINT]
    user32.WindowFromPoint.restype = wintypes.HWND
    user32.GetAncestor.argtypes = [wintypes.HWND, wintypes.UINT]
    user32.GetAncestor.restype = wintypes.HWND
    hwnd = user32.WindowFromPoint(wintypes.POINT(x, y))
    if not hwnd:
        return 0
    root = user32.GetAncestor(hwnd, 2)  # GA_ROOT
    return int(root or hwnd)


def image_at(x: int, y: int) -> str:
    hwnd = root_at(x, y)
    if not hwnd:
        return ""
    return process_image(describe_hwnd(hwnd).pid)


def foreground_image() -> str:
    hwnd = foreground_hwnd()
    if not hwnd:
        return ""
    return process_image(describe_hwnd(hwnd).pid)


def shell_open(target: str) -> None:
    """Launch a file, shortcut, or command through the shell."""
    shell32 = ctypes.WinDLL("shell32", use_last_error=True)
    shell32.ShellExecuteW.argtypes = [
        wintypes.HWND, wintypes.LPCWSTR, wintypes.LPCWSTR, wintypes.LPCWSTR, wintypes.LPCWSTR, ctypes.c_int,
    ]
    shell32.ShellExecuteW.restype = ctypes.c_void_p
    result = int(shell32.ShellExecuteW(None, "open", target, None, None, 1) or 0)
    if result <= 32:
        raise OSError(f"could not launch {target!r} (ShellExecute code {result})")
