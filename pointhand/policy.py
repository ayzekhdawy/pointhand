"""Optional per-app allow-list. Empty means every app is allowed."""

from __future__ import annotations

import os
import ntpath


def _tokens(allow_csv: str) -> set[str]:
    tokens: set[str] = set()
    for part in allow_csv.split(","):
        item = part.strip().lower().replace("/", "\\")
        if not item:
            continue
        base = ntpath.basename(item)
        tokens.add(item)
        if "\\" not in item:
            tokens.update((base, ntpath.splitext(base)[0]))
    return tokens


def app_allowed(image: str, allow_csv: str) -> bool:
    """True when `image` (a path, exe name, or token) is inside the allow-list.

    An empty list allows everything. Entries match the full string, the file
    name, or the file name without an extension, so `notepad.exe` also permits
    a launch token of `notepad`.
    """
    allowed = _tokens(allow_csv)
    if not allowed:
        return True
    text = (image or "").strip().lower().replace("/", "\\")
    base = ntpath.basename(text)
    stem = ntpath.splitext(base)[0]
    return text in allowed or base in allowed or stem in allowed


def enforced() -> bool:
    return bool(_tokens(os.environ.get("POINTHAND_ALLOW", "")))


def ensure(image: str) -> None:
    allow = os.environ.get("POINTHAND_ALLOW", "")
    if not app_allowed(image, allow):
        raise PermissionError(
            f"{image or 'unknown app'} is outside POINTHAND_ALLOW ({allow})"
        )
