# Audio Input

This specification covers the first visible Teamy Studio microphone picker slice.

## CLI Inventory

audio[cli.audio-command]
The CLI must expose an `audio` command group.

audio[cli.input-device-command]
The `audio` command group must expose an `input-device` subcommand group.

audio[cli.input-device-list]
The `audio input-device` command group must expose a `list` subcommand that reports audio input devices through the standard CLI output pipeline.

## Windows Device Enumeration

audio[enumerate.active-windows-recording]
Audio input inventory must enumerate active Windows recording endpoints without starting capture.

audio[enumerate.endpoint-id]
Each audio input device entry must include the stable Core Audio endpoint id.

audio[enumerate.sample-rate]
Each audio input device entry should include the endpoint mix-format sample rate when Windows exposes it without starting capture.

audio[enumerate.windows-icon]
Each audio input device entry must include the Windows device icon resource path or a Windows microphone icon fallback resource.

## Picker Window

audio[gui.launcher-button]
The launcher must expose an `Audio Devices` image button.

audio[gui.picker-window]
The `Audio Devices` button must open a dedicated microphone picker window.

audio[gui.pretty-device-list]
The microphone picker must default to a pretty device list that shows the device name, icon, endpoint id, and sample-rate availability.

audio[gui.windows-icon-sprite]
The microphone picker should render microphone imagery from a Windows icon resource when the resource is available.

audio[gui.keyboard-navigation]
The microphone picker must support keyboard navigation and selection in both pretty and diagnostics modes.

audio[gui.diagnostics-toggle]
The microphone picker must let the `Show diagnostics` chrome button and `Alt+X` toggle the same diagnostics mode.

audio[gui.diagnostics-tui]
The microphone picker diagnostics mode must render as a real TUI with blocks and selected-device colors, not as a plain debug-text dump.

audio[gui.selection-dialog]
Selecting a microphone in the first slice must show a simple dialog reporting the selected microphone instead of opening a per-device workflow window.