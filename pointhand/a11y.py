"""Pure formatting for the accessibility tree shown to agents."""

from __future__ import annotations

from typing import Mapping

_ACTIONS = {
    "expand": "expand",
    "collapse": "collapse",
    "invoke": "invoke",
    "raise": "raise",
    "scroll up": "scroll_up",
    "scroll down": "scroll_down",
    "scroll left": "scroll_left",
    "scroll right": "scroll_right",
}


def canonical_action(name: str) -> str:
    """Normalise a secondary-action label. Matching is case-insensitive."""
    key = " ".join(name.replace("_", " ").strip().lower().split())
    if key not in _ACTIONS:
        known = ", ".join(sorted(_ACTIONS))
        raise ValueError(f"unknown secondary action {name!r}; expected one of: {known}")
    return _ACTIONS[key]


def format_accessibility(nodes: list[Mapping[str, object]]) -> str:
    """Render element indexes, roles, names, and advertised actions.

    Indexes stay valid only until the next snapshot of that window.
    """
    lines: list[str] = []
    for node in nodes:
        depth = int(node.get("depth") or 0)
        name = str(node.get("name") or "").replace("\n", " ").replace('"', "'")
        bits = [f"[{int(node['index'])}]", str(node.get("role") or "Unknown")]
        if name:
            bits.append(f'"{name}"')
        value = str(node.get("value") or "")
        if value:
            shown = value.replace("\n", " ")
            if len(shown) > 120:
                shown = shown[:117] + "..."
            bits.append(f"value={shown!r}")
        flags = []
        if node.get("focused"):
            flags.append("focused")
        if node.get("editable"):
            flags.append("editable")
        if node.get("offscreen"):
            flags.append("offscreen")
        if node.get("enabled") is False:
            flags.append("disabled")
        if flags:
            bits.append(" ".join(flags))
        actions = node.get("actions") or []
        if actions:
            bits.append("actions=" + ",".join(str(item) for item in actions))
        lines.append("  " * depth + " ".join(bits))
    return "\n".join(lines)
