# Security

PointHand is a local desktop-control service. Run it only for clients you trust and keep it on stdio. It is not a sandbox or a remote authentication service.

Use `POINTHAND_ALLOW` to limit input to selected executables. A full path matches that exact executable path; an executable name matches that name in any directory. For example, a Paint-only creative session can use `POINTHAND_ALLOW=mspaint.exe`. Leaving the variable empty allows other apps.

The native host pauses for physical-input idle and interrupts the current action when the user intervenes. Physical Escape stops further mutations. Injected keys are distinguished from physical input. Input monitoring records counts and timestamps, not typed content. Clipboard and screenshot tools can still expose sensitive desktop data to your MCP client; application input permissions do not redact those outputs.

The overlay is excluded from capture by default and does not accept clicks. This is a capture behavior for PointHand's own UI, not protection for other desktop content. Secure-desktop input is refused.

Do not include `.scratch/`, environment files, logs, checkpoints or personal screenshots in public reports. Report a suspected security issue privately to the repository maintainer before posting reproduction data publicly. A dedicated private reporting channel has not yet been configured.
