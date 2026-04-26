# Audio Input

This specification covers the visible Teamy Studio microphone picker and selected-device slices.

## CLI Inventory

audio[cli.audio-command]
The CLI must expose an `audio` command group.

audio[cli.input-device-command]
The `audio` command group must expose an `input-device` subcommand group.

audio[cli.input-device-list]
The `audio input-device` command group must expose a `list` subcommand that reports audio input devices through the standard CLI output pipeline.

audio[cli.daemon-command]
The `audio` command group must expose a `daemon` subcommand group for the local transcription backend.

audio[cli.daemon-status]
The `audio daemon` command group must expose a `status` subcommand that reports the resolved Python daemon source, cache paths, and tensor handoff contract.

## Python Transcription Boundary

audio[python.daemon-project]
Teamy Studio must include a Teamy-owned Python daemon project for WhisperX integration rather than depending on an external checkout.

audio[transcription.log-mel-contract]
Rust must represent the Whisper transcription handoff as a fixed 80 x 3000 little-endian f32 log-mel tensor contract.

audio[transcription.shared-memory-payload]
The Python transcription boundary must treat each inference request payload as a Rust-owned shared-memory slot containing exactly one fixed-shape log-mel tensor.

audio[transcription.shared-memory-slot-pool]
Rust must manage an elastic shared-memory slot pool that writes fixed log-mel tensor payloads, queues ready requests for Python, and releases slots for reuse after Python returns ownership.

audio[transcription.named-pipe-control-protocol]
Rust and Python must share a versioned JSONL control-message protocol for named-pipe requests and results, including the shared-memory slot name, tensor contract, request id, result text, and slot-release instruction.

audio[transcription.live-named-pipe-transport]
Rust must provide a live Windows named-pipe transport that sends one queued transcription control request to the daemon and validates one returned result line.

audio[transcription.result-staging]
Rust must consume transcription daemon results by releasing returned shared-memory slots and staging successful transcript text in the microphone transcript island state.

audio[transcription.debug-runtime-tick]
When transcription is enabled in the microphone window, Rust must be able to run a nonblocking debug transcription tick that submits a placeholder log-mel tensor to the Python pipe path and stages the returned text in the transcript island.

audio[transcription.cached-preview]
The microphone transcription preview must cache spectrogram intensity and energy calculations outside the render-only path so focused-frame redraws can reuse the latest computed preview.

audio[transcription.manual-flush]
The microphone window must provide a manual flush control that sends the current transcription chunk without waiting for a future full-duration chunk boundary, and the UI must indicate chunk duration, energy, send state, and completed request id.

audio[transcription.shared-memory-pool-status]
Rust must expose the initial shared-memory slot-pool sizing and live queue counters that the CLI and GUI can report before Python inference is started.

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

audio[gui.daemon-button]
The launcher must expose an `Audio Daemon` image button for inspecting the local transcription backend.

audio[gui.daemon-window]
The `Audio Daemon` button must open a dedicated GUI window that shows the daemon source, cache paths, transport contract, and shared-memory slot-pool status.

audio[gui.daemon-diagnostics-tui]
The audio daemon diagnostics mode must render as a full ratatui view with transport, payload, filesystem, and live-flow sections.

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

audio[gui.transcription-toggle]
The selected-device window must expose a shader-rendered transcription toggle that can enable or disable staged transcription preparation without automatically sending text to another application.

audio[gui.mel-spectrogram-preview]
When transcription preparation is enabled, the selected-device window must render a mel-spectrogram preview surface derived from recorded audio ahead of the transcription head.

audio[gui.transcription-terminal-island]
The selected-device window must reserve a terminal-styled island below the audio buffer and mel preview where staged transcript text appears.

audio[gui.waveform-selection]
Clicking the waveform without meaningful drag must place the playback head, dragging past a small pixel tolerance must create a visible selection range, and the recording, playback, and transcription heads must also expose draggable grabbers that preserve grab offset, stack when they overlap, and show hover tooltips.

audio[gui.selected-device-diagnostics-tui]
The selected-device diagnostics mode must render as a ratatui application with microphone status, waveform chart, and shared transport hotkeys.