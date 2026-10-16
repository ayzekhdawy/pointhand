# Extended-display validation

`get_system_info` now reports physical-pixel monitor/work-area rectangles and effective DPI. Window metadata also includes `dpi`. These fields are available in Alpha 2.

The live test used an extended Windows desktop with a physical 1920 × 1080 primary display at 125% (120 DPI) and a 2560 × 1440 virtual secondary display at 100% (96 DPI), streamed to a phone through Sunshine. The virtual desktop measured 4480 × 1440. A second run temporarily placed the virtual display to the left at [-2560, 0, 0, 1440], then restored its original right-side position [1920, 0, 4480, 1440].

PointHand MCP moved its disposable fixture by dragging the title bar. The window DPI changed from 120 to 96. Both left-side and right-side runs confirmed exact physical-pixel clicks and stroke endpoints, text entry and a -240 wheel delta on the secondary display. The native overlay remained ready with no renderer error; streamed runs averaged approximately 57–59 FPS at a 60 FPS target. The renderer now uses its own waitable timer (high resolution when available), independently of input cancellation; these measurements do not establish a guaranteed 60 FPS. Streaming and scheduling latency are separate from PointHand's render timing.

This test found a one-pixel coordinate error. Pointer movement and strokes now share native SendInput normalization that targets each pixel's center in the 65536-bin absolute range. Regression tests also cover negative virtual origins and edge coordinates. Focus is confirmed against the actual foreground window or its owned popup before input, with bounded retries when activation is delayed.

To repeat from a source checkout on an extended desktop:

```powershell
node scripts/native-multimonitor-e2e.mjs
```

The fixture requires Python with Tkinter; the main MCP server does not. The script opens, moves and closes only its own fixture. It does not reconfigure the displays. It enables overlay capture in that test instance so a streamed display can show the renderer. Reports and screenshots stay under ignored `.scratch/`.

This establishes a virtual-display mixed-DPI scenario, not certification of every physical adapter or monitor combination. Additional physical second-monitor hardware and display disconnection during an action remain to test. The live negative-origin test passed on the virtual display; it does not cover every possible display topology.

A separate manual test on the right-side secondary display passed with actual user mouse movement during a held-button drawing. The physical hook observed 24 events, the active action returned `user_active`, and both the fixture release event and Windows button-state check confirmed that the left button was released. Setup attempts interrupted before drawing were rejected and do not count as this result. This verifies active-action intervention; secure-desktop switching remains untested.
