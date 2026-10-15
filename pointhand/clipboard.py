"""Unicode clipboard read/write through user32. No extra packages."""

from __future__ import annotations

import ctypes
from ctypes import wintypes

from . import native

CF_UNICODETEXT = 13
GMEM_MOVEABLE = 0x0002


def _bind() -> None:
    user32, kernel32 = native.user32, native.kernel32
    user32.OpenClipboard.argtypes = [wintypes.HWND]
    user32.OpenClipboard.restype = wintypes.BOOL
    user32.CloseClipboard.argtypes = []
    user32.CloseClipboard.restype = wintypes.BOOL
    user32.EmptyClipboard.argtypes = []
    user32.EmptyClipboard.restype = wintypes.BOOL
    user32.GetClipboardData.argtypes = [wintypes.UINT]
    user32.GetClipboardData.restype = ctypes.c_void_p
    user32.SetClipboardData.argtypes = [wintypes.UINT, ctypes.c_void_p]
    user32.SetClipboardData.restype = ctypes.c_void_p
    kernel32.GlobalAlloc.argtypes = [wintypes.UINT, ctypes.c_size_t]
    kernel32.GlobalAlloc.restype = ctypes.c_void_p
    kernel32.GlobalLock.argtypes = [ctypes.c_void_p]
    kernel32.GlobalLock.restype = ctypes.c_void_p
    kernel32.GlobalUnlock.argtypes = [ctypes.c_void_p]
    kernel32.GlobalUnlock.restype = wintypes.BOOL
    kernel32.GlobalFree.argtypes = [ctypes.c_void_p]
    kernel32.GlobalFree.restype = ctypes.c_void_p


def read_text() -> str:
    """Return Unicode text on the clipboard, or '' when the clipboard has none."""
    _bind()
    user32, kernel32 = native.user32, native.kernel32
    if not user32.OpenClipboard(None):
        raise OSError("clipboard is busy")
    try:
        handle = user32.GetClipboardData(CF_UNICODETEXT)
        if not handle:
            return ""
        locked = kernel32.GlobalLock(handle)
        if not locked:
            raise OSError("could not read the clipboard")
        try:
            return ctypes.wstring_at(locked)
        finally:
            kernel32.GlobalUnlock(handle)
    finally:
        user32.CloseClipboard()


def write_text(text: str) -> None:
    """Replace the clipboard with Unicode text. The caller owns the previous contents."""
    _bind()
    user32, kernel32 = native.user32, native.kernel32
    raw = text.encode("utf-16-le") + b"\x00\x00"
    handle = kernel32.GlobalAlloc(GMEM_MOVEABLE, len(raw))
    if not handle:
        raise OSError("could not allocate clipboard memory")
    locked = kernel32.GlobalLock(handle)
    if not locked:
        kernel32.GlobalFree(handle)
        raise OSError("could not lock clipboard memory")
    ctypes.memmove(locked, raw, len(raw))
    kernel32.GlobalUnlock(handle)
    if not user32.OpenClipboard(None):
        kernel32.GlobalFree(handle)
        raise OSError("clipboard is busy")
    try:
        if not user32.EmptyClipboard():
            raise OSError("could not empty the clipboard")
        if not user32.SetClipboardData(CF_UNICODETEXT, handle):
            raise OSError("could not set clipboard text")
        handle = None  # the system owns it after a successful SetClipboardData
    finally:
        user32.CloseClipboard()
        if handle:
            kernel32.GlobalFree(handle)
