# Operating System Integration

This specification describes the Windows-specific behaviors and platform quirks that Teamy Studio intentionally relies on to deliver a low-latency notebook shell experience.

## Windows Shell Resolution

os[shell.default.fallback.windows-comspec]
On Windows, the built-in default shell command must use `COMSPEC` when it is set or `cmd.exe` otherwise, and it must include `/D`.

os[shell.default.windows-launch-resolves-program-on-path]
On Windows, when the configured default shell program is a bare executable name without path separators, Teamy Studio must resolve it through `PATH` and `PATHEXT` before launching it inside the PTY-backed window.

## Windows Presentation Model

os[window.appearance.translucent]
The launched terminal window must use layered-window alpha so the shell surface remains translucent.

os[window.appearance.os-chrome-none]
The launched terminal window must not show OS-managed chrome or decorations such as the title bar, icon, caption buttons, or user-preference-colored window borders.

os[window.interaction.drag.threshold]
The frameless drag strip must support a zero-pixel drag threshold so the native window move loop can begin with no deadzone when that behavior is configured.

os[window.interaction.resize.native-edges]
The launched terminal window must still resize from its edges and corners using native OS hit-testing semantics and native resize cursors even though OS-managed chrome is hidden.

os[window.interaction.clipboard.multiline-paste-confirmation.native-dialog]
The multiline paste confirmation flow should use a native Windows dialog positioned relative to the Teamy Studio window rather than a custom in-client prompt.

os[window.taskbar.progress.osc-9-4]
When the terminal emits supported `OSC 9;4` progress sequences, the Teamy Studio window must mirror that state into native Windows taskbar progress.

## Windows Implementation Direction

os[os.windows.rendering.direct3d12]
The app should render its notebook shell through Windows-native Direct3D 12 presentation paths so panel composition and shader effects can be controlled with low latency.

os[os.windows.rendering.direct3d12.offscreen-terminal-verification]
The Direct3D 12 renderer must support rendering terminal frames to an offscreen target that can be read back for automated verification without presenting to a visible window.

The intent of this document is to make Windows-specific assumptions explicit. These rules are not claims of cross-platform portability; they document the platform hooks and rendering choices that currently define the product experience.