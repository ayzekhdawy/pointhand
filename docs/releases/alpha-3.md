# PointHand v0.2.0-alpha.3

An alpha for supervised Windows workflows. This download adds covered-window capture and the computer-use behavior that landed in source after Alpha 2.

## Highlights

- A window screenshot uses that window's own pixels when Windows graphics capture is available, including while another window covers it. The image is labeled `capture: window` and does not raise the window. If window capture is unavailable, the image is labeled `capture: visible` and may raise the window. Desktop and monitor screenshots stay the visible composite.
- Pointer actions bring the target window forward and stop when a different window still covers the point. Typing and `activate_window` still focus the target. A new action still waits until the pointer has been idle. A started action continues through mouse movement; physical Escape and `emergency_stop` cancel it.
- `screenshotId` binds a click, drag, scroll, or move to one bounded region. A window that crosses monitors, each monitor screenshot, and an owned popup has its own id and screen origin.

## Validation and limits

Windows CI runs Rust, TypeScript, helper and native protocol checks, builds the executable, extracts the ZIP and verifies checksums, the executable manifest and isolated MCP startup. Local host tests and client tests passed. On one display, a covered probe window kept its own color and was not brought forward by the screenshot.

Secure-desktop switching, display disconnection during input, additional physical monitor combinations and clean non-developer-PC startup remain to verify. This capture check did not use a second physical monitor. See the validation checklist.

## Install

Download the Windows x64 ZIP and `.sha256` companion. Verify with `Get-FileHash -Algorithm SHA256`, extract, install Node.js 20+ and configure your MCP client with the absolute extracted `client/dist/index.js` path. Rust/Python are not needed for the main portable server. Use `POINTHAND_ALLOW` to restrict apps and supervise the session. Restart a running PointHand so it loads this executable.

## Join in

Code, documentation, creative examples and compatibility reports are welcome in Turkish or English. Read CONTRIBUTING.md and browse issues labeled `good first issue` or `help wanted`.

The application components retain their 0.2.0 base version; the Git tag identifies this alpha distribution. Alpha 2 and Alpha 1 remain available unchanged.
