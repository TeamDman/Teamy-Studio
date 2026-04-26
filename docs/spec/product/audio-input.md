# Audio Input

This specification covers the visible Teamy Studio microphone picker and selected-device slices.

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

audio[gui.legacy-recording-dialog]
The microphone picker must expose a shader-rendered gear button and hotkey that open the Windows legacy Recording Devices dialog, and the selected-device window must reuse that gear button.

audio[gui.keyboard-navigation]
The microphone picker must support keyboard navigation and selection in both pretty and diagnostics modes.

audio[gui.diagnostics-toggle]
The microphone picker must let the `Show diagnostics` chrome button and `Alt+X` toggle the same diagnostics mode.

audio[gui.diagnostics-tui]
The microphone picker diagnostics mode must render as a real TUI with blocks and selected-device colors, not as a plain debug-text dump.

audio[gui.selected-device-window]
Selecting a microphone must open a selected-device window that shows the microphone icon, name, endpoint id, state, and sample rate.

audio[gui.arm-for-record]
The selected-device window must expose a default-on recording control cluster with tooltip text, including the record button and a loopback toggle; recording must not start until the user activates recording, while loopback may start monitor-only capture without appending to the recorded buffer.

audio[gui.recording-state]
The selected-device window must let Enter and the record button start and stop recording from the chosen microphone, and its visible state text must read as `Recording` or `Not recording`.

audio[gui.record-arm-shader]
The selected-device window record button must be shader-rendered as a dull red circle when inactive and a subtle glowing pulsing red circle while recording, without a boxed clipping artifact.

audio[gui.audio-buffer-waveform]
The selected-device window must render the recorded audio buffer as a waveform with recording, playback, transcription, and selection heads, and the recording head must represent the write location used for append-style capture.

audio[gui.playback-transport]
The selected-device window must expose a shader-rendered play/pause button and let Space play or pause the recorded buffer, K pause playback, repeated J/L adjust backward or forward shuttle speed, and forward or reverse playback must behave sensibly when the playback head is already at the end of the buffer.

audio[gui.waveform-selection]
Clicking the waveform without meaningful drag must place the playback head, dragging past a small pixel tolerance must create a visible selection range, and the recording, playback, and transcription heads must also expose draggable grabbers that preserve grab offset, stack when they overlap, and show hover tooltips.

audio[gui.selected-device-diagnostics-tui]
The selected-device diagnostics mode must render as a ratatui application with microphone status, waveform chart, and shared transport hotkeys.