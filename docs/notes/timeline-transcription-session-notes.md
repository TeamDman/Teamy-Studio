# Timeline Transcription Session Notes

Date: 2026-04-29

## Progress

- Timeline transcription tracks now model three targets: the Rust Whisper model, an input audio track, and an output text track.
- Transcription settings moved into a dedicated scene window and expose model, input audio, output text, automation toggles, manual flush, and create-track actions.
- Creating a transcription track now opens the transcription settings window immediately.
- The timeline track list has a `+ New Track` row after the existing tracks, mirroring a browser new-tab slot.
- The transcription settings window can create a new text track and automatically select it as output.
- The transcription settings window can create a new microphone-backed audio track through the mic picker and automatically select it as input.
- Microphone audio tracks created through picker flows now persist the selected endpoint id in the timeline document, so timeline windows can materialize a live audio runtime from document state.
- Live transcription uses the Rust Whisper pipeline instead of the Python debug path for GUI transcription chunks.
- Background work is tracked through a jobs registry and rendered in a Jobs window, including Rust transcription chunks and model preparation.
- Model preparation state is inspected before enabling transcription. Invalid models show a warning scene with model locations, open/copy actions, a warning chime, and a hold-to-confirm `HOLD PREPARE` action.
- Successful Rust transcription results are staged by the audio runtime, then drained into the transcription track's targeted output text track as timeline text blocks.
- Right-button timeline pan and wheel zoom now cooperate by rebasing the active pan drag after zoom. Pan start is restricted to non-empty track content to avoid bad capture states from ruler/body/gutter right-clicks.
- `check-all.ps1 -VerboseBuild` now provides extra Cargo/build/network diagnostics for investigating heavy build network activity.

## Lessons Learned

- Scene windows can synchronously re-enter message handling during close/destroy. Avoid holding `SCENE_APP_STATE` borrows while destroying windows or spawning follow-up warning windows.
- Timeline document state and scene-local runtime state can diverge. Anything a later scene needs to reconstruct must be represented in the document, not only in the window that created it.
- Audio tracks need source identity, not just display labels. A microphone track without an endpoint id is not a valid live audio target.
- Runtime transcription results are not timeline output until an explicit bridge commits them into the document. Staging alone only updates the audio runtime.
- Do not drop staged transcription text just because no output target is selected yet. Keep it buffered and log that it is waiting for a target.
- Right-button pan state stores an origin viewport. If wheel zoom happens during a pan, the pan origin must be rebased to the zoomed viewport or the next mouse move can overwrite the zoom.
- Trace logging every mouse move can make `--log-filter trace` feel like a freeze. Prefer sparse breadcrumbs around input state transitions unless diagnosing a specific short interaction.
- Hit testing should be narrower for capture-based interactions than for passive interactions. Wheel zoom can cover broad timeline surfaces, but right-button pan should only capture where panning is meaningful.
- Invalid states should be pushed into typed document APIs where possible. `set_transcription_track_target_audio_track` and `set_transcription_track_target_text_track` reject wrong track kinds, and microphone-backed audio creation now records source identity.

## Remaining Work

- Transcription still consumes the live microphone runtime rather than a fully generalized audio-track source abstraction.
- The chunk begin/end heads need stronger direct manipulation and document-backed persistence.
- The warning scene styling is functional but still not a complete animated shader language for warnings.
- Jobs window updates are visible on render ticks, but a more explicit job-changed notification would make it feel more live.
- Existing timeline documents with microphone tracks created before endpoint persistence may still be label-only and cannot reconstruct live runtime identity.

## Validation

- `./check-all.ps1` passed after the latest code changes: format, clippy, build, tests, and Tracey status.