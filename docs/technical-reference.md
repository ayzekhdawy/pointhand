# Technical reference

[← Back to PointHand](../README.md)

Implementation details and API conventions for the current alpha. See the [changelog](../CHANGELOG.md) for differences from the Alpha 2 download.

## Layout
| Piece | Role |
| --- | --- |
| `client/` | TypeScript MCP server. Speaks JSON-lines to the host and returns PNG tool results as images. |
| `host/` | `pointhand-computer-use.exe`. Windows, input, screenshots, accessibility, clipboard, idle guard, allow-list, Escape stop, native overlay. |
| `pointhand/` | Python MCP server. Optional reference implementation with its own Python overlay. |
| `scripts/` | `build-windows.ps1` (build + manifest), `mcp-smoke.mjs` (talk to the server like a real client). |

One JSON object per line. A request is `{"id","method","params"}`. A response is
`{"id","ok","result"}` or `{"id","ok":false,"error"}`. The client passes
`--parent-pid` so the host exits with it. Physical Escape interrupts strokes, typing, idle waits and batch waits. Alpha 2 also exposes an out-of-band MCP `emergency_stop` tool.
Input requests are bounded to 4 MiB; oversized or invalid UTF-8 lines are drained
and rejected without breaking the next request.

A window screenshot uses that window's own pixels when Windows graphics capture
is available, including while another window covers it. That capture does not
bring the window forward. Each region is labeled `capture: "window"` or
`capture: "visible"`. The visible label means graphics capture was unavailable,
so the image is the desktop composite and the window may be raised. Desktop and
monitor screenshots stay the visible composite. Pointer actions bring the target
window forward and refuse a point that a different window still covers.

## What the agent can call
`list_apps`, `list_windows`, `get_window`, `launch_app`, `activate_window`,
`get_window_state`, `screenshot`, `click`, `move`, `drag`, `draw`, `draw_shape`,
`scroll`, `type_text`, `press_key`, `set_value`, `perform_secondary_action`,
`paste`, `clipboard_read`, `clipboard_write`, `batch`, `overlay_status`,
`guard_status`, `get_system_info`, `draw_strokes`, `emergency_stop`.

`emergency_stop` is available in Alpha 2: it interrupts active
input without waiting for the request queue. Restart the MCP server to resume.
See [release changes](../CHANGELOG.md); the existing Alpha 1 ZIP has 25 tools.

`draw_shape` traces a line, ellipse, rectangle, polygon, star, heart, spiral, or
zigzag with the left button held (pick a pen in the app first). `screenshot`
captures the whole desktop or a `[left, top, right, bottom]` region.

`get_window_state` can include a PNG and an accessibility object. The object
always has `tree` when text was requested, and adds `focused_element`,
`selected_elements`, `document_text`, or `selected_text` when UI Automation
exposes them. Click an `element_index` from that tree, or a point. With
`window_id`, `x` and `y` are window pixels. Divide image pixels by
`screenshot.scale`. Each captured region returns `screenshot.id` and a
`screenshots` entry with that id, image size, screen origin, `zIndex`, `role`,
and `scale`. A window on one monitor has one region. A window that crosses
monitors adds one `monitor` region per intersection, using that intersection's
origin. Owned popups, including a menu on another monitor, are `transient`
regions. `screenshot` without a region returns one image per monitor in
addition to the combined desktop image. Pass the region's id as `screenshotId`
to `click`, `drag`, `scroll`, or `move`. `x` and `y` are physical pixels inside
that region. The id must come from the latest capture of that window or desktop,
and the window must still have the captured size and position. A desktop id
also requires the virtual screen to be unchanged. The id does not turn image
pixels into region pixels. Positive `scrollY` moves the page down. Window
records include a `monitors` array with each touched display and the overlap.

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
and does nothing. After the action starts, moving or clicking the mouse does
not cancel it. Physical Escape stops the active stroke and later mutations
until the MCP server restarts. `POINTHAND_ALLOW` is a comma-separated list of
paths, exe names, or names without an extension (`notepad.exe` also allows
`notepad`). An empty list allows every app.

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
Window actions accept the window2 `screenshotId` contract. PointHand still
returns the PNG/scale object beside the `screenshots` array. `width` and
`height` on that array are image pixels, not a second coordinate system.
`lastUsedDate` and `useCount` are omitted; Windows does not supply them here.
A window image is that window's own pixels when graphics capture works, so a
covered window is not photographed as the window in front of it. `capture` is
`window` or `visible`. Pointer actions raise the target and stop if another
window remains on top of the point. Desktop screenshots stay the visible composite.
