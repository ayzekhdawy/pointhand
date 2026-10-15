# PointHand input behavior

PointHand serves drawing, design and development workflows. Exact endpoints and interruptibility take priority over decorative movement.

`move`, coordinate/element `click`, and `scroll` use a minimum-jerk acceleration curve by default. Travel time depends on distance (up to 450 ms), with approximately 8 ms scheduling intervals. Actual event delivery depends on Windows scheduling and the target application. The final point is exact. This is a predictable motion model, not a claim of indistinguishability from human input.

These tools accept `motion: "instant"` for direct positioning or `motion: "natural"` for the default curve. Drawing and stroke geometry remain unchanged: no random coordinate jitter or wandering is added. Wheel input is paced in single notches, with slower intervals at the ends of a burst. Each axis is limited to 100 notches per call. Instant mode sends the wheel delta together. Drag and typing timing still need further refinement and validation.

Physical input interrupts the active action. Escape stops subsequent mutations. The native overlay follows the actual pointer and targets 60 FPS; UI smoothness still needs live testing on the user's display. Multiple monitors, mixed DPI and application-specific input handling require end-to-end checks before declaring the release complete.
