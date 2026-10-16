# Changes

## Unreleased

- Bind `click`, `drag`, and `scroll` to the latest per-window `screenshotId`, and return screenshot id, origin, and accessibility text fields beside the existing PNG/scale result.
- A started action continues when the mouse moves or clicks. Physical Escape and `emergency_stop` still cancel it. A new action still waits until the pointer has been idle.
- Multi-monitor actions use bounded regions. A window that crosses displays, each monitor screenshot, and an owned popup has its own `screenshotId` and screen origin. `list_windows` reports the monitors a window touches.

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
