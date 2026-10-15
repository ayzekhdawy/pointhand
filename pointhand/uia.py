"""UI Automation tree for one window.

Element indexes come from a depth-first control-view walk. They stay valid
only until the next snapshot. This module is a clean-room use of the public
IUIAutomation interfaces; it does not wrap any vendor computer-use binary.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from comtypes import COMError, CoCreateInstance

from .a11y import format_accessibility

MAX_NODES = 400
MAX_DEPTH = 14

_INVOKE, _VALUE, _SCROLL, _EXPAND, _SCROLL_ITEM = 10000, 10002, 10004, 10005, 10017
_NO_AMOUNT, _SMALL_DEC, _SMALL_INC = 2, 1, 3

_ROLES = {
    50000: "Button", 50001: "Calendar", 50002: "CheckBox", 50003: "ComboBox", 50004: "Edit",
    50005: "Hyperlink", 50006: "Image", 50007: "ListItem", 50008: "List", 50009: "Menu",
    50010: "MenuBar", 50011: "MenuItem", 50012: "ProgressBar", 50013: "RadioButton",
    50014: "ScrollBar", 50015: "Slider", 50016: "Spinner", 50017: "StatusBar", 50018: "Tab",
    50019: "TabItem", 50020: "Text", 50021: "ToolBar", 50022: "ToolTip", 50023: "Tree",
    50024: "TreeItem", 50025: "Custom", 50026: "Group", 50027: "Thumb", 50028: "DataGrid",
    50029: "DataItem", 50030: "Document", 50031: "SplitButton", 50032: "Window", 50033: "Pane",
    50034: "Header", 50035: "HeaderItem", 50036: "Table", 50037: "TitleBar", 50038: "Separator",
}

_client: Any = None
_cached_hwnd = 0
_nodes: list[dict[str, Any]] = []
_elements: list[Any] = []


@dataclass
class _Patterns:
    invoke: Any = None
    value: Any = None
    scroll: Any = None
    expand: Any = None
    scroll_item: Any = None


def _automation() -> Any:
    global _client
    if _client is not None:
        return _client
    import comtypes.client

    comtypes.client.GetModule("UIAutomationCore.dll")
    from comtypes.gen.UIAutomationClient import CUIAutomation, IUIAutomation

    _client = CoCreateInstance(
        CUIAutomation._reg_clsid_,
        interface=IUIAutomation,
        clsctx=comtypes.CLSCTX_INPROC_SERVER,
    )
    return _client


def _pattern(element: Any, pattern_id: int, interface: Any) -> Any:
    try:
        unknown = element.GetCurrentPattern(pattern_id)
    except COMError:
        return None
    if not unknown:
        return None
    try:
        return unknown.QueryInterface(interface)
    except COMError:
        return None


def _patterns(element: Any) -> _Patterns:
    from comtypes.gen.UIAutomationClient import (
        IUIAutomationExpandCollapsePattern,
        IUIAutomationInvokePattern,
        IUIAutomationScrollItemPattern,
        IUIAutomationScrollPattern,
        IUIAutomationValuePattern,
    )

    return _Patterns(
        invoke=_pattern(element, _INVOKE, IUIAutomationInvokePattern),
        value=_pattern(element, _VALUE, IUIAutomationValuePattern),
        scroll=_pattern(element, _SCROLL, IUIAutomationScrollPattern),
        expand=_pattern(element, _EXPAND, IUIAutomationExpandCollapsePattern),
        scroll_item=_pattern(element, _SCROLL_ITEM, IUIAutomationScrollItemPattern),
    )


def _safe(fn, default=None):
    try:
        return fn()
    except COMError:
        return default


def _describe(element: Any, index: int, depth: int) -> dict[str, Any]:
    patterns = _patterns(element)
    role_id = int(_safe(lambda: element.CurrentControlType, 0) or 0)
    rect = _safe(lambda: element.CurrentBoundingRectangle)
    value = ""
    editable = False
    if patterns.value is not None:
        value = str(_safe(lambda: patterns.value.CurrentValue, "") or "")
        editable = not bool(_safe(lambda: patterns.value.CurrentIsReadOnly, True))
    actions: list[str] = []
    if patterns.invoke is not None:
        actions.append("Invoke")
    if patterns.expand is not None:
        state = int(_safe(lambda: patterns.expand.CurrentExpandCollapseState, 3) or 3)
        if state != 3:  # LeafNode has no expand/collapse
            actions.extend(["Expand", "Collapse"])
    if patterns.scroll_item is not None:
        actions.append("Raise")
    if patterns.scroll is not None:
        actions.extend(["Scroll Up", "Scroll Down", "Scroll Left", "Scroll Right"])
    return {
        "index": index,
        "depth": depth,
        "name": str(_safe(lambda: element.CurrentName, "") or ""),
        "role": _ROLES.get(role_id, f"ControlType({role_id})"),
        "value": value,
        "actions": actions,
        "left": int(rect.left) if rect is not None else 0,
        "top": int(rect.top) if rect is not None else 0,
        "right": int(rect.right) if rect is not None else 0,
        "bottom": int(rect.bottom) if rect is not None else 0,
        "enabled": bool(_safe(lambda: element.CurrentIsEnabled, True)),
        "offscreen": bool(_safe(lambda: element.CurrentIsOffscreen, False)),
        "focused": bool(_safe(lambda: element.CurrentHasKeyboardFocus, False)),
        "editable": editable,
    }


def _walk(walker: Any, element: Any, depth: int, nodes: list[dict[str, Any]], elements: list[Any]) -> None:
    if len(nodes) >= MAX_NODES or depth > MAX_DEPTH:
        return
    elements.append(element)
    nodes.append(_describe(element, len(nodes), depth))
    try:
        child = walker.GetFirstChildElement(element)
    except COMError:
        return
    while child and len(nodes) < MAX_NODES:
        _walk(walker, child, depth + 1, nodes, elements)
        try:
            child = walker.GetNextSiblingElement(child)
        except COMError:
            break


def snapshot(hwnd: int) -> str:
    """Capture the control-view tree of `hwnd` and remember it for later actions."""
    global _cached_hwnd, _nodes, _elements
    client = _automation()
    root = client.ElementFromHandle(int(hwnd))
    if not root:
        raise LookupError(f"UI Automation could not read window {hwnd}")
    nodes: list[dict[str, Any]] = []
    elements: list[Any] = []
    _walk(client.ControlViewWalker, root, 0, nodes, elements)
    _cached_hwnd = int(hwnd)
    _nodes = nodes
    _elements = elements
    text = format_accessibility(nodes)
    if len(nodes) >= MAX_NODES:
        text += f"\n(truncated at {MAX_NODES} elements; scroll or narrow the window and snapshot again)"
    return text or "(empty accessibility tree)"


def cached_hwnd() -> int:
    return _cached_hwnd


def _require(index: int) -> tuple[dict[str, Any], Any]:
    if not _elements or index < 0 or index >= len(_elements):
        raise LookupError(
            f"element_index {index} is not in the latest accessibility snapshot; call get_window_state again"
        )
    return _nodes[index], _elements[index]


def element_rect(index: int) -> tuple[int, int, int, int]:
    node, _element = _require(index)
    return int(node["left"]), int(node["top"]), int(node["right"]), int(node["bottom"])


def set_value(index: int, value: str) -> None:
    node, element = _require(index)
    _safe(lambda: element.SetFocus())
    patterns = _patterns(element)
    if patterns.value is None:
        raise ValueError(f"element [{index}] {node['role']} has no editable value")
    try:
        patterns.value.SetValue(value)
    except COMError as exc:
        raise OSError(f"could not set the value of element [{index}]: {exc}") from exc


def secondary_action(index: int, action: str) -> None:
    from .a11y import canonical_action

    node, element = _require(index)
    kind = canonical_action(action)
    advertised = {canonical_action(name) for name in node["actions"]}
    if kind not in advertised:
        raise ValueError(f"element [{index}] does not offer {action!r}; it offers {list(node['actions'])}")
    _safe(lambda: element.SetFocus())
    patterns = _patterns(element)
    try:
        if kind == "invoke" and patterns.invoke is not None:
            patterns.invoke.Invoke()
        elif kind == "expand" and patterns.expand is not None:
            patterns.expand.Expand()
        elif kind == "collapse" and patterns.expand is not None:
            patterns.expand.Collapse()
        elif kind == "raise" and patterns.scroll_item is not None:
            patterns.scroll_item.ScrollIntoView()
        elif kind == "scroll_up" and patterns.scroll is not None:
            patterns.scroll.Scroll(_NO_AMOUNT, _SMALL_DEC)
        elif kind == "scroll_down" and patterns.scroll is not None:
            patterns.scroll.Scroll(_NO_AMOUNT, _SMALL_INC)
        elif kind == "scroll_left" and patterns.scroll is not None:
            patterns.scroll.Scroll(_SMALL_DEC, _NO_AMOUNT)
        elif kind == "scroll_right" and patterns.scroll is not None:
            patterns.scroll.Scroll(_SMALL_INC, _NO_AMOUNT)
        else:
            raise ValueError(f"element [{index}] could not run {action!r}")
    except COMError as exc:
        raise OSError(f"secondary action {action!r} failed on element [{index}]: {exc}") from exc
