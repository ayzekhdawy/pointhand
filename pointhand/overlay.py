"""On-screen activity layer: a glowing glass frame plus the agent's own cursor.

Run as its own process (`python -m pointhand.overlay`). Servers only touch a
stamp file; this process shows the layer while the stamp is fresh and exits
after a long idle period. The window is per-pixel-alpha, click-through, and
excluded from screen capture so it never changes what the agent sees.

Environment:
  POINTHAND_OVERLAY=0           disable the overlay entirely
  POINTHAND_OVERLAY_CAPTURE=1   keep the overlay visible to screen capture (debug)
"""

from __future__ import annotations

import ctypes
import math
import os
import subprocess
import sys
import tempfile
import threading
import queue
import time
from collections import deque
from ctypes import wintypes
from pathlib import Path

from . import glass

STAMP_PATH = Path(tempfile.gettempdir()) / "pointhand-active.stamp"
MUTEX_NAME = "Local\\pointhand-overlay"
LABEL_TEXT = "pointhand is controlling this PC  ·  touch the mouse to pause"
LINGER_S = 1.2
RIPPLE_S = 0.5
IDLE_EXIT_S = 90.0
FPS = 60
FRAME_FPS = 8
CURSOR_SURFACE_SIZE = (160, 160)
TRAIL_LENGTH = 7

_ERROR_ALREADY_EXISTS = 183
_SYNCHRONIZE = 0x00100000
_CREATE_NO_WINDOW, _DETACHED_PROCESS = 0x08000000, 0x00000008
_WS_EX_LAYERED, _WS_EX_TRANSPARENT = 0x00080000, 0x00000020
_WS_EX_TOOLWINDOW, _WS_EX_NOACTIVATE = 0x00000080, 0x08000000
_GWL_EXSTYLE, _WDA_EXCLUDEFROMCAPTURE = -20, 0x11
_ULW_ALPHA, _AC_SRC_OVER, _AC_SRC_ALPHA = 0x2, 0x0, 0x1

_click_counter = 0


def overlay_enabled() -> bool:
    return os.environ.get("POINTHAND_OVERLAY", "1") != "0"


def signal_activity(x: int | None = None, y: int | None = None, clicked: bool = False) -> None:
    """Mark the desktop as being controlled right now and make sure the overlay runs.

    x/y place the agent's own cursor; clicked=True makes it emit a click ripple.
    The stamp file holds: "<pid>:<click_counter> <x> <y>" (x/y are -1 when unknown).
    """
    global _click_counter
    if not overlay_enabled():
        return
    if clicked:
        _click_counter += 1
    px = -1 if x is None else int(x)
    py = -1 if y is None else int(y)
    try:
        STAMP_PATH.write_text(f"{os.getpid()}:{_click_counter} {px} {py}", encoding="utf-8")
    except OSError:
        return
    ensure_running()


def stamp_age_s() -> float:
    try:
        return time.time() - STAMP_PATH.stat().st_mtime
    except OSError:
        return float("inf")


def read_stamp() -> tuple[str, int, int]:
    """Return (click token, x, y); x/y are -1 when the agent has no position yet."""
    try:
        token, x, y = STAMP_PATH.read_text(encoding="utf-8").split()
        return token, int(x), int(y)
    except (OSError, ValueError):
        return "", -1, -1


def _overlay_running() -> bool:
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.OpenMutexW.restype = wintypes.HANDLE
    kernel32.OpenMutexW.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.LPCWSTR]
    handle = kernel32.OpenMutexW(_SYNCHRONIZE, False, MUTEX_NAME)
    if handle:
        kernel32.CloseHandle(handle)
        return True
    return False


def ensure_running() -> None:
    if _overlay_running():
        return
    try:
        subprocess.Popen(
            [sys.executable, "-m", "pointhand.overlay"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            creationflags=_CREATE_NO_WINDOW | _DETACHED_PROCESS,
            close_fds=True,
        )
    except OSError:
        pass  # The overlay is a courtesy; never break input because of it.


def _claim_single_instance() -> bool:
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.CreateMutexW.restype = wintypes.HANDLE
    kernel32.CreateMutexW.argtypes = [wintypes.LPVOID, wintypes.BOOL, wintypes.LPCWSTR]
    handle = kernel32.CreateMutexW(None, False, MUTEX_NAME)
    return bool(handle) and ctypes.get_last_error() != _ERROR_ALREADY_EXISTS


class _BitmapInfoHeader(ctypes.Structure):
    _fields_ = [("biSize", wintypes.DWORD), ("biWidth", wintypes.LONG), ("biHeight", wintypes.LONG),
                ("biPlanes", wintypes.WORD), ("biBitCount", wintypes.WORD), ("biCompression", wintypes.DWORD),
                ("biSizeImage", wintypes.DWORD), ("biXPelsPerMeter", wintypes.LONG),
                ("biYPelsPerMeter", wintypes.LONG), ("biClrUsed", wintypes.DWORD),
                ("biClrImportant", wintypes.DWORD)]


class _BlendFunction(ctypes.Structure):
    _fields_ = [("BlendOp", ctypes.c_ubyte), ("BlendFlags", ctypes.c_ubyte),
                ("SourceConstantAlpha", ctypes.c_ubyte), ("AlphaFormat", ctypes.c_ubyte)]


class LayeredSurface:
    """A per-pixel-alpha window surface that we update with premultiplied BGRA bytes."""

    def __init__(self, hwnd: int, origin: tuple[int, int], size: tuple[int, int]) -> None:
        self._user32 = ctypes.WinDLL("user32", use_last_error=True)
        self._gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)
        self._hwnd = wintypes.HWND(hwnd)
        self._origin, self._size = origin, size
        user32, gdi32 = self._user32, self._gdi32
        user32.GetDC.restype = wintypes.HDC
        user32.GetDC.argtypes = [wintypes.HWND]
        gdi32.CreateCompatibleDC.restype = wintypes.HDC
        gdi32.CreateCompatibleDC.argtypes = [wintypes.HDC]
        gdi32.CreateDIBSection.restype = wintypes.HBITMAP
        gdi32.CreateDIBSection.argtypes = [wintypes.HDC, ctypes.c_void_p, wintypes.UINT,
                                           ctypes.POINTER(ctypes.c_void_p), wintypes.HANDLE, wintypes.DWORD]
        gdi32.SelectObject.argtypes = [wintypes.HDC, wintypes.HGDIOBJ]
        user32.UpdateLayeredWindow.argtypes = [
            wintypes.HWND, wintypes.HDC, ctypes.POINTER(wintypes.POINT), ctypes.POINTER(wintypes.SIZE),
            wintypes.HDC, ctypes.POINTER(wintypes.POINT), wintypes.COLORREF,
            ctypes.POINTER(_BlendFunction), wintypes.DWORD]
        self._screen_dc = user32.GetDC(None)
        self._mem_dc = gdi32.CreateCompatibleDC(self._screen_dc)
        header = _BitmapInfoHeader(ctypes.sizeof(_BitmapInfoHeader), size[0], -size[1], 1, 32, 0,
                                   size[0] * size[1] * 4, 0, 0, 0, 0)
        self._bits = ctypes.c_void_p()
        self._bitmap = gdi32.CreateDIBSection(self._screen_dc, ctypes.byref(header), 0,
                                              ctypes.byref(self._bits), None, 0)
        if not self._bitmap or not self._bits.value:
            raise OSError("CreateDIBSection failed")
        gdi32.SelectObject(self._mem_dc, self._bitmap)
        self._byte_count = size[0] * size[1] * 4

    def present(self, bgra: bytes, origin: tuple[int, int] | None = None) -> None:
        if len(bgra) != self._byte_count:
            raise ValueError("frame size does not match the surface")
        ctypes.memmove(self._bits, bgra, self._byte_count)
        destination = wintypes.POINT(*(origin if origin is not None else self._origin))
        size = wintypes.SIZE(*self._size)
        source = wintypes.POINT(0, 0)
        blend = _BlendFunction(_AC_SRC_OVER, 0, 255, _AC_SRC_ALPHA)
        self._user32.UpdateLayeredWindow(self._hwnd, self._screen_dc, ctypes.byref(destination),
                                         ctypes.byref(size), self._mem_dc, ctypes.byref(source), 0,
                                         ctypes.byref(blend), _ULW_ALPHA)


def _prepare_window(root: "tk.Tk") -> int:  # noqa: F821 (tk imported lazily)
    """Make the Tk window click-through, capture-excluded and layered; return its HWND."""
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    user32.GetParent.restype = wintypes.HWND
    user32.GetParent.argtypes = [wintypes.HWND]
    user32.GetWindowLongW.argtypes = [wintypes.HWND, ctypes.c_int]
    user32.SetWindowLongW.argtypes = [wintypes.HWND, ctypes.c_int, ctypes.c_long]
    root.update_idletasks()
    hwnd = user32.GetParent(root.winfo_id()) or root.winfo_id()
    style = user32.GetWindowLongW(hwnd, _GWL_EXSTYLE) & ~_WS_EX_LAYERED
    user32.SetWindowLongW(hwnd, _GWL_EXSTYLE, style)  # clear, then set: lets UpdateLayeredWindow work
    style |= _WS_EX_LAYERED | _WS_EX_TRANSPARENT | _WS_EX_TOOLWINDOW | _WS_EX_NOACTIVATE
    user32.SetWindowLongW(hwnd, _GWL_EXSTYLE, style)
    if os.environ.get("POINTHAND_OVERLAY_CAPTURE") != "1":
        user32.SetWindowDisplayAffinity(hwnd, _WDA_EXCLUDEFROMCAPTURE)
    return int(hwnd)


def compose_scene(size: tuple[int, int], elapsed: float, cursor: tuple[float, float] | None,
                  trail: list[tuple[float, float]], ripple_progress: float | None, background=None):
    """Build one RGBA frame: glow ring, label, trail, cursor, and click ripple."""
    width, height = size
    intensity = 0.82 + 0.18 * math.sin(elapsed * 2.2)
    if background is None:
        scene = glass.render_frame(size, elapsed * 0.35, intensity)
        pill = glass.render_pill(LABEL_TEXT)
        glass.composite_at(scene, pill, (width - pill.width) // 2, 22)
    else:
        scene = background.copy()
    if cursor is None:
        return scene
    for index, (tx, ty) in enumerate(trail):
        fade = (index + 1) / (len(trail) + 1)
        dot = glass.render_dot(max(2, round(3 + 5 * fade)), round(150 * fade))
        glass.composite_at(scene, dot, round(tx) - dot.width // 2, round(ty) - dot.height // 2)
    pulse = glass.render_dot(round(14 + 4 * math.sin(elapsed * 5.0)), 70)
    glass.composite_at(scene, pulse, round(cursor[0]) - pulse.width // 2, round(cursor[1]) - pulse.height // 2)
    if ripple_progress is not None:
        ring = glass.render_ripple(ripple_progress)
        glass.composite_at(scene, ring, round(cursor[0]) - ring.width // 2, round(cursor[1]) - ring.height // 2)
    sprite = glass.render_cursor_sprite()
    glass.composite_at(scene, sprite, round(cursor[0]) - glass.CURSOR_PAD, round(cursor[1]) - glass.CURSOR_PAD)
    return scene


def run() -> int:
    if not overlay_enabled() or not _claim_single_instance():
        return 0
    import tkinter as tk  # imported here so servers never pay for Tk

    from . import native

    native.enable_dpi_awareness()
    origin, size = native.virtual_screen()
    root = tk.Tk()
    root.overrideredirect(True)
    root.attributes("-topmost", True)
    root.geometry(f"{size[0]}x{size[1]}{origin[0]:+d}{origin[1]:+d}")
    surface = LayeredSurface(_prepare_window(root), origin, size)

    # Frame generation never blocks the 60 Hz cursor loop. Queue keeps only
    # the latest rendered frame instead of accumulating stale animation.
    frames: queue.Queue[bytes] = queue.Queue(maxsize=1)
    stop_render = threading.Event()
    frame_started = time.monotonic()
    def render_frames() -> None:
        while not stop_render.is_set():
            if stamp_age_s() >= LINGER_S:
                stop_render.wait(0.05)
                continue
            started = time.monotonic()
            frame = compose_scene(size, started - frame_started, None, [], None)
            data = glass.to_premultiplied_bgra(frame)
            try:
                frames.get_nowait()
            except queue.Empty:
                pass
            frames.put_nowait(data)
            stop_render.wait(max(0.0, 1 / FRAME_FPS - (time.monotonic() - started)))
    threading.Thread(target=render_frames, daemon=True).start()
    cursor_root = tk.Toplevel(root)
    cursor_root.overrideredirect(True)
    cursor_root.attributes("-topmost", True)
    cursor_root.geometry("160x160+0+0")
    cursor_surface = LayeredSurface(_prepare_window(cursor_root), (0, 0), CURSOR_SURFACE_SIZE)
    from PIL import Image
    transparent_cursor = Image.new("RGBA", CURSOR_SURFACE_SIZE)
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    user32.GetCursorPos.argtypes = [ctypes.POINTER(wintypes.POINT)]
    user32.GetCursorPos.restype = wintypes.BOOL

    state: dict[str, object] = {"visible": True, "started": time.monotonic(), "cursor": None,
                                "token": "", "ripple_at": None}
    trail: deque[tuple[float, float]] = deque(maxlen=TRAIL_LENGTH)

    def follow(token: str, tx: int, ty: int) -> tuple[float, float] | None:
        point = wintypes.POINT()
        if user32.GetCursorPos(ctypes.byref(point)):
            tx, ty = point.x, point.y
        target = (float(tx - origin[0]), float(ty - origin[1]))
        current = target  # follow actual injected motion without an easing lag
        state["cursor"] = current
        trail.append(current)
        if state["token"] and token != state["token"]:
            state["ripple_at"] = time.monotonic()
        state["token"] = token
        return current

    def tick() -> None:
        tick_started = time.monotonic()
        age = stamp_age_s()
        if age > IDLE_EXIT_S:
            stop_render.set()
            root.destroy()
            return
        active = age < LINGER_S
        if active and not state["visible"]:
            root.deiconify()
            cursor_root.deiconify()
            root.attributes("-topmost", True)
            state["visible"] = True
        elif not active and state["visible"]:
            root.withdraw()
            cursor_root.withdraw()
            state["visible"] = False
        if state["visible"]:
            now = time.monotonic()
            cursor = follow(*read_stamp())
            ripple_progress = None
            if state["ripple_at"] is not None:
                progress = (now - state["ripple_at"]) / RIPPLE_S  # type: ignore[operator]
                if progress >= 1.0:
                    state["ripple_at"] = None
                else:
                    ripple_progress = progress
            try:
                surface.present(frames.get_nowait())
            except queue.Empty:
                pass
            if cursor is not None:
                # All cursor effects fit in a small surface; do not copy the
                # entire desktop bitmap for each pointer update.
                anchor = (80, 80)
                local_trail = [(x - cursor[0] + anchor[0], y - cursor[1] + anchor[1]) for x, y in trail]
                scene = compose_scene(CURSOR_SURFACE_SIZE, now - state["started"], anchor,
                                      local_trail, ripple_progress, transparent_cursor)
                destination = (round(cursor[0] + origin[0]) - anchor[0],
                               round(cursor[1] + origin[1]) - anchor[1])
                cursor_surface.present(glass.to_premultiplied_bgra(scene), destination)
        delay = max(1, round((1 / FPS - (time.monotonic() - tick_started)) * 1000))
        root.after(delay, tick)

    root.after(0, tick)
    root.mainloop()
    return 0


if __name__ == "__main__":
    raise SystemExit(run())
