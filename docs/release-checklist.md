# Release verification

The current version remains alpha. Do not label untested behavior complete.

The README release badge is `assets/release.svg`. It is served from this repository. Update its label when the release tag changes. Do not point it at shields.io: that service reads the GitHub API with a shared token pool and renders "Unable to select next GitHub token from pool" when the pool is empty.

- [x] Rust unit tests and release executable build.
- [x] TypeScript tests and MCP client build.
- [x] Native JSONL protocol, cancellation and shutdown regressions.
- [x] Real PointHand MCP input into the disposable fixture: click, move, drag, text and scroll.
- [x] Native overlay approximately 60 FPS in the fixture test.
- [x] Public Mona Lisa assets stripped of source paths and private image metadata.
- [x] Retest paced scrolling after timing changes.
- [x] Extended virtual-display mixed-DPI test: 1920 × 1080 at 125%, 2560 × 1440 at 100%, exact clicks.
- [x] Pixel-center normalization and negative-origin coordinate round-trip regression tests.
- [x] Live negative-origin placement on the virtual display: left=-2560, zero click/stroke endpoint error; original right-side layout restored.
- [ ] Additional physical-monitor combinations and display disconnection during input.
- [x] MCP emergency stop during a real held-button stroke: 67–141 ms observed locally, button released, queued input blocked.
- [x] Fail-closed monitor-loss policy tests; periodic desktop checks during input. Mouse movement does not cancel a started action.
- [x] Earlier Alpha 2 run: physical mouse movement during a held-button drawing returned `user_active` and released the button. Current builds keep drawing until Escape or `emergency_stop`.
- [ ] Secure-desktop switching during a long drawing.
- [x] Portable directory startup without source-tree dependencies.
- [x] Hosted Windows ZIP extraction, checksum and isolated-startup verification passed in CI (7125795).
- [ ] Manual startup on a non-developer Windows machine.
- [x] Turkish/English alias and pixel-unit checks, plus custom Paint selectors with ambiguity rejection.
- [x] Fresh local Turkish Paint color-dialog test: edit-colors, RGB update and confirmation via PointHand MCP.
- [ ] Paint tutorial on fresh Turkish and English installations.
- [x] GitHub destination: `ayzekhdawy/pointhand`; alpha publication authorized by the owner.

Build with `scripts/build-windows.ps1`, then run `scripts/package-windows.ps1` to create a portable directory and ZIP under ignored `artifacts/`. Node.js 20+ is required at runtime. Configure the MCP client to run `node` with the extracted absolute `client/dist/index.js` path. Rust and Python are not required for the main MCP server or bundled stroke plan.

Run `node scripts/native-input-e2e.mjs` only in an interactive desktop session: it opens and closes a disposable test window and moves the pointer. It never uses another computer-use provider.

After packaging, run `./scripts/verify-package.ps1` to verify the ZIP hash, binary manifest and isolated MCP startup. `node --test tests/js/*.test.mjs` covers the Paint selector helpers. The interactive native test now holds the mouse button during a long stroke and concurrently calls `emergency_stop`; it checks both the released event and Windows button state. This does not substitute for real physical-intervention or secure-desktop-switch testing.

See [extended-display validation](multimonitor-validation.md) for the Sunshine virtual-display test, its measured DPI/coordinate results and the remaining physical-hardware limits.

For a manual input check, run `node scripts/manual-physical-e2e.mjs` from a source checkout with Python/Tkinter installed. Keep the physical mouse still during setup. The drawing is expected to finish if the mouse moves. Escape, not mouse movement, cancels the active stroke and later requests until restart. The script closes its fixture and stores the report under ignored `.scratch/`.
