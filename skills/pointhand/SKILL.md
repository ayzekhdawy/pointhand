---
name: pointhand
description: Use the PointHand MCP server to see the Windows desktop and work in desktop apps (Paint, Illustrator, Office, settings dialogs) through real mouse and keyboard input. Use when the user asks you to click, type, draw or inspect something in a native Windows app.
---

# PointHand: Windows computer use

PointHand (https://github.com/ayzekhdawy/pointhand) is a Windows computer-use MCP server.
When it is configured as the `pointhand` MCP server, its tools appear as `pointhand__<tool>`
(the exact prefix depends on the client). It works on Windows 10/11 only.

## Workflow

1. **Find the target.** Use `list_apps` / `list_windows`, or `launch_app` if it is not running. Then use `activate_window`.
2. **Look before acting.** Call `get_window_state` (accessibility tree plus screenshot) or `screenshot`.
   Each result carries a `screenshotId` and a screen origin. Pointer actions (`click`, `drag`, `scroll`)
   bind to the latest `screenshotId` of that window, so take a fresh one after the UI changes.
3. **Prefer semantic actions.** Use `set_value` and `perform_secondary_action` (Invoke, Toggle, Select, Expand …)
   on accessibility elements when they exist. In a large window, `find_elements` finds a control by name, role or
   automation id without reading the whole tree. Fall back to coordinates (`click`, `move`, `drag`) only when they don't.
4. **Wait for the result, not a timer.** After a click that opens a dialog or starts loading, use `wait_for`
   (window title or element, or `gone: true`) instead of a fixed wait.
5. **Text and keys.** Use `type_text` for short input, `paste` for long or formatted text (`format`: `text`, `md` or `html`),
   and `press_key` for chords (e.g. `ctrl+s`).
6. **Windows.** `set_window_state` minimizes, maximizes or restores; `move_window` moves or resizes;
   `close_window` asks a window to close (it may show a save prompt; check `closed`).
7. **Drawing.** `draw` takes freehand strokes, `draw_shape` geometric shapes, and `batch` a sequence of actions in a single call.
8. **Background mode.** When the user wants to keep working while you work, pass `background: true`
   (or set it once on `batch`). Prefer `set_value` and `perform_secondary_action`; posted clicks and keys work in
   classic apps but may be ignored by browsers and Electron apps. The window must stay open (covered is fine),
   not minimized. Verify every step with `get_window_state` (`background: true`).
9. **Verify.** Take another `get_window_state` or `screenshot` and confirm the result before reporting success.

## Safety rules

- The user can press **Escape** at any time. It stops all further input until the server restarts.
  If a tool reports that the guard is stopped (`guard_status`), stop and tell the user. Do not loop.
- If something goes wrong mid-action, call `emergency_stop`.
- A refusal that names a covering window means the point is under another app. Retry once with the
  returned `screenshotId`; if it is still covered, ask the user.
- Only drive the apps the user asked for. `POINTHAND_ALLOW` limits which executables receive input.
- Never type passwords, payment data or secrets unless the user explicitly asks in this session.
- Elevated (admin) windows return `elevated_target`, and the secure desktop (UAC, lock screen) rejects input.
  Report it instead of retrying.
- Only call `close_window` when the user asked to close that window.

## Setup (for the user)

```toml
# ~/.grok/config.toml
[mcp_servers.pointhand]
command = "node"
args = ["C:/Tools/PointHand/client/dist/index.js"]

[mcp_servers.pointhand.env]
POINTHAND_ALLOW = "mspaint.exe"
```

Or, in the app: Settings → Extensions → MCP → Add server → **Fill in PointHand**.
