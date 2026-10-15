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
- [ ] Physical intervention and secure-desktop failure during a long drawing.
- [x] Portable directory startup without source-tree dependencies.
- [ ] Clean-machine ZIP extraction and checksum verification.
- [ ] Paint tutorial on a fresh setup; current UI selectors require Turkish Paint.
- [x] GitHub destination: `ayzekhdawy/pointhand`; alpha publication authorized by the owner.

Build with `scripts/build-windows.ps1`, then run `scripts/package-windows.ps1` to create a portable directory and ZIP under ignored `artifacts/`. Node.js 20+ is required at runtime. Configure the MCP client to run `node` with the extracted absolute `client/dist/index.js` path. Rust and Python are not required for the main MCP server or bundled stroke plan.

Run `node scripts/native-input-e2e.mjs` only in an interactive desktop session: it opens and closes a disposable test window and moves the pointer. It never uses another computer-use provider.
