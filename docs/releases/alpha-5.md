# PointHand v0.2.0-alpha.5

An alpha for supervised Windows workflows. This download adds speaker loopback recording and tighter accessibility action labels.

## Highlights

- An accessibility line includes `actions="Invoke, Expand"` only for patterns that element can run. Scroll labels are left off at the end of that axis. Pass one of those labels to `perform_secondary_action`. Matching ignores case.
- `start_audio_recording` records what Windows is playing. It is speaker loopback, not a microphone, and other actions continue while it records. It does not wait for the pointer to be idle. `max_duration_ms` defaults to 60000 and cannot exceed 300000. One recording at a time.
- `stop_audio_recording` returns a 24 kHz stereo 16-bit WAV. The file path is on this computer. The MCP result carries one audio part and leaves the raw bytes out of the text. A quiet stretch stays in the file as silence. Physical Escape ends capture early; call stop to collect the partial recording.

## Validation and limits

Windows CI runs Rust, TypeScript, helper and native protocol checks, builds the executable, extracts the ZIP and verifies checksums, the executable manifest and isolated MCP startup. Local host tests (43) and client tests (19) passed before this tag. Duration limits, mix-format decode, resampling, the WAV header, and quiet-duration padding are unit-tested. A live loopback check recorded a playing system sound and a silent interval. Device capture is not part of CI, because a runner may have no audio output.

Secure-desktop switching, display disconnection during input, additional physical monitor combinations and clean non-developer-PC startup remain to verify. See the validation checklist.

## Install

Download the Windows x64 ZIP and `.sha256` companion. Verify with `Get-FileHash -Algorithm SHA256`, extract, install Node.js 20+ and configure your MCP client with the absolute extracted `client/dist/index.js` path. Rust/Python are not needed for the main portable server. Use `POINTHAND_ALLOW` to restrict apps and supervise the session. Restart a running PointHand so it loads this executable.

## Join in

Code, documentation, creative examples and compatibility reports are welcome in Turkish or English. Read CONTRIBUTING.md and browse issues labeled `good first issue` or `help wanted`.

The application components retain their 0.2.0 base version; the Git tag identifies this alpha distribution. Alpha 4, Alpha 3, Alpha 2 and Alpha 1 remain available unchanged.
