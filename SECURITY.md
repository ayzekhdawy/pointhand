# Security

PointHand is a local desktop-control service. Run it only for clients you trust and keep it on stdio. It is not a sandbox or a remote authentication service.

Use `POINTHAND_ALLOW` to limit input to selected executables. A full path matches that exact executable path; an executable name matches that name in any directory. For example, a Paint-only creative session can use `POINTHAND_ALLOW=mspaint.exe`. Leaving the variable empty allows other apps.

The native host pauses for physical-input idle before an action starts. Mouse movement and clicks do not interrupt an action that has already started. Physical Escape stops further mutations. Injected keys are distinguished from physical input. Input monitoring records counts and timestamps, not typed content. Clipboard and screenshot tools can still expose sensitive desktop data to your MCP client; application input permissions do not redact those outputs.

The overlay is excluded from capture by default and does not accept clicks. This is a capture behavior for PointHand's own UI, not protection for other desktop content. Secure-desktop input is refused.

Do not include `.scratch/`, environment files, logs, checkpoints or personal screenshots in public reports. Report suspected vulnerabilities through [GitHub private vulnerability reporting](https://github.com/ayzekhdawy/pointhand/security/advisories/new). Do not disclose reproduction data in public issues before coordinated review.

Alpha 2 exposes `emergency_stop` over MCP. It signals the native event outside the input queue and leaves the host stopped until restart. Active input also rechecks desktop availability at a target interval of 50 ms and refuses to continue if physical-input monitoring is lost. Scheduling delays and native UIA calls can affect response time; this is not a hard real-time guarantee.
