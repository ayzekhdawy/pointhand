<p align="center">
  <img src="assets/logo.png" alt="pointhand" width="420">
</p>

<p align="center"><b>A Windows computer-use server for AI agents, over MCP.</b></p>

> **Status: alpha (v0.2.0-alpha.1).** The core tools run through a real MCP client:
> windows, the UI Automation tree, screenshots, input, clipboard, drawing, and `batch`.
> Verified live against Paint (strokes and shapes land where requested).
> Native Rust glass frame and cursor are live-tested. Multi-monitor and mixed-DPI
> verification and broader benchmarks remain on the roadmap.

PointHand lets an MCP client see the screen and operate Windows apps: windows,
screenshots, mouse, keyboard, scrolling, the UI Automation tree, the clipboard,
and freehand or geometric drawing. The main path is a TypeScript MCP server that
spawns a Rust executable. The host now owns the native Win32 glass frame,
activity label, cursor, motion trail and click ripple. The main TypeScript/Rust
path needs no Python, Pillow, Tk, stamp file or separate overlay process.
The Python server remains an optional reference implementation.

This is an independent Win32 and UI Automation implementation. It does not ship
or wrap any vendor computer-use binary.

## Download and quick start

Published by [@ayzekhdawy](https://github.com/ayzekhdawy).

Download the Windows x64 ZIP and SHA-256 file from [Releases](https://github.com/ayzekhdawy/pointhand/releases). This is an alpha release for supervised creative workflows. Mixed-DPI and multiple-monitor validation are still pending.

Extract the ZIP, install Node.js 20 or newer, and configure your MCP client:

```json
{
  "mcpServers": {
    "pointhand": {
      "command": "node",
      "args": ["C:/Tools/PointHand/client/dist/index.js"],
      "env": { "POINTHAND_ALLOW": "mspaint.exe,Illustrator.exe" }
    }
  }
}
```

Replace the example path with the extracted package's `client/dist/index.js`. The portable package includes the Rust host and runtime dependencies; it needs no Rust or Python installation. Verify the ZIP hash with `Get-FileHash -Algorithm SHA256` before use. [Mona Lisa tutorial and demo](examples/mona-lisa/README.md) · [Safety](SECURITY.md) · [Known validation gaps](docs/release-checklist.md).

## Layout

| Piece | Role |
| --- | --- |
| `client/` | TypeScript MCP server. Speaks JSON-lines to the host and returns PNG tool results as images. |
| `host/` | `pointhand-computer-use.exe`. Windows, input, screenshots, accessibility, clipboard, idle guard, allow-list, Escape stop, native overlay. |
| `pointhand/` | Python MCP server. Optional reference implementation with its own Python overlay. |
| `scripts/` | `build-windows.ps1` (build + manifest), `mcp-smoke.mjs` (talk to the server like a real client). |

One JSON object per line. A request is `{"id","method","params"}`. A response is
`{"id","ok","result"}` or `{"id","ok":false,"error"}`. The client passes
`--parent-pid` so the host exits with it. Physical Escape interrupts strokes, typing, idle waits and batch waits.
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
`guard_status`, `get_system_info`, `draw_strokes`.

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
An empty list allows every app. Escape stops the current turn.

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

## Requirements

| Needed | Why | How to get it |
| --- | --- | --- |
| Windows 10 or 11 | Win32 and UI Automation | - |
| Node.js 20+ | Runs the MCP server | `winget install OpenJS.NodeJS.LTS` |
| Rust (stable) | Builds the host | [rustup.rs](https://rustup.rs) |
| Visual Studio Build Tools, "Desktop development with C++" | Rust's MSVC linker on Windows | `winget install Microsoft.VisualStudio.2022.BuildTools` |
| Python 3.10+ (optional) | Only for the optional Python reference server | `winget install Python.Python.3.12` |

## Build

```powershell
powershell -File scripts\build-windows.ps1
cd client
npm install
npm run build
```

The script writes `bin\windows\x64\pointhand-computer-use.exe` and a
`manifest.json` of sha256 and size. Point the client at a custom binary with
`POINTHAND_HOST`.

MCP client configuration:

```json
{
  "mcpServers": {
    "pointhand": {
      "command": "node",
      "args": ["C:/path/to/winpilot/client/dist/index.js"]
    }
  }
}
```

Python front door:

```powershell
python -m pip install -e ".[test]"
python -m pointhand.server
```

## Tests

```powershell
python -m pytest
cargo test --release --manifest-path host\Cargo.toml
cargo clippy --release --manifest-path host\Cargo.toml -- -D warnings
node --test scripts/host-regression.mjs
cd client; npm test
```

Pure logic (keys, coordinates, shapes, allow-list, glass) does not touch the
desktop. End-to-end scenarios that move the real mouse are skipped unless
`POINTHAND_E2E=1` is set. `node scripts/mcp-smoke.mjs tools` lists the tools
through a real MCP client.

## Roadmap

- [x] Connect Rust mouse/click/stroke activity to PointHand glass frame and cursor
- [x] Standalone Rust glass renderer without Python
- [ ] Multi-monitor and mixed-DPI verification
- [ ] Optional, off-by-default system access (shell, files) with allow-lists and an audit log
- [ ] `draw_image`: turn a picture into strokes
- [ ] Reproducible benchmarks

## Clean-room statement

PointHand is an independent project. It uses documented Windows APIs
(`SendInput`, UI Automation, `EnumWindows`, the clipboard, `GetLastInputInfo`)
and the general idea of pairing a screenshot with an accessibility tree. It
contains no code from, and is not affiliated with, any vendor computer-use
product.

## License

MIT. See [LICENSE](LICENSE).

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

## Verification (2026-10-15)

Python: 48 passed, 8 desktop E2E scenarios skipped. Rust release tests: 18 passed.
TypeScript build and tests: 13 passed. Real binary regression tests: 4 passed. Tests cover aliases, `parses_sky_style_chord`,
brace compatibility, malformed chords, allow-lists, accessibility formatting,
shape paths, window/click parameter compatibility, partial input failures,
oversized lines/coordinates, native cancellation, process errors and EOF shutdown.

The release host and SHA256/size manifest were regenerated in `bin/windows/x64`.
`cargo clippy --release -- -D warnings` passes without warnings.
A real MCP client selected Paint's pencil through PointHand's UIA tree and drew
seven fish strokes through TypeScript and Rust. Result: `.scratch/paint-fish.png`.
The latest live test ran with Python explicitly unavailable and exercised the
native Rust frame/cursor through PointHand MCP. `.scratch/native-overlay-status.json`
records renderer readiness and frame timings.
Run `scripts/paint-fish.mjs` only against an observed blank Paint window with
the matching pixel scale and window border offset. The real desktop test required
execution outside the command sandbox; initial successful input responses
inside the sandbox did not produce visible strokes.

## Native cursor performance

The Rust renderer keeps a 128 x 128 cursor surface on a 60 FPS target loop.
Four small edge-strip windows update the rounded glass frame at 10 FPS;
the status label and pointer sprite are cached. It reads the actual Windows
cursor position without easing lag, and draws a trail and click ripple in place.
Activity is shared in process memory, with no IPC files and no child renderer.
Hidden surfaces stop rendering. Virtual desktop bounds are refreshed to handle
resolution changes; multi-monitor/mixed-DPI behavior still needs live verification.

The final live Paint run, with `POINTHAND_ALLOW=mspaint.exe` and Python
unavailable, reported average active frame-loop FPS of 59.99, a last interval
of 17,123 microseconds, and 289 microseconds of render work. The renderer was
ready and reported no error. Normal capture exclusion was also verified with
the renderer active, and no host or overlay process remained after MCP closed.
`.scratch/native-overlay.png` shows the native visuals; `.scratch/paint-fish.png`
shows the normal capture with the overlay excluded. These are host loop timings,
not a compositor/display refresh guarantee. `overlay_status` includes average
active FPS, maximum frame interval, last render timing and error details.

The renderer uses documented Win32 layered windows:
[UpdateLayeredWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-updatelayeredwindow)
and [SetWindowDisplayAffinity](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity).


## Creative workflows

[Reproduce the Mona Lisa drawing in Paint](examples/mona-lisa/README.md): includes the public reference, reusable stroke plan, actual Paint result and a silent demo video. Input is delivered through PointHand MCP; the script does not paste a bitmap into Paint.

[Pointer behavior and precision](docs/input-behavior.md) explains natural acceleration, instant positioning and exact drawing geometry.

For a scoped Paint session, set `POINTHAND_ALLOW=mspaint.exe` in the MCP server environment. Full executable paths restrict matching to that specific path. Click and scroll re-check the actual receiving application's permission after pointer travel. Physical intervention interrupts an action and Escape stops mutations. These controls reduce accidental or unauthorized use; they are not an isolation boundary against a malicious local process. No key content is logged by the physical-input guard.
