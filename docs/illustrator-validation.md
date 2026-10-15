# Illustrator validation

The interactive test launched a closed Adobe Illustrator 2025 installation through PointHand `list_apps` and `launch_app`. It created a new 600 × 600 document and drew two closed shapes with actual Pen-tool mouse clicks, set CMYK values through UI Automation and added an Arial Bold wordmark. No SVG import, bitmap paste, JSX or Illustrator scripting was used to create the document.

The result was saved through Illustrator's Save dialog as a local `.ai` document. Its PDF-compatible page was independently opened and rendered to verify the saved result. Local test evidence is under ignored `.scratch/logo-test/`; it is not included in public packages.

This test exposed a focus bug: repeated activation of the main HWND could take keyboard focus away from its dialog. The native `focus` function now keeps an already-active target or its owned popup active. Chords also include a short key-down interval before releasing keys.

The native Illustrator drawing surface and several dialogs expose incomplete UIA information. They still require screenshot inspection and measured coordinates. CMYK edits exposed by UIA were reliable; hexadecimal entry attempts required a fallback to those CMYK fields. This is not proof that every Illustrator feature is semantically accessible or that workflows work behind an obscuring window.

Real mouse drawing requires the target to receive desktop input. Background UIA actions depend on the control's supported patterns. A separate interactive session or virtual machine is the recommended architecture for concurrent desktop use; that mode is not implemented in this release.
