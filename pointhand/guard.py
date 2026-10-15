"""Cross-process action lock and user-activity guard.

Several MCP servers (one per agent session) can share one desktop. The named
mutex serialises their input bursts so they never interleave, and the guard
waits for the human to stop touching the mouse instead of failing outright.
"""

from __future__ import annotations

import time
from ctypes import wintypes

from . import native

_MUTEX_NAME = "Local\\pointhand-input-lock"
_WAIT_OBJECT_0, _WAIT_ABANDONED = 0, 0x80
_TICK_MASK = 0xFFFFFFFF
_HALF_RANGE = 0x80000000


class LockTimeout(TimeoutError):
    """Another controller held the desktop for too long."""


class ActionLock:
    def __init__(self, timeout_s: float = 15.0) -> None:
        self._timeout_ms = int(timeout_s * 1000)
        create = native.kernel32.CreateMutexW
        create.restype = wintypes.HANDLE
        create.argtypes = [wintypes.LPVOID, wintypes.BOOL, wintypes.LPCWSTR]
        self._handle = create(None, False, _MUTEX_NAME)
        if not self._handle:
            raise OSError("could not create the shared input mutex")

    def __enter__(self) -> "ActionLock":
        result = native.kernel32.WaitForSingleObject(self._handle, self._timeout_ms)
        if result not in (_WAIT_OBJECT_0, _WAIT_ABANDONED):
            raise LockTimeout("another desktop controller is busy; try again shortly")
        return self

    def __exit__(self, *_exc: object) -> None:
        native.kernel32.ReleaseMutex(self._handle)

    def close(self) -> None:
        native.kernel32.CloseHandle(self._handle)


def tick_is_newer(a: int, b: int) -> bool:
    """True when tick a is strictly later than tick b (wrap-safe)."""
    return 0 < ((a - b) & _TICK_MASK) < _HALF_RANGE


def user_is_active(quiet_ms: int = 400, tolerance_ms: int = 40) -> bool:
    """True when the human, not us, produced input within the last quiet_ms."""
    user_tick = native.last_user_input_tick()
    ours = native.last_injection_tick()
    if ours and not tick_is_newer(user_tick, (ours + tolerance_ms) & _TICK_MASK):
        return False
    return ((native.tick_now() - user_tick) & _TICK_MASK) < quiet_ms


def wait_for_idle(timeout_s: float = 8.0, quiet_ms: int = 400) -> bool:
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        if not user_is_active(quiet_ms):
            return True
        time.sleep(0.1)
    return False
