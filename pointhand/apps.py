"""Running apps, Start Menu launch targets, and window records.

An app id is the executable path for a running process, or the .lnk path for
a Start Menu shortcut. Window ids are the native window handles.
"""

from __future__ import annotations

import os
from typing import Any

from . import native
from .policy import ensure

_START_LIMIT = 200


def window_record(info: native.WindowInfo, app_id: str | None = None) -> dict[str, Any]:
    app = app_id if app_id is not None else (native.process_image(info.pid) or f"pid:{info.pid}")
    return {
        "id": info.hwnd,
        "app": app,
        "title": info.title,
        "pid": info.pid,
        "rect": [info.left, info.top, info.right, info.bottom],
    }


def _display_name(app_id: str, fallback: str) -> str:
    stem = os.path.splitext(os.path.basename(app_id))[0]
    return stem or fallback or app_id


def list_apps(query: str = "", include_start_menu: bool = False) -> list[dict[str, Any]]:
    """Group visible windows by executable. Optionally add Start Menu shortcuts."""
    groups: dict[str, dict[str, Any]] = {}
    for info in native.list_windows():
        app_id = native.process_image(info.pid) or f"pid:{info.pid}"
        group = groups.setdefault(app_id, {
            "id": app_id,
            "displayName": _display_name(app_id, info.title),
            "isRunning": True,
            "windows": [],
        })
        group["windows"].append(window_record(info, app_id))
    apps = list(groups.values())
    if include_start_menu:
        apps.extend(_start_menu(query))
    needle = query.strip().lower()
    if needle:
        apps = [app for app in apps if _matches(app, needle)]
    return apps


def _matches(app: dict[str, Any], needle: str) -> bool:
    if needle in str(app.get("displayName", "")).lower() or needle in str(app.get("id", "")).lower():
        return True
    return any(needle in str(win.get("title", "")).lower() for win in app.get("windows", []))


def _start_dirs() -> list[str]:
    roots = []
    appdata = os.environ.get("APPDATA", "")
    programdata = os.environ.get("ProgramData", "")
    if appdata:
        roots.append(os.path.join(appdata, "Microsoft", "Windows", "Start Menu", "Programs"))
    if programdata:
        roots.append(os.path.join(programdata, "Microsoft", "Windows", "Start Menu", "Programs"))
    return [path for path in roots if os.path.isdir(path)]


def _start_menu(query: str) -> list[dict[str, Any]]:
    needle = query.strip().lower()
    found: list[dict[str, Any]] = []
    for root in _start_dirs():
        for dirpath, _dirs, files in os.walk(root):
            for name in files:
                if not name.lower().endswith(".lnk"):
                    continue
                path = os.path.join(dirpath, name)
                display = os.path.splitext(name)[0]
                if needle and needle not in display.lower() and needle not in path.lower():
                    continue
                found.append({
                    "id": path,
                    "displayName": display,
                    "isRunning": False,
                    "windows": [],
                })
                if len(found) >= _START_LIMIT:
                    return found
    return found


def resolve_launch_target(app: str) -> str:
    """Pick a concrete path or command for launch_app."""
    text = app.strip().strip('"')
    if not text:
        raise ValueError("launch_app needs an app id or name")
    if os.path.exists(text):
        return text
    needle = text.lower()
    for candidate in list_apps(query=text, include_start_menu=True):
        display = str(candidate["displayName"]).lower()
        app_id = str(candidate["id"])
        if needle in {display, os.path.basename(app_id).lower(), os.path.splitext(os.path.basename(app_id))[0].lower()}:
            return app_id
    return text


def launch_app(app: str) -> str:
    target = resolve_launch_target(app)
    ensure(target)
    native.shell_open(target)
    return target


def get_window(window_id: int) -> dict[str, Any]:
    info = native.describe_hwnd(int(window_id))
    return window_record(info)
