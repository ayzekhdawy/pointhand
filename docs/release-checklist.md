# Release verification

The current version remains alpha. Do not label untested behavior complete.

- [x] Rust unit tests and release executable build.
- [x] TypeScript tests and MCP client build.
- [x] Native JSONL protocol, cancellation and shutdown regressions.
- [x] Real PointHand MCP input into the disposable fixture: click, move, drag, text and scroll.
- [x] Native overlay approximately 60 FPS in the fixture test.
- [x] Public Mona Lisa assets stripped of source paths and private image metadata.
- [x] Retest paced scrolling after timing changes.
- [ ] Multiple monitors, negative coordinates and mixed DPI on actual hardware.
- [x] MCP emergency stop during a real held-button stroke: 111 ms locally, button released, queued input blocked.
- [x] Fail-closed monitor-loss and physical-intervention policy tests; periodic desktop checks during input.
- [ ] Actual physical intervention and secure-desktop switching during a long drawing.
- [x] Portable directory startup without source-tree dependencies.
- [ ] Hosted Windows ZIP extraction, checksum and isolated-startup verification (new CI step).
- [ ] Manual startup on a non-developer Windows machine.
- [x] Turkish/English alias and pixel-unit checks, plus custom Paint selectors with ambiguity rejection.
- [x] Fresh local Turkish Paint color-dialog test: edit-colors, RGB update and confirmation via PointHand MCP.
- [ ] Paint tutorial on fresh Turkish and English installations.
- [x] GitHub destination: `ayzekhdawy/pointhand`; alpha publication authorized by the owner.

Build with `scripts/build-windows.ps1`, then run `scripts/package-windows.ps1` to create a portable directory and ZIP under ignored `artifacts/`. Node.js 20+ is required at runtime. Configure the MCP client to run `node` with the extracted absolute `client/dist/index.js` path. Rust and Python are not required for the main MCP server or bundled stroke plan.

Run `node scripts/native-input-e2e.mjs` only in an interactive desktop session: it opens and closes a disposable test window and moves the pointer. It never uses another computer-use provider.

After packaging, run `./scripts/verify-package.ps1` to verify the ZIP hash, binary manifest and isolated MCP startup. `node --test tests/js/*.test.mjs` covers the Paint selector helpers. The interactive native test now holds the mouse button during a long stroke and concurrently calls `emergency_stop`; it checks both the released event and Windows button state. This does not substitute for real physical-intervention or secure-desktop-switch testing.
