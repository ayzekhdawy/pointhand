# Changes

## Unreleased

- Pace overlay frames with an independent waitable timer; input stops do not shut down the renderer.
- Report per-monitor bounds/effective DPI and per-window DPI; add an interactive mixed-DPI test for extended displays.
- Correct one-pixel pointer/stroke drift by targeting absolute-coordinate pixel centers, and confirm real foreground activation before injecting input.

- Add `emergency_stop` through the MCP server's out-of-band native event. Long drawings and queued mutations stop without waiting for stdin; guard status reports whether a restart is required.
- Re-check the active desktop periodically during input and fail closed if physical-input monitoring becomes unavailable.
- Verify cancellation during a real held-button stroke, including the released Windows button state and blocked queued input.
- Support Turkish/English Paint selector aliases, pixel units and explicit per-control name mappings for other languages. Ambiguous matches stop the example.
- Verify ZIP checksums, executable manifests and isolated MCP startup from an extracted package in Windows CI.

These changes are in the source branch and are not included in the existing v0.2.0-alpha.1 download.

## v0.2.0-alpha.1

First public alpha published under @ayzekhdawy: native Rust overlay, 25 MCP tools, creative drawing examples, Windows tests and a portable ZIP with checksums. Public demo metadata and private builder paths were removed.
