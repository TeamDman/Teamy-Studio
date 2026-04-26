# Audio Input Inbox Plan

## Goal

Improve Teamy Studio by replacing the unsafe external voice-to-text workflow with a Teamy-owned audio input path whose first visible product is a safe transcription inbox.

The next slice is deliberately smaller than a full Rust WhisperX rewrite. It should let Teamy Studio enumerate recording devices, expose them through a visible `Audio Devices` main-menu flow, let the user choose one through a hybrid terminal/native graphical flow, and provide the foundation for a controlled text buffer where future transcriptions land before anything is sent to another app.

The UI direction is hybrid from the start. Teamy Studio should keep a real terminal TUI for users who invoke audio tools from a terminal, while the main GUI should offer an `Audio Devices` button that opens a dedicated mic picker window. That window should reuse the same interaction/state logic as the TUI, but default to a pretty graphical presentation. Its existing `Show diagnostics` chrome button should toggle between the pretty view and the TUI/diagnostic view, similar in spirit to the cursor-info virtual-session path. `Alt+X` should activate the same diagnostics/mode toggle by keyboard.

The product rule is simple: dictated text must never be sprayed into whichever external window happens to have focus. Teamy Studio may later offer explicit copy, paste, IME, or send-to-target actions, but automatic OS key injection is not the default data path.

## Current Status

- Done so far:
  - Read the narrated design note without modifying it.
  - Read Teamy Studio repo instructions in `AGENTS.md`.
  - Confirmed Tracey is the repo's observable behavior contract through `.config/tracey/config.styx`.
  - Reviewed the current product specs in `docs/spec/product/behavior.md`, `docs/spec/product/windowing.md`, and `docs/spec/product/cli.md`.
  - Confirmed the launcher and audio picker already exist in `src/app/windows_scene.rs` and `src/app/windows_app.rs`.
  - Confirmed the current `Audio` launcher action is a bell-source picker with `Windows` and `Pick File`, backed by `src/app/windows_audio.rs`.
  - Confirmed `cursor-info` has already moved beyond its older plan: it exists as a CLI command and launcher action.
  - Read the current Python narration tool at `d:\Repos\ml\voice2text\transcribe_hotkey_typer.py` and extracted its useful behavior: hard-coded microphone discovery, push-to-talk, toggle listening, API-controlled listening, websocket result streaming, WhisperX transcription, and fallback typewriter delivery through `pyautogui`.
  - Grilled the Rust/Python boundary for the next implementation: Python should become only a long-lived WhisperX inference daemon, while Rust owns capture, resampling, activation state, buffering, chunking, log-mel feature creation, routing, and inbox delivery.
  - Confirmed from WhisperX and Burnt Apple code that the first supported model family uses 16 kHz mono audio, 30-second Whisper windows, 80 mel bins, 3000 frames, and 240,000 little-endian `f32` feature values per inference tensor.
  - Chose a from-scratch Windows IPC shape: named pipes for control messages and Rust-owned shared-memory slots for tensor payloads.
  - Decided the first implementation slice must include real Windows recording-device enumeration. Fake providers are useful for tests, but fake-only CLI plumbing is not enough vertical progress.
  - Identified reusable microphone enumeration and icon-loading logic in `youre-muted-btw/crates/mic_detection_plugin`: `mic_list.rs`, `mic_icon.rs`, `icon_path.rs`, `load_icon_from_dll.rs`, and `hicon_to_image.rs`.
  - Decided the recording-device inventory CLI should be named `audio input-device list` rather than `audio input list` to avoid confusing device inventory with future audio-input flows.
  - Decided CLI `audio input-device list` should not dump raw icon data, but the first GUI slice should include microphone imagery in the main-menu button and microphone icons in the mic picker.
  - Decided the first `audio input-device list` slice should enumerate active Windows recording devices only. Disabled, unplugged, and not-present devices should be handled when selected-device persistence and stale-selection diagnostics are added.
  - Decided the first stable recording-device id should be the Windows Core Audio endpoint id returned by `IMMDevice::GetId()`.
  - Decided `audio input-device list` should expose verbose debug metadata for discovered devices in JSON/CSV output. When stdout is a terminal and no explicit output format is requested, it should render readable text instead.
  - Decided the audio-device UI should be hybrid from the start: terminal invocation keeps TUI behavior, while the GUI main menu gets an `Audio Devices` button that opens a dedicated window with pretty mode by default and a `Show diagnostics` chrome toggle to reveal the TUI/diagnostic view.
  - Decided TUI logic should be reusable and mode-aware so waveform, spectrogram, and chooser views can render as ratatui/cell-grid views in terminal mode and as richer native graphics in pretty window mode.
  - Decided the first visible GUI slice is all about the mic picker window: it includes the `Audio Devices` main-menu button with an image, a mic picker window listing name/icon/id/sample rate, keyboard navigation in pretty and TUI modes, `Alt+X` to toggle diagnostics/modes, and a storage-style dialog showing the picked microphone or microphones. The per-device audio-device window and `arm for record` control are deferred.
  - Implemented the selected-microphone window slice: choosing a microphone now opens a dedicated microphone window with icon, name, endpoint id, state, sample rate, and a default-on arm-for-record icon button with tooltip text. This remains a no-capture UI control.
  - Added Windows legacy recording-device integration: the microphone picker has a shader-rendered gear button and hotkey for the legacy Recording Devices dialog, and the selected-microphone window reuses the gear button. Per-row properties buttons were removed because there is no supported direct jump to a specific Core Audio endpoint properties page.
  - Implemented the first selected-microphone recording slice: Enter and the shader record button start/stop WASAPI capture from the chosen endpoint, the microphone page renders an audio-buffer waveform with recording/playback/transcription/selection heads, Space plays the captured buffer from a generated WAV handoff, J/K/L update transport speed state, click places the playback head, drag creates a visible selection, and diagnostics mode now renders a ratatui microphone dashboard with a chart-backed waveform.
  - Fixed the first real smoke-test issues in the recording slice: playback generation now starts from the current playback head, pause explicitly stops active WinMM playback, J/L generate crude fast/reverse shuttle buffers, and the pretty/TUI waveforms normalize amplitude to available vertical space and render full-width bars to avoid periodic visual gaps.
  - Hardened the microphone-detail transport slice after smoke testing: forward playback at end-of-buffer now restarts cleanly, reverse playback can shuttle backward from the end, recording appends instead of clobbering, and the recording head now represents the active write position.
  - Added microphone-detail control polish: the page now has a separated loopback toggle, draggable stacked grabbers for the recording/playback/transcription heads, hover tooltips for those heads, and a refined recording shader that no longer looks boxed in.
  - Wired the loopback toggle into the capture path so microphone audio can be monitored through the default render endpoint even when recording is not active; when used outside recording, the monitor session does not append to the recorded buffer.
  - Validated the recording slice with `./check-all.ps1` on 2026-04-25: format, clippy, build, tests, and Tracey passed. Tracey reported `teamy-studio-audio-input/rust: 23 of 23 requirements are covered. 11 of 23 have a verification reference.`
  - Captured the current Tracey status on 2026-04-25: all tracked requirements are covered. Verification remains partial: behavior 31/56, cli 26/44, convention 0/4, os 6/10, publishing 0/8, tool-standards 22/28, windowing 11/16.
  - Observed `tracey query unmapped` still reports broad repo-wide mapping debt, so new work should add explicit requirement references for touched code instead of trying to solve all historical mapping debt in this slice.
  - Added the first hosted transcription surface inside the selected-microphone window: a shader-rendered transcription toggle, a mel-spectrogram preview area driven by recorded audio ahead of the transcription head, and a terminal-styled transcript island below the audio buffer.
  - Validated the transcription UI shell with `./check-all.ps1`: format, clippy, build, tests, and Tracey passed. Tracey reported `teamy-studio-audio-input/rust: 26 of 26 requirements are covered. 14 of 26 have a verification reference.`
  - Started the Python integration slice: added a Rust `WhisperLogMel80x3000` payload contract, exposed `audio daemon status`, added a Teamy-owned `python/whisperx-daemon` scaffold, and changed the default Teamy window size to 1300x900.
  - Added the first daemon GUI surface: the main menu now has an `Audio Daemon` card, the daemon opens as a dedicated scene window, and diagnostics mode renders a ratatui status view over the Python entrypoint, cache paths, tensor payload contract, shared-memory slot-pool sizing, and queue counters.
  - Added the first real Rust-side shared-memory slot pool: it creates Windows file mappings, writes fixed `WhisperLogMel80x3000` payloads, queues ready requests for Python, elastically allocates an extra slot when every slot is queued, and releases slots for reuse.
  - Added the Rust/Python control-message contract for the future named pipe: Rust serializes queued shared-memory requests as versioned JSONL, Rust parses daemon result lines, and the Python daemon scaffold validates matching requests and emits slot-release debug results.
- Current focus:
  - Continue from the shared JSONL control-message contract into a live named-pipe transport and Python daemon slot validation.
- Remaining work:
  - Harden the first capture/playback path after more real-hardware smoke testing, especially for loopback latency, render-format mismatches, and longer recordings.
  - Replace the current mel-preview visualization with the same log-mel feature data that will be sent to Python.
  - Add the live named-pipe transport for request/result/slot release.
  - Add the Teamy-owned Python WhisperX daemon project and validation path.
  - Feed returned transcript chunks into the hosted transcript island without sending them to the OS focus target.
- Next step:
  - Add a live named-pipe transport that sends queued shared-memory slot requests to Python and accepts result/slot-release responses.

## Why This Slice

The narration contains several tempting directions: a file explorer, a text editor, a generalized action grammar, command palette work, recording timelines, richer Tracey-style captures, and a Rust transcription stack.

The best next slice is the audio input inbox because it satisfies four constraints at once:

- It addresses a concrete failure observed during the narration: transcribed text was typed into VS Code's Explorer instead of the editor.
- It uses an existing Teamy surface: the launcher already has an Audio action and a picker window.
- It advances the shared choice/action model without requiring the whole command palette or multiplayer focus system first.
- It creates the place where later Whisper, waveform, recording, and AI-choice work can attach safely.

This makes it a vertical product slice rather than a pure architecture exercise.

## Existing Python Narrator Baseline

The current narration workflow lives outside Teamy Studio in `d:\Repos\ml\voice2text\transcribe_hotkey_typer.py`. Treat it as the compatibility baseline for behavior worth preserving, not as code to port line-for-line.

Current behavior:

- The microphone worker repeatedly enumerates `speech_recognition.Microphone.list_microphone_names()` and waits until a device named `Microphone (WOER)` appears.
- Once the desired microphone exists, it opens `speech_recognition.Microphone(sample_rate=16000, device_index=...)` and blocks in `Recognizer.listen` on an executor thread.
- Audio is only queued for transcription when either keyboard-controlled listening or API-controlled listening is active.
- The keyboard worker uses F23 as push-to-talk and Pause as a listening toggle.
- Manual keyboard activation clears API listening state.
- The transcription worker loads WhisperX with `large-v2` on CUDA or `small.en` on CPU, then emits full WhisperX result objects containing segments.
- If an API key is configured, a local HTTPS web server exposes `POST /start_listening`, `POST /stop_listening`, `GET /results`, and `GET /`.
- API transcription results are sent to connected websocket clients while API listening is active.
- If API listening is inactive, transcription results are routed to a typewriter queue.
- The main loop joins segment text and sends it with `pyautogui.typewrite`, which means the text goes to whatever OS surface currently has focus.
- Websocket clients send `keepalive`; API listening is automatically disabled when no active websocket remains.
- Microphone unplug/replug is handled by retrying the hard-coded device discovery loop, but failures are only visible through logs.

Behavior to preserve in Teamy Studio:

- Separate keyboard/manual listening state from API/client-controlled listening state.
- Support push-to-talk and toggle-style activation.
- Support client-driven start/stop and streamed transcription results.
- Automatically stop client-driven listening when no client is attached.
- Keep recovering from missing or unplugged microphones.

Behavior to change in Teamy Studio:

- Replace hard-coded microphone name matching with persisted device selection and visible stale-device diagnostics.
- Replace blind `pyautogui.typewrite` delivery with staged inbox output by default.
- Replace log-only microphone failures with observable UI and CLI state.
- Treat model loading, capture, transcription, routing, and delivery as separately inspectable states.

## Inference Boundary Decisions

These decisions were made during the 2026-04-25 design grilling pass.

- Python's long-term role is to hold the WhisperX model in memory and perform model inference only.
- Rust should not call WhisperX's high-level `model.transcribe(audio)` path as the target architecture because that path performs VAD, chunking, feature preparation, tokenization policy, and other pipeline work inside Python.
- The first supported real model path is `large-v2` on CUDA only. CPU fallback is intentionally out of scope for the real daemon.
- The first model scope is 80-mel Whisper models such as `large-v2` and `small.en`. 128-mel models are out of scope for the first slice.
- Rust sends fixed-shape log-mel feature tensors to Python, not raw waveform chunks.
- Rust must represent the feature payload with strong newtypes so the language boundary is unambiguous. The first newtype should represent exactly `80 x 3000` `f32` values.
- Python must validate received tensor dtype, byte length, dimensions, and alignment before inference.
- The normal feature payload is exactly 240,000 `f32` values, or 960,000 bytes, in little-endian order.
- The first result payload should be a full debug summary, but not raw logits or raw encoder tensors. It should include stable summaries such as request id, slot id, model metadata, tensor shape, checksums, token ids, transcript text, segment timing when available, and timing metrics.
- The Python daemon should use a custom wrapper copied from WhisperX's `generate_segment_batched` shape so it can return token ids, decoded text, timings, and model metadata instead of only decoded text.
- Rust owns microphone capture, native-format normalization, downmixing, resampling, energy pause detection, buffering, log-mel feature computation, output routing, and inbox delivery.
- Rust should accept the microphone's native WASAPI format and normalize it to 16 kHz mono `f32` before detection and feature extraction.
- Resampling should sit behind a Rust trait or seam. A simple first-pass implementation is acceptable if needed, but the design must allow replacing it with a proven resampler.
- Rust should preserve the current Python recognizer defaults for the first energy detector: fixed energy threshold `300`, pause threshold `0.8s`, and dynamic energy disabled.
- The real WhisperX validation belongs behind a `self-test audio-transcription` subcommand that `./check-all.ps1 -Full` can run. The default validation path should not require CUDA/model availability unless `-Full` is explicitly requested.
- The managed Python daemon source should live inside Teamy Studio.
- The daemon environment should be a Teamy-owned `uv` Python project.
- The installed `teamy-studio.exe` must not depend on a checked-out copy of the Teamy Studio repo to run Python transcription. In development, the daemon project can resolve from the checkout; in packaged builds, the daemon project/source must resolve from Teamy-installed or bundled resources.
- The `uv` virtual environment should be runtime state created and managed by Teamy Studio under the resolved Teamy cache home, for example a daemon-specific child directory such as `<cache-home>/python/whisperx-daemon/.venv`. It is disposable and rebuildable, not committed source.
- WhisperX/model downloads should also resolve under Teamy-managed cache state, in a separate model-cache directory from the virtual environment, so `audio daemon status` can report both paths independently.
- Teamy should expose `audio daemon doctor` and setup/check commands. The quick doctor verifies the Python environment, package imports, and `torch.cuda.is_available()`. A full doctor additionally loads `large-v2`.
- `audio daemon status` should include both Python process/model readiness and Rust-side shared-memory pool metrics: slot count, total bytes, queued request count, oldest queued age, and Python lag. It must also show the resolved `uv` virtual environment path and the model download/cache path used by the daemon.
- `./check-all.ps1 -Full` should run `audio daemon doctor --full` before `self-test audio-transcription` so setup/model/CUDA failures are separated from pipeline correctness failures.

## Tensor IPC Direction

Use named pipes for control messages and shared memory for feature tensors.

Rust owns shared-memory slot lifecycle:

- Rust creates a slot.
- Rust writes a typed `WhisperLogMel80x3000` tensor into the slot.
- Rust sends a named-pipe control message that identifies the slot and request metadata.
- Python maps the slot, validates the tensor contract, runs inference, and returns a debug result over the named pipe.
- Python returns ownership of the slot to Rust after it has finished reading.
- Rust reuses returned slots instead of allocating new ones.

Slot allocation policy:

- The slot pool has a lower bound of three slots: one being written, one waiting to be read, and one being read by Python.
- The pool is elastic and intentionally unbounded. If Python falls behind, Rust keeps allocating new slots so microphone data is not lost by policy.
- The implementation must expose slot count, total shared-memory bytes, oldest queued request age, and Python lag in diagnostics because unbounded growth must be observable.

The phrase "ring buffer" should be avoided for this design. The intended structure is an elastic shared-memory slot pool plus an ordered ready queue.

## Constraints And Assumptions

- Do not edit `docs/notes/So trying to work on this studio softwar.md`; treat it as raw captured source material.
- Use `./check-all.ps1` for repo validation, not direct `cargo check`.
- New CLI subcommands must follow the repo rule: each subcommand gets its own directory module and its own `*_cli.rs` implementation file re-exported by `mod.rs`.
- The existing bell-source picker is useful but semantically different from recording-device selection. Do not overload `BellSource` with microphone concepts.
- Enumerating recording devices must not start capture, light an LED, or create a privacy-sensitive recording session.
- Reading display metadata for recording devices, including endpoint id, friendly name, icon, and sample rate, must not start capture, light an LED, or create a privacy-sensitive recording session.
- The first inbox can hold text typed manually or provided through test hooks before real ML transcription exists.
- Sending text to another application is a separate explicit action and should have a visible target or confirmation model.
- The app already has a scene-window diagnostics model. New audio-input windows should be able to explain their current choices through diagnostics text.
- The audio-device UI should be hybrid: real terminal invocation uses TUI behavior, while Teamy window invocation uses a pretty graphical presentation by default and can switch to a TUI/diagnostic view through the existing `Show diagnostics` chrome button.
- The logic that powers the TUI must be reusable and mode-aware. It should own interaction state, focus/selection, actions, and view-model data independently from the renderer so a terminal renderer and a pretty native renderer can consume the same state.
- The first implementation should prefer local Rust/Win32/Core Audio plumbing over depending on the existing Python transcriber.
- The existing Python tool proves the desired control model, but its `pyautogui` typewriter path is the safety bug this plan is meant to remove.
- The first real speech validation should use external VCTK files through a local generated manifest rather than committing audio or transcript corpus files to the repo. The manifest should contain paired `wav48` and `txt` paths. Candidate pair: `p230_397.wav` with transcript `p230_397.txt`, whose text is `Is there a waiting list ?`.
- The first `audio input-device list` implementation must enumerate real Windows recording devices. Test fakes should support deterministic parser and formatter coverage, but they are not the implementation milestone by themselves.
- The first `audio input-device list` implementation may list active devices only. Broader inactive-state enumeration belongs with selected-device persistence because that is when stale selections become user-visible behavior.
- Recording-device identity should use the Core Audio endpoint id as the first stable id. Display names are user-facing labels only and must not be used as persisted identifiers.
- The first `audio input-device list` report should favor observability over polish. Structured output should include endpoint id, display name, default-device flags, state, backend/source, role information when available, and discovered Windows property debug data. When stdout is a terminal and no output format is requested, text output should be the default and may summarize the same information in a readable form. When stdout is not a terminal and no output format is requested, JSON should be the default.
- Reuse the proven icon-discovery shape from `youre-muted-btw` in the first GUI slice by adapting it into Teamy-owned code rather than making installed Teamy Studio depend on that checkout. The relevant source logic reads Core Audio device properties for icon paths, falls back to `@%SystemRoot%\system32\mmres.dll,-3012`, loads icons from DLL or `.ico` sources, and converts `HICON` to RGBA.
- The first GUI slice should surface each active microphone's current/default sample rate when Windows exposes it without starting capture. If the sample rate cannot be resolved without crossing the no-capture boundary, the UI should display an explicit unknown/unavailable value rather than inventing one.

## Product Requirements

### Committed Requirements

- Teamy Studio must distinguish bell output settings from audio input devices.
- Teamy Studio must expose a recording-device inventory that can list currently known input devices without beginning capture.
- Each recording device must have a stable internal id, a display name, and enough metadata to tell default, active, disabled, and disconnected states apart when the OS exposes them.
- The user must be able to choose a recording device before a future recording or transcription session starts.
- Teamy Studio must provide a transcription inbox surface where recognized text is staged inside Teamy Studio.
- The transcription inbox must not automatically type text into the currently focused external app.
- The inbox must expose explicit actions for later text movement, such as copy, clear, append chunk, and eventually send-to-target.
- Teamy Studio must maintain a terminal TUI path for audio-device and audio-input workflows when invoked from a real terminal.
- Teamy Studio must provide a graphical audio-device flow from the main GUI through an `Audio Devices` button.
- The `Audio Devices` main-menu button must include an image.
- The graphical audio-device flow must open a dedicated mic picker window that defaults to a pretty presentation and uses the `Show diagnostics` chrome button to toggle to the TUI/diagnostic presentation.
- The mic picker must support keyboard navigation in both pretty mode and TUI/diagnostic mode.
- `Alt+X` must activate the same action as the `Show diagnostics` chrome button, toggling between pretty mode and TUI/diagnostic mode.
- The microphone picker window must list active microphones with name, icon, id, and sample rate when available.
- Selecting a microphone in the first slice must show a simple storage-button-style dialog that reports which microphone or microphones were picked.
- A later slice must add an audio-device window for the selected microphone with icon, name, id, sample rate, and recording controls.
- TUI render logic and pretty render logic must share the same mode-aware interaction/state model rather than diverging into separate behavior implementations.
- Diagnostics for the audio input picker and inbox must include the selected device, armed/listening state, and output routing state.
- Teamy Studio must model manual listening and client-controlled listening as distinct states.
- Teamy Studio must support push-to-talk and toggle activation without requiring the transcription backend to be running first.
- Teamy Studio must support a future local client API that can start listening, stop listening, and receive staged transcription results.
- Client-controlled listening must stop automatically when no interested client remains attached.
- Missing or unplugged selected devices must be visible as recoverable state rather than only as logs.

### Deferred Requirements

- Advanced real-time waveform and spectrogram rendering before capture exists.
- WASAPI capture and resampling.
- WhisperX or Burn-based model inference.
- Native Rust Whisper model inference.
- Speaker diarization, timestamps, and segment editing.
- IME integration or OS-sponsored text composition.
- Global hotkeys for arm, pause, snooze, and censor.
- Screen, mouse, keyboard, and audio timeline synthesis.
- Automatic insertion into another app as a default behavior.
- Compatibility with the exact existing local HTTPS/websocket API shape.
- A full generalized command palette and AI picker over every Teamy action.

These are important, but they should attach to the inbox after the safe staging model exists.

## Architectural Direction

Build the slice in six layers.

1. Audio input inventory

  Add value types for recording devices and an OS-backed enumerator. Keep this separate from `BellSource` and the bell preview code. The inventory should be callable from CLI, tests, and the native scene layer. The first inventory should include endpoint id, display name, default flags, icon data/source for GUI use, and sample-rate metadata when Windows exposes it without starting capture.

2. Choice presentation

    Build a mode-aware audio-device interaction model that can be presented in a real terminal TUI or in a Teamy-owned window. The Teamy main menu should gain an `Audio Devices` button with an image. Clicking it should open a mic picker window that defaults to a pretty graphical view and lists microphones with name, icon, id, and sample rate. Pretty and TUI modes must both support keyboard navigation. The `Show diagnostics` chrome button and `Alt+X` should toggle the picker between pretty mode and the TUI/diagnostic view. Selecting a microphone in the first slice should show a storage-button-style dialog reporting the picked microphone or microphones. The same selection/action state should power both renderers.

3. Safe inbox

   Add an audio input or transcription inbox window that owns text chunks. Future capture and model code writes to this inbox. External delivery is modeled as an explicit command, not as blind keyboard synthesis.

4. Control plane

    Preserve the useful shape of the Python tool by modeling activation sources explicitly: manual push-to-talk, manual toggle, and client-requested listening. The control plane should produce state transitions that UI, CLI, logs, and future websocket clients can all inspect.

5. Python inference daemon

    Add a managed Python daemon that loads `large-v2` on CUDA, stays resident, receives typed log-mel tensors through Rust-owned shared memory, validates the tensor contract, performs lower-level Whisper inference without the high-level `model.transcribe(audio)` pipeline, and returns debug summaries over a named pipe.

6. Visualization renderers

    Treat waveform and spectrogram views as shared data/view-model problems with multiple renderers. Terminal mode can use ratatui-style cell-grid visualizations, while pretty window mode can use the native graphics stack for smoother waveform, spectrogram, and microphone-choice presentations. Pretty mode should be the default for Teamy-owned windows; TUI mode remains available for real terminals and diagnostics.

Suggested module boundaries:

- `src/app/windows_audio.rs` remains the bell-output implementation until it is worth splitting.
- Add `src/app/windows_audio_input.rs` or an `src/app/audio_input/` module for input-device inventory and selected input state.
- Add `src/cli/audio/audio_cli.rs` for the top-level `audio` command group.
- Add `src/cli/audio/input/audio_input_cli.rs` for the input subgroup.
- Add `src/cli/audio/input_device/list/audio_input_device_list_cli.rs` for `audio input-device list`.
- Later add scene support in `src/app/windows_scene.rs` and action dispatch in `src/app/windows_app.rs` once the backend exists.
- Add a Teamy-owned mic picker window path modeled after the cursor-info virtual-session shape, but with a mode-aware renderer that can switch between pretty view and TUI/diagnostic view through the `Show diagnostics` chrome button or `Alt+X`.
- Add `src/cli/audio/transcribe/audio_transcribe_cli.rs` for `audio transcribe <input>`.
- Add `src/cli/audio/daemon/audio_daemon_cli.rs` plus child modules for explicit daemon `start`, `status`, and `stop` commands.
- Add Teamy-owned Python daemon files under a dedicated Python project directory, for example `python/whisperx-daemon/` or `tools/python/whisperx-daemon/`.
- Add `src/cli/self_test/audio_transcription/self_test_audio_transcription_cli.rs` for the real WhisperX fixture validation that `./check-all.ps1 -Full` should run.

## Tracey Specification Strategy

This is a new behavior area, so it should get a dedicated product spec rather than being squeezed into `windowing.md` or the terminal behavior spec.

Recommended new spec:

- `docs/spec/product/audio-input.md`

Recommended Tracey config update:

- Add `teamy-studio-audio-input` to `.config/tracey/config.styx` with Rust implementation coverage from `src/**/*.rs` and tests from `tests/**/*.rs`.

Baseline commands for the first implementation session:

```powershell
tracey query status
tracey query uncovered
tracey query unmapped --path src/app/windows_audio_input.rs
tracey query unmapped --path src/cli/audio
tracey query validate --deny warnings
```

After tests exist for the new command surface, also run:

```powershell
tracey query untested
```

Do not try to clear the repo-wide unmapped baseline in the same slice. Keep new and touched code mapped, and leave historical mapping debt as separate cleanup work.

## Phased Task Breakdown

### Phase 1: Spec, Inventory, And Visible Audio Devices Slice

Objective: Make recording-device inventory real and immediately visible through both CLI output and the first GUI audio-device flow, without opening a recorder.

Tasks:

- Create `docs/spec/product/audio-input.md`.
- Add the new spec to `.config/tracey/config.styx`.
- Add a top-level `audio` CLI command group without breaking the existing launcher Audio action.
- Add `audio input-device list` with `--output-format text|json|csv` support through the existing global output path.
- Implement a recording-device model with Core Audio endpoint id, display name, default-device metadata when available, active state, backend/source, role information when available, sample-rate metadata when available, icon metadata/image data for GUI use, and Windows property debug metadata.
- Implement Windows recording-device enumeration without starting capture. For this first slice, enumerate active Core Audio capture endpoints only.
- Adapt microphone icon metadata/loading from `youre-muted-btw/crates/mic_detection_plugin` into Teamy-owned Windows audio-input code for GUI use. Icon lookup failures must degrade to a fallback or missing-icon state without failing enumeration.
- Add the `Audio Devices` main-menu button with an image.
- Add the mic picker window launched by `Audio Devices`. Pretty mode is default and lists microphones with name, icon, id, and sample rate when available.
- Add keyboard navigation for the mic picker in both pretty mode and TUI/diagnostic mode.
- Use the existing `Show diagnostics` chrome button to toggle the mic picker between pretty mode and TUI/diagnostic mode.
- Add `Alt+X` as the keyboard path for activating the same diagnostics/mode toggle as the `Show diagnostics` chrome button.
- Add selection behavior that shows a simple storage-button-style dialog reporting the microphone or microphones that were picked. Do not open the per-device audio-device window in this slice.
- Add tests for CLI parsing and output shaping that do not require a physical microphone.
- Add pure tests for the audio-device interaction/view-model state: picker list shape, keyboard navigation, mode toggle, and selected-mic dialog content.

Definition of done:

- `teamy-studio audio input-device list` exists.
- `teamy-studio audio input-device list` enumerates real active Windows recording devices on Windows without opening capture.
- `teamy-studio audio input-device list` emits readable text by default when stdout is a terminal.
- `teamy-studio audio input-device list` emits JSON by default when stdout is redirected.
- `teamy-studio audio input-device list --output-format json` exposes verbose debug metadata for each discovered recording device.
- The main menu has an `Audio Devices` button with an image.
- Clicking `Audio Devices` opens a mic picker window in pretty mode by default.
- The mic picker lists active microphones with name, icon, id, and sample rate when available.
- Keyboard navigation works in the mic picker in pretty mode.
- Keyboard navigation works in the mic picker in TUI/diagnostic mode.
- The `Show diagnostics` chrome button toggles from pretty mode to the TUI/diagnostic view.
- `Alt+X` activates the same diagnostics/mode toggle as the `Show diagnostics` chrome button.
- Selecting a microphone shows a simple dialog, like the current Storage button pattern, that reports which microphone or microphones were picked.
- The per-device audio-device window and `arm for record` control are explicitly deferred from this first slice.
- Tests cover parser shape and at least one mocked or pure formatting path.
- Tests cover the audio-device view model without requiring a physical microphone.
- New requirements have Tracey references in touched implementation and tests.
- `tracey query validate --deny warnings` passes.
- `./check-all.ps1` passes or any unrelated failure is documented.

### Phase 2: Persist Selected Input Device

Objective: Let Teamy Studio remember the intended microphone without beginning capture.

Tasks:

- Add selected recording-device persistence under the resolved app home.
- Add `audio input-device show` to report the selected/default effective device.
- Add `audio input-device set <device-id>` or equivalent.
- Expand enumeration or lookup behavior as needed to distinguish active, disabled, unplugged, and not-present selected devices when Windows exposes those states.
- Handle missing devices by reporting a stale selection rather than silently falling back.
- Add diagnostics text helpers for selected device and device availability.

Definition of done:

- The selected device can be shown, set, and detected as stale.
- Persistence behavior is tested with an isolated app home.
- The design does not rely on display names as stable identifiers.

### Phase 3: Hybrid Recording Device Picker Hardening

Objective: Harden the Phase 1 audio-device picker into a persisted, fully mode-aware microphone choice flow while preserving one shared interaction model.

Tasks:

- Keep the `Audio Devices` main-menu button and mic picker window working without breaking the existing bell-output audio path.
- Refine the audio-device window path modeled after cursor-info's virtual-session integration.
- Extend the mode-aware audio-device interaction model so it owns persisted selection, focus, actions, diagnostics, and view-model data independently from the renderer.
- Harden the real terminal TUI renderer for the audio-device picker using the shared interaction model.
- Harden the pretty Teamy-window renderer for the audio-device picker using the same interaction model. Pretty mode remains the default when launched from the GUI.
- Keep the existing `Show diagnostics` chrome button and `Alt+X` as equivalent mode toggles between pretty mode and the TUI/diagnostic view.
- Extend scene/window data so labels, actions, and pretty-mode controls can be generated from runtime device inventory and persisted selected-device state.
- Expand microphone icon metadata/loading as needed for polished picker and selected-device visuals.
- Add the per-device audio-device window for a selected microphone, sharing the same view-model state introduced in the first slice rather than reimplementing device selection.
- Add the per-device audio-device window's icon, name, id, sample rate, and default-on `arm for record` icon button with a tooltip.
- Show disabled/disconnected/default visual states clearly enough for diagnostics.
- Selecting a device persists it and opens or updates the selected-device audio-device window according to the chosen interaction.
- Update diagnostics text to include selected input device and the available choices.

Definition of done:

- A user can open Teamy Studio, click `Audio Devices`, see a pretty microphone chooser by default, and choose a microphone.
- Choosing a microphone can open or update the selected-device audio-device window.
- The selected-device window includes icon, name, id, sample rate, and a default-on armed state control for future recording.
- A user can invoke the audio-device picker from a real terminal and get TUI behavior backed by the same interaction model.
- The `Show diagnostics` chrome button toggles the Teamy audio-device window between pretty mode and TUI/diagnostic mode.
- The Windows bell picker still works as before.
- The selected microphone is visible in diagnostics text.
- Pretty mode includes microphone icons when icon lookup succeeds. Icon lookup failures must not prevent listing or selecting devices.
- No capture starts as a side effect of listing or selecting a device.

### Phase 4: Transcription Inbox Shell

Objective: Create the safe place for future transcription output before adding real transcription.

Tasks:

- Add a transcription inbox window or hosted surface.
- Store staged text chunks in Teamy-owned state.
- Support append, clear, copy-to-clipboard, and diagnostics.
- Add a test hook or CLI command to append text into the inbox model without microphone capture.
- Model output routing as `staged only` by default.

Definition of done:

- Text can be staged in Teamy Studio and copied intentionally.
- The inbox never sends text to another app unless an explicit future route is chosen.
- Diagnostics expose chunk count, selected device, armed/listening state, and routing state.

### Phase 5: Listening Control Plane

Objective: Preserve the current tool's useful hotkey/API control model without depending on capture or Whisper first.

Tasks:

- Add a state model for manual push-to-talk, manual toggle, and client-requested listening.
- Add commands or test hooks that can start and stop each activation source independently.
- Ensure manual activation can clear or override client-controlled listening where that behavior is still desired.
- Add a client attachment count concept so client-controlled listening can stop when no client remains.
- Expose the effective listening state in diagnostics and CLI output.

Definition of done:

- Effective listening state is derived from named activation sources rather than a single boolean.
- Tests cover push-to-talk press/release, toggle on/off, client start/stop, and client disconnect auto-stop.
- No audio capture or text delivery is required for these state transitions to be validated.

### Phase 6: Capture And Transcription Integration

Objective: Connect actual audio and model inference after the safe destination exists.

Tasks:

- Add WASAPI capture for the selected device.
- Add arm/pause/stop states.
- Add in-memory audio buffering with enough metadata for timestamps.
- Add resampling and chunking behind a stable trait/interface.
- Add shared waveform and spectrogram view-model data that can drive both terminal and pretty renderers.
- Add ratatui/cell-grid waveform and spectrogram views for terminal/TUI mode.
- Add richer native graphics waveform and spectrogram views for pretty Teamy-window mode.
- Connect an initial transcription backend that writes chunks to the inbox.
- Add trace/log events for device changes, capture start/stop, chunk boundaries, transcription results, and delivery actions.

Definition of done:

- Recording can be armed and paused from inside Teamy Studio.
- Recording visuals are available in both modes: cell-grid approximations in terminal/TUI mode and native graphical visualizers in pretty mode.
- Transcription chunks appear in the inbox.
- Unplug/replug scenarios fail visibly and recoverably rather than silently killing the worker.
- Output still defaults to staged-only.

### Phase 7: Fixture-To-Daemon Vertical Slice

Objective: Prove the Rust-owned audio preparation and Python inference boundary with deterministic file input before live microphone capture.

Tasks:

- Add a local VCTK sample manifest reader. The manifest contains external `wav48` and `txt` file paths and expected normalized transcript text.
- Add a PowerShell helper, for example `tools/generate-vctk-audio-samples.ps1`, that regenerates the manifest from local `teamy-mft` queries instead of copying corpus files into the repo.
- Ensure the generated local manifest, for example `tests/fixtures/audio/vctk-samples.local.json`, is ignored by git because it contains machine-specific absolute paths.
- Add Teamy-owned audio normalization that accepts a manifest-provided 48 kHz VCTK WAV path and produces 16 kHz mono `f32` samples.
- Add the fixed-shape `WhisperLogMel80x3000` Rust newtype.
- Adapt the Burnt Apple log-mel frontend into Teamy-owned code with tests that prove output dimensions and representative values.
- Add named-pipe control messages for daemon health, inference request, inference result, and slot release.
- Add Rust-owned shared-memory slot allocation and reuse for 960,000-byte feature tensors.
- Add a managed Python daemon path that loads `large-v2` on CUDA and validates tensor payloads before inference.
- Add `teamy-studio audio daemon start|status|stop|doctor`.
- Add `teamy-studio audio transcribe <wav>` that auto-starts the daemon when needed, normalizes the file, builds log-mel features, sends the tensor through shared memory, and prints text when stdout is a terminal or pretty JSON when redirected.
- Add `teamy-studio self-test audio-transcription` for the VCTK manifest path.

Definition of done:

- `audio transcribe <manifest-provided-vctk.wav>` exercises Rust normalization, Rust log-mel, shared-memory tensor transfer, Python validation, lower-level Python inference, and structured result return.
- `audio daemon doctor` can quickly prove the uv-managed Python environment imports WhisperX and sees CUDA, and can fully prove `large-v2` loads when requested.
- `audio daemon status` reports Python readiness, the `uv` virtual environment path, the model download/cache path, and shared-memory slot pool metrics.
- Python rejects malformed tensor size/dtype/dimension/alignment in a way Rust can surface.
- The result includes transcript text plus debug summaries without raw logits or raw encoder tensors.
- `self-test audio-transcription` runs the same real daemon path, reads external VCTK sample paths from the generated manifest, and requires the normalized exact transcript for each selected sample. The initial target transcript remains `is there a waiting list`.
- `self-test audio-transcription` is suitable for `./check-all.ps1 -Full`.
- Default non-full validation does not require CUDA or WhisperX model availability.

### Phase 8: Client Streaming Compatibility

Objective: Reintroduce the useful websocket/client workflow after staged output and listening state are safe.

Tasks:

- Add a local client API for start listening, stop listening, and receiving transcription chunks.
- Decide whether to preserve the exact current route names or expose a Teamy-native command protocol.
- Add keepalive or connection lifecycle handling.
- Route client-requested transcriptions to attached clients and to the inbox, not to global typewriting.
- Keep API key or local authorization requirements explicit.

Definition of done:

- A local client can request listening and receive staged transcription chunks.
- Client disconnect stops client-controlled listening.
- Inbox state remains the source of truth for generated text.

## Recommended Implementation Order

1. `audio-input.md` spec plus Tracey config entry.
2. Recording-device model, fake provider, CLI scaffolding, and formatter tests for `audio input-device list`.
3. Windows active-device enumerator with endpoint id, name, sample-rate metadata when available, and icon data/source for GUI use.
4. Shared audio-device interaction/view-model state for picker selection, keyboard navigation, dialog content, and pretty/TUI modes.
5. Main-menu `Audio Devices` button with image, pretty mic picker window, TUI/diagnostic mode, `Alt+X` mode toggle, and selected-microphone dialog.
6. Selection persistence, selected-device window, and stale-device handling.
7. Per-device `arm for record` control.
8. Inbox model and window.
9. Listening control state.
10. Fixture-to-daemon vertical slice.
11. Live capture, visualizers, and staged transcription.
12. Client streaming compatibility.

## Open Decisions

- Should the existing launcher `Audio` card remain bell-output focused while a new `Audio Devices` button handles microphone choice, or should the old `Audio` card later become an audio category picker?
- Should the inbox be a standalone window, a terminal-hosted virtual session like cursor-info, or a future general text surface?
- What is the first explicit delivery action: copy to clipboard, paste into a selected Teamy terminal, or send through an IME-style path?
- Should Teamy Studio preserve F23 and Pause as default activation keys, or should those become user-configured bindings from the start?
- Should manual activation continue to clear API/client listening, matching the Python tool, or should activation sources compose without clearing one another?
- Should the future client API preserve the current `/start_listening`, `/stop_listening`, and `/results` routes for compatibility?
- Should `audio daemon doctor --full` also run a one-token or no-op model warmup, or is model load sufficient?

## First Concrete Slice

Build the visible `Audio Devices` slice and the audio-input Tracey spec.

The slice should not capture audio and should not touch transcription inference. It should end with a visible GUI path from the main menu to a mic picker window, with keyboard navigation in both pretty and TUI/diagnostic modes. Selecting a microphone should show a simple dialog reporting which microphone or microphones were picked, following the current Storage button pattern. It should also expose the same underlying inventory through `audio input-device list` for debugging and automation.

Implementation sketch:

- Add `docs/spec/product/audio-input.md` with requirements for inventory, no-capture listing, selected-device identity, and staged-only transcription safety.
- Register the spec in `.config/tracey/config.styx`.
- Add CLI modules for `audio`, `audio input-device`, and `audio input-device list` following the repo's file layout convention.
- Add `RecordingDeviceSummary` and a formatting path that supports existing `CliOutput` formats.
- Add a fake inventory provider for tests before wiring the Windows enumerator.
- Add Windows recording-device enumeration with active Core Audio capture endpoints, endpoint id, display name, default metadata, sample-rate metadata when available, and icon data/source for GUI use.
- Add the `Audio Devices` main-menu button with an image.
- Add a mic picker window that lists microphones with name, icon, id, and sample rate when available.
- Add keyboard navigation for the mic picker in pretty mode and in TUI/diagnostic mode.
- Add a `Show diagnostics` chrome toggle from pretty mode to TUI/diagnostic mode for the mic picker.
- Add `Alt+X` as the keyboard path for the same diagnostics/mode toggle.
- Add selection behavior that shows a storage-button-style dialog reporting which microphone or microphones were picked.
- Defer the per-device audio-device window and the `arm for record` icon button to a later slice.
- Validate with `./check-all.ps1` and Tracey validation.

This slice is the right first move because it creates an executable and visible foundation for the narrated microphone picker and transcription workflow while avoiding the two biggest traps: blind OS key injection and premature ML-inference work.

## First Inference Slice

After the inventory slice, build `audio transcribe` against the fixture-to-daemon path.

Observable target:

```powershell
teamy-studio audio daemon status
teamy-studio audio transcribe <wav-path-from-vctk-sample-manifest>
teamy-studio self-test audio-transcription
```

Expected data flow:

1. Rust reads a local sample manifest that contains paired VCTK `wav48` and `txt` paths.
2. Rust selects an original 48 kHz VCTK WAV path from the manifest and reads the paired transcript text.
3. Rust normalizes to 16 kHz mono `f32` samples.
4. Rust uses the energy pause detector to choose utterance chunks.
5. Rust pads/trims a chunk to 30 seconds.
6. Rust computes a `WhisperLogMel80x3000` tensor.
7. Rust writes exactly 960,000 bytes of feature data into a Rust-owned shared-memory slot.
8. Rust sends a named-pipe inference request with slot metadata.
9. Python validates the slot and tensor contract.
10. Python runs lower-level Whisper inference using the resident `large-v2` CUDA model.
11. Python returns a debug summary result over the named pipe.
12. Rust releases or reuses the slot and renders CLI output according to the global output-format behavior.
13. The self-test normalizes the daemon transcript and the paired VCTK transcript and requires an exact word sequence match.

This slice is intentionally file-based. Live microphone capture should come after this path is passing because the fixture path makes every boundary observable and repeatable.

### VCTK Sample Manifest

Do not commit VCTK audio or transcript corpus files to Teamy Studio. Commit only the reader/generator code and, if useful, a small example manifest shape that does not point at required machine-specific paths.

The real local manifest should be generated and read by the self-test. Suggested path:

```text
tests/fixtures/audio/vctk-samples.local.json
```

Suggested manifest shape:

```json
{
  "samples": [
    {
      "id": "p230_397",
      "wav48_path": "G:\\Datasets\\VCTK\\VCTK-Corpus-smaller\\wav48\\p230\\p230_397.wav",
      "txt_path": "G:\\Datasets\\VCTK\\VCTK-Corpus-smaller\\txt\\p230\\p230_397.txt",
      "expected_normalized_text": "is there a waiting list"
    }
  ]
}
```

The generator script should use `teamy-mft` to discover candidate corpus roots and files. It should support selecting a fresh set of samples, for example with a `-Count` parameter, so new validation samples can be generated without changing the implementation. Starting-point discovery commands:

```powershell
teamy-mft query "VCTK txt" --limit 5
teamy-mft query "VCTK wav48" --limit 5
```

The generator should pair files by utterance id, such as `p225_001`, by matching `txt/<speaker>/<id>.txt` with `wav48/<speaker>/<id>.wav`. It should write only paths and expected normalized text into the local manifest. It should not copy audio or text files into the repo.