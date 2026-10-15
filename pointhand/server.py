"""pointhand MCP server (stdio).

Design goals: few round trips (batched actions, no screenshot unless asked),
no interleaving between several controllers (shared mutex), and no hard
failure when the human touches the mouse (wait, then continue or report).
"""

from __future__ import annotations

import logging
import time
from typing import Any, Callable

from mcp.server.fastmcp import FastMCP, Image

from . import __version__, apps, clipboard, guard, native, overlay, policy, screen, uia
from .a11y import canonical_action
from .keys import rect_center, wheel_ticks, window_to_screen

logging.basicConfig(level=logging.WARNING)
log = logging.getLogger("pointhand")

mcp = FastMCP("pointhand")

MAX_BATCH = 50
MAX_WAIT_S = 10.0


def _int(params: dict[str, Any], key: str) -> int:
    if key not in params:
        raise ValueError(f"missing parameter: {key}")
    return int(params[key])


def _window_origin(p: dict[str, Any]) -> tuple[int, int] | None:
    window_id = int(p.get("window_id") or 0)
    if not window_id:
        return None
    info = native.describe_hwnd(window_id)
    return info.left, info.top


def _screen_point(p: dict[str, Any], x_key: str = "x", y_key: str = "y") -> tuple[int, int]:
    """Screen point from an element index, a window-relative point, or absolute pixels."""
    element_index = int(p.get("element_index", -1))
    if element_index >= 0:
        return rect_center(*uia.element_rect(element_index))
    x, y = _int(p, x_key), _int(p, y_key)
    origin = _window_origin(p)
    if origin is None:
        return x, y
    return window_to_screen(x, y, origin[0], origin[1])


def _do_click(p: dict[str, Any]) -> str:
    x, y = _screen_point(p)
    native.click(x, y, str(p.get("button", p.get("mouse_button", "left"))), int(p.get("count", p.get("click_count", 1))))
    return "clicked"


def _do_move(p: dict[str, Any]) -> str:
    x, y = _screen_point(p)
    native.move(x, y)
    return "moved"


def _do_drag(p: dict[str, Any]) -> str:
    start = {"window_id": p.get("window_id") or 0, "x": p.get("x1", p.get("from_x")), "y": p.get("y1", p.get("from_y"))}
    end = {"window_id": p.get("window_id") or 0, "x": p.get("x2", p.get("to_x")), "y": p.get("y2", p.get("to_y"))}
    x1, y1 = _screen_point(start)
    x2, y2 = _screen_point(end)
    native.drag(x1, y1, x2, y2)
    return "dragged"


def _do_draw(p: dict[str, Any]) -> str:
    raw = p.get("points")
    if not isinstance(raw, list):
        raise ValueError("draw needs points: [[x, y], ...]")
    points = [(int(pt[0]), int(pt[1])) for pt in raw]
    drawn = native.draw_path(points, int(p.get("step_px", 8)))
    return f"drew stroke through {len(points)} points ({drawn} samples)"


def _do_scroll(p: dict[str, Any]) -> str:
    x, y = _screen_point(p)
    if "scrollY" in p or "scroll_y" in p or "scrollX" in p or "scroll_x" in p:
        # Sky-style deltas: positive scrollY moves content down, positive scrollX moves it right.
        vertical = -wheel_ticks(int(p.get("scrollY", p.get("scroll_y", 0)) or 0))
        horizontal = wheel_ticks(int(p.get("scrollX", p.get("scroll_x", 0)) or 0))
        if vertical:
            native.scroll(x, y, vertical, False)
        if horizontal:
            native.scroll(x, y, horizontal, True)
        if not vertical and not horizontal:
            raise ValueError("scroll delta is zero")
        return "scrolled"
    native.scroll(x, y, _int(p, "ticks"), bool(p.get("horizontal", False)))
    return "scrolled"


def _do_set_value(p: dict[str, Any]) -> str:
    uia.set_value(_int(p, "element_index"), str(p.get("value", "")))
    return "value set"


def _do_secondary(p: dict[str, Any]) -> str:
    action = str(p.get("secondary") or p.get("name") or "")
    canonical_action(action)  # validate before touching the control
    uia.secondary_action(_int(p, "element_index"), action)
    return f"ran {action}"


def _do_paste(p: dict[str, Any]) -> str:
    text = str(p.get("text", ""))
    window_id = int(p.get("window_id") or 0)
    if window_id:
        native.focus_hwnd(window_id)
    clipboard.write_text(text)
    native.press("ctrl+v")
    return f"pasted {len(text)} chars"


def _do_type(p: dict[str, Any]) -> str:
    text = str(p.get("text", ""))
    native.type_text(text)
    return f"typed {len(text)} chars"


def _do_key(p: dict[str, Any]) -> str:
    native.press(str(p.get("keys", "")))
    return "pressed"


def _do_wait(p: dict[str, Any]) -> str:
    seconds = min(max(float(p.get("seconds", 0.5)), 0.0), MAX_WAIT_S)
    deadline = time.monotonic() + seconds
    while (remaining := deadline - time.monotonic()) > 0:
        overlay.signal_activity()  # keep the frame up during long waits
        time.sleep(min(0.5, remaining))
    return f"waited {seconds:.2f}s"


def _action_position(action: dict[str, Any]) -> tuple[int | None, int | None]:
    """Where the agent's cursor should appear for this action, if it has one."""
    try:
        if int(action.get("element_index", -1)) >= 0:
            return rect_center(*uia.element_rect(int(action["element_index"])))
        if "points" in action and action["points"]:
            last = action["points"][-1]
            return int(last[0]), int(last[1])
        if "x2" in action or "to_x" in action:
            return _screen_point(action, "x2" if "x2" in action else "to_x", "y2" if "y2" in action else "to_y")
        if "x" in action or "from_x" in action:
            return _screen_point(action, "x" if "x" in action else "from_x", "y" if "y" in action else "from_y")
    except (TypeError, ValueError, IndexError, KeyError, LookupError):
        pass
    return None, None


def _ensure_action_allowed(action: dict[str, Any]) -> None:
    if not policy.enforced():
        return
    name = str(action.get("action", ""))
    if int(action.get("element_index", -1)) >= 0:
        hwnd = uia.cached_hwnd()
        if not hwnd:
            raise LookupError("no accessibility snapshot yet")
        policy.ensure(native.process_image(native.describe_hwnd(hwnd).pid))
        return
    window_id = int(action.get("window_id") or 0)
    if window_id:
        policy.ensure(native.process_image(native.describe_hwnd(window_id).pid))
        return
    if name == "focus_window":
        matches = native.list_windows(str(action.get("title", "")))
        if matches:
            policy.ensure(native.process_image(matches[0].pid))
        return
    if name == "draw" and action.get("points"):
        point = action["points"][0]
        policy.ensure(native.image_at(int(point[0]), int(point[1])))
        return
    if name in {"click", "double_click", "right_click", "move", "drag", "scroll"} and "x" in action:
        policy.ensure(native.image_at(_int(action, "x"), _int(action, "y")))
        return
    if name in {"type", "key", "paste"}:
        policy.ensure(native.foreground_image())


CLICKING = {"click", "double_click", "right_click"}


def _do_focus(p: dict[str, Any]) -> str:
    target = native.focus_window(str(p.get("title", "")))
    return f"focused {target.title!r}"


ACTIONS: dict[str, Callable[[dict[str, Any]], str]] = {
    "click": _do_click,
    "double_click": lambda p: _do_click({**p, "count": 2}),
    "right_click": lambda p: _do_click({**p, "button": "right"}),
    "move": _do_move,
    "drag": _do_drag,
    "draw": _do_draw,
    "scroll": _do_scroll,
    "type": _do_type,
    "key": _do_key,
    "wait": _do_wait,
    "focus_window": _do_focus,
    "set_value": _do_set_value,
    "secondary": _do_secondary,
    "paste": _do_paste,
}


def run_actions(actions: list[dict[str, Any]]) -> list[str]:
    """Run actions in order under the shared lock; stop at the first failure."""
    if not actions:
        return ["error: no actions given"]
    if len(actions) > MAX_BATCH:
        return [f"error: at most {MAX_BATCH} actions per batch"]
    results: list[str] = []
    lock = guard.ActionLock()
    try:
        with lock:
            if not guard.wait_for_idle():
                return ["error: user_active (you are using the mouse/keyboard; nothing was done)"]
            for index, action in enumerate(actions):
                name = str(action.get("action", ""))
                handler = ACTIONS.get(name)
                if handler is None:
                    results.append(f"{index}: error: unknown action {name!r}")
                    break
                px, py = _action_position(action)
                overlay.signal_activity(px, py, clicked=name in CLICKING)
                try:
                    _ensure_action_allowed(action)
                    results.append(f"{index}: {handler(action)}")
                except (ValueError, OSError, LookupError, PermissionError) as exc:
                    results.append(f"{index}: error: {exc}")
                    break
    except guard.LockTimeout as exc:
        results.append(f"error: {exc}")
    finally:
        lock.close()
    return results


def _summary(results: list[str]) -> str:
    return "\n".join(results)


@mcp.tool()
def screen_info() -> str:
    """Virtual screen origin/size and the current cursor position (full-resolution pixels)."""
    origin, size = native.virtual_screen()
    cx, cy = native.cursor_position()
    return f"origin={origin} size={size} cursor=({cx},{cy}) pointhand={__version__}"


@mcp.tool()
def screenshot(scale: float = 0.5, region: list[int] | None = None) -> list[Any]:
    """Capture the desktop. All coordinates used by other tools are full-resolution.

    scale: 0.05-1.0 (default 0.5). region: [left, top, right, bottom] in full-resolution pixels.
    The reply states the scale applied: multiply image pixels by 1/scale (plus the
    region's left/top) to get click coordinates.
    """
    data, effective = screen.capture_png(region, scale)
    left, top = (region[0], region[1]) if region else native.virtual_screen()[0]
    note = f"scale={effective:.3f}; image pixel (px,py) -> screen ({left}+px/{effective:.3f}, {top}+py/{effective:.3f})"
    return [note, Image(data=data, format="png")]


@mcp.tool()
def list_windows(title_contains: str = "") -> str:
    """List visible top-level windows (title, pid, rectangle)."""
    rows = native.list_windows(title_contains)
    return "\n".join(f"{w.title!r} pid={w.pid} rect=({w.left},{w.top},{w.right},{w.bottom})" for w in rows) or "none"


@mcp.tool()
def batch(actions: list[dict[str, Any]], screenshot_after: bool = False, scale: float = 0.5) -> list[Any]:
    """Run several actions in one round trip, stopping at the first error.

    Each action is an object with "action" plus its parameters:
      click/double_click/right_click {x, y, [button, count]}   move {x, y}
      drag {x1, y1, x2, y2}   draw {points: [[x, y], ...], [step_px]}
      scroll {x, y, ticks, [horizontal]}   type {text}
      key {keys: "ctrl+shift+s"}   wait {seconds}   focus_window {title}
      set_value {element_index, value}   secondary {element_index, secondary: "Expand"}
      paste {text, [window_id]}
    Coordinates are full-resolution screen pixels. Add window_id to treat x/y
    (or from_x/from_y/to_x/to_y, or scrollX/scrollY) as window-screenshot pixels.
    element_index refers to the latest get_window_state tree.
    Set screenshot_after=true to receive a screenshot of the final state.
    """
    results = run_actions(actions)
    reply: list[Any] = [_summary(results)]
    if screenshot_after and not any("error" in line for line in results):
        data, effective = screen.capture_png(None, scale)
        reply += [f"scale={effective:.3f}", Image(data=data, format="png")]
    return reply


@mcp.tool()
def click(
    x: int | None = None,
    y: int | None = None,
    button: str = "left",
    count: int = 1,
    window_id: int = 0,
    element_index: int = -1,
) -> str:
    """Click a screen point, a window-relative point, or an accessibility element.

    button: left/right/middle. count: 1 or 2.
    With window_id, x and y are pixels inside that window's screenshot.
    With element_index, the click hits the centre of that element from the
    latest get_window_state snapshot. Refresh the snapshot after the UI changes.
    """
    action: dict[str, Any] = {"action": "click", "button": button, "count": count, "element_index": element_index}
    if window_id:
        action["window_id"] = window_id
    if x is not None:
        action["x"] = x
    if y is not None:
        action["y"] = y
    return _summary(run_actions([action]))


@mcp.tool()
def type_text(text: str) -> str:
    """Type text into the focused control (Unicode-safe; newline presses Enter)."""
    return _summary(run_actions([{"action": "type", "text": text}]))


@mcp.tool()
def press_key(keys: str) -> str:
    """Press a key or chord, e.g. 'enter', 'ctrl+a', 'alt+f4'."""
    return _summary(run_actions([{"action": "key", "keys": keys}]))


@mcp.tool()
def focus_window(title_contains: str) -> str:
    """Bring the first visible window whose title contains the text to the foreground."""
    return _summary(run_actions([{"action": "focus_window", "title": title_contains}]))


def _json(payload: Any) -> str:
    import json
    return json.dumps(payload, ensure_ascii=False)


@mcp.tool()
def list_apps(query: str = "", include_start_menu: bool = False) -> str:
    """List running apps and the windows they own.

    Each app has an id (executable path), displayName, isRunning, and windows.
    A window id is the native handle to pass to get_window_state and activate_window.
    Set include_start_menu to also return Start Menu shortcuts that can be launched.
    query matches the app name, path, or window title.
    """
    return _json(apps.list_apps(query, include_start_menu))


@mcp.tool()
def launch_app(app: str) -> str:
    """Launch an app by id from list_apps, a Start Menu name, a path, or a command like notepad."""
    try:
        opened = apps.launch_app(app)
    except (OSError, ValueError, PermissionError) as exc:
        return f"error: {exc}"
    return f"launched {opened}"


@mcp.tool()
def get_window(window_id: int) -> str:
    """Re-read one open window by the id returned from list_apps or list_windows."""
    try:
        return _json(apps.get_window(window_id))
    except LookupError as exc:
        return f"error: {exc}"


@mcp.tool()
def activate_window(window_id: int) -> str:
    """Bring an open window to the foreground. Input tools also focus their target window."""
    return _summary(run_actions([{"action": "focus_window", "title": ""}]) if False else _activate(window_id))


def _activate(window_id: int) -> list[str]:
    action = {"action": "focus_window", "window_id": window_id, "title": ""}
    # focus_window by title is the batch action; activation by id is direct.
    results: list[str] = []
    lock = guard.ActionLock()
    try:
        with lock:
            if not guard.wait_for_idle():
                return ["error: user_active (you are using the mouse/keyboard; nothing was done)"]
            try:
                _ensure_action_allowed(action)
                info = native.focus_hwnd(window_id)
                results.append(f"0: focused {info.title!r}")
            except (ValueError, OSError, LookupError, PermissionError) as exc:
                results.append(f"error: {exc}")
    except guard.LockTimeout as exc:
        results.append(f"error: {exc}")
    finally:
        lock.close()
    return results


@mcp.tool()
def get_window_state(
    window_id: int,
    include_screenshot: bool = True,
    include_text: bool = False,
    scale: float = 0.5,
) -> list[Any]:
    """Capture one window: its record, optional accessibility tree, optional screenshot.

    Call this before element_index actions. Indexes belong to this snapshot only.
    Screenshot coordinates are full-resolution pixels of the window rectangle
    (origin at the window's top-left on screen). The scale note maps image pixels
    back to those window pixels. The shot is what is visible on the desktop at
    that rectangle; activate the window first when another window covers it.
    """
    try:
        info = native.describe_hwnd(window_id)
        if policy.enforced():
            policy.ensure(native.process_image(info.pid))
    except (LookupError, PermissionError, OSError) as exc:
        return [f"error: {exc}"]
    record = apps.window_record(info)
    lines = [f"window={_json(record)}"]
    if include_text:
        try:
            lines.append("accessibility:\n" + uia.snapshot(info.hwnd))
        except (LookupError, OSError, COMError) as exc:
            lines.append(f"accessibility_error: {exc}")
    reply: list[Any] = ["\n".join(lines)]
    if include_screenshot:
        region = [info.left, info.top, info.right, info.bottom]
        try:
            data, effective = screen.capture_png(region, scale)
        except (ValueError, OSError) as exc:
            reply.append(f"screenshot_error: {exc}")
            return reply
        reply.append(
            f"scale={effective:.3f}; window pixel (px,py) -> screen "
            f"({info.left}+px/{effective:.3f}, {info.top}+py/{effective:.3f})"
        )
        reply.append(Image(data=data, format="png"))
    return reply


# Imported for the except clause above without exposing a hard dependency at module import
# time to type checkers that run off Windows. comtypes is a runtime dependency.
try:
    from comtypes import COMError
except ImportError:  # pragma: no cover
    COMError = OSError  # type: ignore[misc, assignment]


@mcp.tool()
def set_value(window_id: int, element_index: int, value: str) -> str:
    """Replace the text of an editable element from the latest get_window_state tree."""
    return _summary(run_actions([{
        "action": "set_value", "window_id": window_id, "element_index": element_index, "value": value,
    }]))


@mcp.tool()
def perform_secondary_action(window_id: int, element_index: int, action: str) -> str:
    """Run a secondary action advertised on an element: Invoke, Expand, Collapse, Raise, or Scroll Up/Down/Left/Right."""
    return _summary(run_actions([{
        "action": "secondary", "window_id": window_id, "element_index": element_index, "secondary": action,
    }]))


@mcp.tool()
def paste(text: str, window_id: int = 0) -> str:
    """Put text on the clipboard and press Ctrl+V. Focuses window_id when given.

    This replaces the current clipboard text and leaves the pasted text there.
    """
    action: dict[str, Any] = {"action": "paste", "text": text}
    if window_id:
        action["window_id"] = window_id
    return _summary(run_actions([action]))


@mcp.tool()
def clipboard_read() -> str:
    """Read Unicode text from the clipboard."""
    try:
        return clipboard.read_text()
    except OSError as exc:
        return f"error: {exc}"


@mcp.tool()
def clipboard_write(text: str) -> str:
    """Replace the clipboard with Unicode text."""
    try:
        clipboard.write_text(text)
    except OSError as exc:
        return f"error: {exc}"
    return f"wrote {len(text)} chars"


def main() -> None:
    native.enable_dpi_awareness()
    mcp.run()


if __name__ == "__main__":
    main()
