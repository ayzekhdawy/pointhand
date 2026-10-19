# Changes

## Unreleased

## v0.2.0-alpha.5

- Accessibility lines list only the secondary actions that element can run. Scroll labels are omitted at the end of that axis.
- `start_audio_recording` and `stop_audio_recording` record speaker loopback as a 24 kHz stereo WAV. The default limit is 60 seconds and the maximum is 300 seconds. Other actions continue. Physical Escape ends capture early, and stop returns the partial file. A quiet stretch stays in the file as silence.

These changes are included in Alpha 5. The existing v0.2.0-alpha.4 download remains unchanged.

## v0.2.0-alpha.4

- When a pointer point stays under a different window, name that process and return a fresh screenshot of the target. No input is sent. Retry the action once with the new `screenshotId`. A batch stops and leaves the image for `get_window_state`.
- `paste` accepts `format` `text`, `md`, or `html`. Markdown and HTML also place the Windows HTML clipboard format. Plain text remains available. Links are limited to http, https, and mailto.
- After a recent action, `get_window_state` and `screenshot` wait about 1 second, and up to 5 seconds more while a progress indicator is visible or the layout is still changing. The result includes `waited_ms`. Escape still cancels the wait.

These changes are included in Alpha 4. The existing v0.2.0-alpha.3 download remains unchanged.

## v0.2.0-alpha.3

- Capture a covered window from its own pixels when Windows graphics capture is available. The shot is labeled `capture: window` and does not raise the window. Pointer actions bring that window forward and refuse a point that stays covered. If window capture is unavailable, the shot is labeled `capture: visible` and may raise the window.
- Bind `click`, `drag`, and `scroll` to the latest per-window `screenshotId`, and return screenshot id, origin, and accessibility text fields beside the existing PNG/scale result.
- A started action continues when the mouse moves or clicks. Physical Escape and `emergency_stop` still cancel it. A new action still waits until the pointer has been idle.
- Multi-monitor actions use bounded regions. A window that crosses displays, each monitor screenshot, and an owned popup has its own `screenshotId` and screen origin. `list_windows` reports the monitors a window touches.

These changes are included in Alpha 3. The existing v0.2.0-alpha.2 download remains unchanged.

## v0.2.0-alpha.2

- Open community contributions with bilingual guidance, issue/PR templates and a public launch kit.
- Refresh the README showcase with a looping GIF of the actual Paint drawing.

- Add a repeatable manual physical-mouse interruption test; verify active drawing cancellation and released button state on the secondary virtual display.

- Pace overlay frames with an independent waitable timer; input stops do not shut down the renderer.
- Report per-monitor bounds/effective DPI and per-window DPI; add an interactive mixed-DPI test for extended displays.
- Correct one-pixel pointer/stroke drift by targeting absolute-coordinate pixel centers, and confirm real foreground activation before injecting input.

- Add `emergency_stop` through the MCP server's out-of-band native event. Long drawings and queued mutations stop without waiting for stdin; guard status reports whether a restart is required.
- Re-check the active desktop periodically during input and fail closed if physical-input monitoring becomes unavailable.
- Verify cancellation during a real held-button stroke, including the released Windows button state and blocked queued input.
- Support Turkish/English Paint selector aliases, pixel units and explicit per-control name mappings for other languages. Ambiguous matches stop the example.
- Verify ZIP checksums, executable manifests and isolated MCP startup from an extracted package in Windows CI.

These changes are included in Alpha 2. The existing v0.2.0-alpha.1 download remains unchanged.

## v0.2.0-alpha.1

First public alpha published under @ayzekhdawy: native Rust overlay, 25 MCP tools, creative drawing examples, Windows tests and a portable ZIP with checksums. Public demo metadata and private builder paths were removed.
