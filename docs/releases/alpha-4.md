# PointHand v0.2.0-alpha.4

An alpha for supervised Windows workflows. This download adds the computer-use behavior that landed in source after Alpha 3.

## Highlights

- If a pointer point stays under a different window, no input is sent. The error names that process, including the Start menu, and includes a fresh screenshot of the target when window capture works. Retry the action once with the new `screenshotId`. A batch stops and leaves the image for `get_window_state`.
- `paste` accepts `format` `text`, `md`, or `html`. Markdown and HTML also place the Windows HTML clipboard format. Plain text remains available. Links are limited to http, https, and mailto.
- After a recent action, `get_window_state` and `screenshot` wait about 1 second, and up to 5 seconds more while a progress indicator is visible or the layout is still changing. The result includes `waited_ms`. Do not add a separate pause. Physical Escape still cancels the wait.

## Validation and limits

Windows CI runs Rust, TypeScript, helper and native protocol checks, builds the executable, extracts the ZIP and verifies checksums, the executable manifest and isolated MCP startup. Local host tests (36) and client tests (18) passed before this tag. The covered-point message and the paste clipboard bytes are unit-tested. The observation wait is unit-tested against its sleep budget.

Secure-desktop switching, display disconnection during input, additional physical monitor combinations and clean non-developer-PC startup remain to verify. See the validation checklist.

## Install

Download the Windows x64 ZIP and `.sha256` companion. Verify with `Get-FileHash -Algorithm SHA256`, extract, install Node.js 20+ and configure your MCP client with the absolute extracted `client/dist/index.js` path. Rust/Python are not needed for the main portable server. Use `POINTHAND_ALLOW` to restrict apps and supervise the session. Restart a running PointHand so it loads this executable.

## Join in

Code, documentation, creative examples and compatibility reports are welcome in Turkish or English. Read CONTRIBUTING.md and browse issues labeled `good first issue` or `help wanted`.

The application components retain their 0.2.0 base version; the Git tag identifies this alpha distribution. Alpha 3, Alpha 2 and Alpha 1 remain available unchanged.
