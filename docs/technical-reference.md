# Technical reference

[← Back to PointHand](../README.md)

Implementation details and API conventions for the current alpha. See the [changelog](../CHANGELOG.md) for differences from the Alpha 2 download.

## Layout
| Piece | Role |
| --- | --- |
| `client/` | TypeScript MCP server. Speaks JSON-lines to the host and returns PNG tool results as images and WAV tool results as audio. |
| `host/` | `pointhand-computer-use.exe`. Windows, input, screenshots, accessibility, clipboard, speaker loopback, idle guard, allow-list, Escape stop, native overlay. |
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
window forward. If a different window still covers the point, no input is sent.
The result names that process, including the Start menu when it is
`StartMenuExperienceHost.exe`, and includes a fresh screenshot of the target
when capture works. Retry that input once with the new `screenshotId`. The host
line stays `ok: true` with `acted: false` so the screenshot travels with the
refusal. The MCP tool reports that result as an error. A batch stops there and
omits the image bytes; call `get_window_state`, then retry once.

## What the agent can call
`list_apps`, `list_windows`, `get_window`, `launch_app`, `activate_window`,
`get_window_state`, `screenshot`, `click`, `move`, `drag`, `draw`, `draw_shape`,
`scroll`, `type_text`, `press_key`, `set_value`, `perform_secondary_action`,
`paste`, `clipboard_read`, `clipboard_write`, `start_audio_recording`,
`stop_audio_recording`, `batch`, `overlay_status`, `guard_status`,
`get_system_info`, `draw_strokes`, `emergency_stop`, `find_elements`,
`wait_for`, `set_window_state`, `move_window`, `close_window`, `get_element`.

## Background mode

Background mode lets the agent work in one window while you keep working in
another. Pass `background: true` on a call, on `batch` (it becomes the default
for every action), or set `POINTHAND_BACKGROUND=1` to make it the default; an
explicit `background: false` always wins.

- `set_value` and `perform_secondary_action` run through UI Automation without
  moving focus. This is the most reliable background path.
- `click`, `move`, `drag`, `draw`, `draw_shape`, `draw_strokes` and `scroll`
  post mouse messages to the deepest visible child of the target window under
  the point. The real pointer does not move, and a covered window still gets
  the input. The window must not be minimized.
- `type_text` posts characters to the control that has focus inside the target
  window's thread; click the field with `background: true` first. `press_key`
  posts one key without modifiers. Chords such as `ctrl+s` cannot be posted
  without touching your keyboard and are refused.
- `paste` is refused because it would replace your clipboard.
- `get_window_state` with `background: true` captures the window without waiting
  for you to stop typing.

Every background call needs a target window (`window_id`, an `element_index`, or a
window `screenshotId`). It skips the idle wait, because nothing physical moves,
and does not draw the cursor overlay. The allow-list, the elevated-window check,
the locked-desktop check and physical Escape still apply. Escape stops every
mutation, including when you press it in your own window.

Posted messages are not proof of delivery. Classic Win32 controls accept them;
Chromium, Electron and some WinUI surfaces may ignore them. Results report
`delivery: "posted"`; check the effect with a window screenshot, which reads the
window's own pixels even while it is covered.

`get_element` reads one element from the latest snapshot: role, name,
`automation_id`, class, rectangle, enabled, offscreen, focused, value, toggle
state and runnable actions. A password field never returns its value.
`perform_secondary_action` also accepts `Focus`.

`emergency_stop` is available in Alpha 2: it interrupts active
input without waiting for the request queue. Restart the MCP server to resume.
See [release changes](../CHANGELOG.md); the existing Alpha 1 ZIP has 25 tools.

`draw_shape` traces a line, ellipse, rectangle, polygon, star, heart, spiral, or
zigzag with the left button held (pick a pen in the app first). `screenshot`
captures the whole desktop or a `[left, top, right, bottom]` region.

`get_window_state` can include a PNG and an accessibility object. The object
always has `tree` when text was requested, and adds `focused_element`,
`selected_elements`, `document_text`, or `selected_text` when UI Automation
exposes them. A line includes `actions="Invoke, Expand"` only for patterns that
element can run, including `Toggle` and `Select`. Scroll labels are left off at
the end of that axis. Check boxes and toggle buttons add `checked`, `unchecked`
or `mixed`. A password field adds `password` and never shows its value. Pass one
of those labels to `perform_secondary_action`; matching ignores case. `Toggle`
returns the new `state`. A disabled element is refused before anything is sent. Click an
`element_index` from that tree, or a point. With
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

After a recent action, `get_window_state` and `screenshot` wait about 1 second before reading the screen. If a progress indicator is visible, or the window rectangle or layout is still changing, the wait continues in 200 ms steps for up to 5 more seconds. The result field is `waited_ms`. A finished progress bar does not keep the wait open. The agent should not add its own pause. The wait is not the idle gate: it does not require the pointer to stay still, and physical Escape still cancels it. A covered-point refusal does not start this clock.

`batch` runs up to 50 actions and stops at the first error. The Rust host
accepts `click`, `double_click`, `triple_click`, `right_click`, `move`, `drag`,
`draw`, `scroll`, `type`, `key`, `set_value`, `secondary`, `toggle`, `select`,
`paste`, `focus_window`, `window_state`, `move_window`, `wait`, and `wait_for`.
`click` accepts `click_count` 1 to 3.

`find_elements` searches one window's accessibility tree by `name` (a
case-insensitive part of the accessible name), `role` (control type such as
`Button` or `CheckBox`; case and spaces ignored) and `automation_id` (exact).
Every given field must match. It walks up to 3000 elements, depth 32 and 256
children per element, and returns at most `limit` matches (default 50, maximum
200) with their `element_index`. It replaces the cached snapshot, so those
indices work with `click`, `set_value` and `perform_secondary_action`.
`complete: false` means the walk stopped before the end of the tree.

`wait_for` polls every 250 ms until a window whose title contains
`window_title` appears, or an element matching the `find_elements` fields
appears in `window_id`. `gone: true` waits for it to disappear instead; a window
that closes counts as gone. `timeout_ms` defaults to 5000 and stops at 30000; a
timeout is an error, so a batch stops there. It reads only, takes no input lane,
and physical Escape stops it.

`set_window_state` minimizes, maximizes or restores a window. `move_window`
sets `x`, `y`, `width` and `height` in physical screen pixels; omitted values
keep the current ones, a maximized or minimized window is restored first, each
side must be at least 100 px and the rectangle must overlap the virtual desktop.
`close_window` posts the same close request as the title-bar X and reports
whether the window was gone within 2 seconds; the app may show a save prompt
instead. It refuses Windows session processes such as `explorer.exe`, `dwm.exe`
and `csrss.exe` by exact name, and it is not available inside `batch`. All three
use the same allow-list, idle wait and Escape stop as other input.

Background mode follows agent-desktop's headless accessibility actions. The idea for these tools comes from Vercel Labs'
[agent-desktop](https://github.com/vercel-labs/agent-desktop) (Apache-2.0).
PointHand reimplements them for its own host; no agent-desktop code is copied.

Key names include `Return`, `Control_L+a`, `KP_0`, `page_up`, and `super`.

`paste` takes `format`: `text` (default), `md`, or `html`. `text` places Unicode text and presses Ctrl+V. `md` and `html` also place the Windows `HTML Format` clipboard value, so a document or slide editor can paste headings, lists, emphasis, and links. The plain-text value stays the markdown source, or the tag-stripped HTML. Supported markdown is headings, paragraphs, `-` and `*` lists, numbered lists, `` `code` ``, fenced code, `**bold**`, `*italic*`, and links. A link is kept only when its target is `http`, `https`, or `mailto`. The previous clipboard contents stay replaced. A paste is limited to 200000 characters.

`start_audio_recording` records what Windows is playing through the default speaker. It is loopback, not a microphone, and it does not wait for the pointer to be idle. Other actions continue while it records. `max_duration_ms` defaults to 60000 and cannot exceed 300000. One recording at a time. `stop_audio_recording` returns a 24 kHz stereo 16-bit WAV. The host line includes a temp `filepath`, `bytes`, and `data_url`. The MCP tool attaches one `audio/wav` part and removes `bytes` and `data_url` from the text. A quiet stretch stays in the file as silence, so the duration matches how long capture ran. Physical Escape ends capture early; call `stop_audio_recording` to collect the partial file. The host drops an unfinished recording when it exits.

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

Windows blocks synthetic input into elevated windows. Before sending input,
PointHand compares the target process's integrity level with its own. A higher
target returns `elevated_target` and nothing is sent. Capture and
`activate_window` still work. If the level cannot be read, the input goes ahead
as before.

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
