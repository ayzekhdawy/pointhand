# PointHand v0.2.0-alpha.2

A community-focused alpha for supervised Windows creative workflows. This release brings the source improvements after Alpha 1 into the portable download.

## Highlights

- 26 MCP tools, including out-of-band `emergency_stop`: active input and queued mutations stop until the server restarts.
- Per-monitor bounds/effective DPI and window DPI diagnostics; pixel-center mapping fixes one-pixel pointer/stroke drift.
- Independent overlay frame pacing, real foreground validation and periodic active-desktop checks during input.
- Turkish/English Paint selectors and custom name mappings; reproducible Mona Lisa script, public video and GIF.
- Contribution guide, issue/PR templates and beginner-friendly community tasks.

## Validation and limits

Windows CI runs Rust, TypeScript, helper and native protocol checks, builds the executable, extracts the ZIP and verifies checksums, the executable manifest and isolated MCP startup. Live PointHand tests covered a 125% primary display and 100% virtual secondary streamed through Sunshine, left/right placement, held-button emergency cancellation and real physical-mouse intervention.

Observed mixed-DPI overlay timings were approximately 57–59 FPS at a 60 FPS target. This is not a guaranteed display refresh rate. Secure-desktop switching, display disconnection during input, additional physical monitor combinations and clean non-developer-PC startup remain to verify. See the validation checklist.

## Install

Download the Windows x64 ZIP and `.sha256` companion. Verify with `Get-FileHash -Algorithm SHA256`, extract, install Node.js 20+ and configure your MCP client with the absolute extracted `client/dist/index.js` path. Rust/Python are not needed for the main portable server. Use `POINTHAND_ALLOW` to restrict apps and supervise the session.

## Join in

Code, documentation, creative examples and compatibility reports are welcome in Turkish or English. Read CONTRIBUTING.md and browse issues labeled `good first issue` or `help wanted`.

The application components retain their 0.2.0 base version; the Git tag identifies this alpha distribution. Alpha 1 remains available unchanged.
