# Technical reference

[← Back to PointHand](../README.md)

Implementation details and API conventions for the current source branch. See the [changelog](../CHANGELOG.md) for differences from the Alpha 1 download.

## Layout
| Piece | Role |
| --- | --- |
| `client/` | TypeScript MCP server. Speaks JSON-lines to the host and returns PNG tool results as images. |
| `host/` | `pointhand-computer-use.exe`. Windows, input, screenshots, accessibility, clipboard, idle guard, allow-list, Escape stop, native overlay. |
| `pointhand/` | Python MCP server. Optional reference implementation with its own Python overlay. |
| `scripts/` | `build-windows.ps1` (build + manifest), `mcp-smoke.mjs` (talk to the server like a real client). |

One JSON object per line. A request is `{"id","method","params"}`. A response is
`{"id","ok","result"}` or `{"id","ok":false,"error"}`. The client passes
`--parent-pid` so the host exits with it. Physical Escape interrupts strokes, typing, idle waits and batch waits. The source
branch also exposes an out-of-band MCP `emergency_stop` tool.
Input requests are bounded to 4 MiB; oversized or invalid UTF-8 lines are drained
and rejected without breaking the next request.

Screenshots are the visible desktop at the window rectangle. Pixels hidden
behind another window are whatever is in front. `activate_window` brings the
target forward before a shot.

## What the agent can call
`list_apps`, `list_windows`, `get_window`, `launch_app`, `activate_window`,
`get_window_state`, `screenshot`, `click`, `move`, `drag`, `draw`, `draw_shape`,
`scroll`, `type_text`, `press_key`, `set_value`, `perform_secondary_action`,
`paste`, `clipboard_read`, `clipboard_write`, `batch`, `overlay_status`,
`guard_status`, `get_system_info`, `draw_strokes`, `emergency_stop`.

`emergency_stop` is a source-branch addition after Alpha 1: it interrupts active
input without waiting for the request queue. Restart the MCP server to resume.
See [unreleased changes](../CHANGELOG.md); the existing Alpha 1 ZIP has 25 tools.

`draw_shape` traces a line, ellipse, rectangle, polygon, star, heart, spiral, or
zigzag with the left button held (pick a pen in the app first). `screenshot`
captures the whole desktop or a `[left, top, right, bottom]` region.

`get_window_state` can include a PNG and an accessibility tree. Click an
`element_index` from that tree, or a point. With `window_id`, `x` and `y` are
window pixels. Divide image pixels by `screenshot.scale`. Positive `scrollY`
moves the page down.

`batch` runs up to 50 actions and stops at the first error. The Rust host
accepts `click`, `double_click`, `right_click`, `move`, `drag`, `draw`, `scroll`,
`type`, `key`, `set_value`, `secondary`, `paste`, `focus_window`, and `wait`.

Key names include `Return`, `Control_L+a`, `KP_0`, `page_up`, and `super`.

## Safety
An agent with a mouse and keyboard can do anything you can. Treat on-screen
content as untrusted, and do not point it at banking, password managers, or
anything irreversible without watching it.

Input waits until the keyboard and mouse have been idle for a short moment.
If that does not happen within about five seconds the call returns `user_active`
and does nothing. `POINTHAND_ALLOW` is a comma-separated list of paths, exe
names, or names without an extension (`notepad.exe` also allows `notepad`).
An empty list allows every app. Physical Escape stops mutations until the MCP server restarts.

Windows blocks synthetic input into elevated windows. PointHand reports that
as an error.

The Rust host draws PointHand's own click-through, non-activating glass frame
and cursor. Native layered windows are excluded from screenshots by default;
if capture exclusion fails, the renderer reports an error and shuts down.
`POINTHAND_OVERLAY=0` disables it. `POINTHAND_OVERLAY_CAPTURE=1` includes the
visuals in screenshots for debugging. `overlay_status` reports readiness,
errors and last render/frame timings. Renderer windows are omitted from app lists.
Overlay surfaces and GDI resources close with the Rust host.

A failed or cancelled stroke releases the left mouse button; keyboard transactions
attempt to release every pressed key even if another release fails. Client timeouts
reject queued calls and signal an out-of-band native stop event, allowing cleanup
before a one-second forced termination fallback. A configured `POINTHAND_HOST`
that does not exist now produces an explicit error.

## Key and window compatibility
`Control_L+a`, `Return`, `KP_0`, `page_up`, `super`, and left/right Control,
Shift and Alt aliases use Windows virtual-key codes. Numpad arithmetic aliases
(Add/Subtract/Multiply/Divide/Decimal) preserve their separate VKs.
Punctuation includes `period`, `less`, `slash`, `question`, and brackets.
Use explicit Shift for shifted punctuation: `Control_L+Shift_L+period`.
Empty chord components are rejected; repeated modifiers are deduplicated.

Rust and Python `brace_chord()` are pure compatibility helpers:
`Control_L+a` becomes `{Ctrl}a`, and `Return` becomes `{Enter}`.
Brace syntax stays in the alias table; actual input uses virtual-key codes.
`type_text` types braces literally.

MCP window actions accept `window: {"id": 123, "app": "...", "title": "..."}`
as well as `window_id`; `get_window` also accepts `id`.
`click` accepts `mouse_button` and `click_count`. Conflicting window IDs fail.
Indexed actions must target the window of the latest UIA snapshot.
Value and secondary actions now honor the idle guard and allow-list.
This is parameter compatibility with window2, not the complete sky protocol:
screenshots retain PointHand PNG/scale output; screenshotId is not supported.
