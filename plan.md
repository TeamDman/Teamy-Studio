# Teamy Studio Crate Refactor Plan

## Mission

Break the current monolith into separate crates to reduce compile times while preserving existing behavior and functionality.

This refactor is explicitly **not** a rewrite.

## Preservation Mandate

The existing code is the result of many compounding fixes and behavioral tweaks.

Because of that:

- We must preserve code as-is to the greatest extent possible.
- When moving code into a crate, prefer exact moves and thin re-export shims over rewrites.
- Do not omit, simplify, or "clean up" lines just because they look unimportant.
- Do not redesign APIs during extraction unless a change is required to make the code compile.
- If a seam is messy, move the mess intact first, then improve it later behind tests and explicit review.
- Compile-time wins are valuable only if behavior stays unchanged.

## Non-Goals

- No event-system rewrite as the driver of this effort.
- No hard cutover that deletes or sidelines working code.
- No speculative architecture cleanup that changes product behavior.
- No drive-by renames, formatting churn, or style rewrites unrelated to the active extraction.

## Working Style

- Extract one stable subsystem at a time.
- Keep old import paths working with compatibility re-exports where practical.
- Prefer exact file moves or exact content copies before any behavioral edits.
- Use `cargo clippy -- -D warnings` for fast iteration while extracting crates.
- Run `.\check-all.ps1` at larger checkpoints after a batch of slices.
- If a locked executable blocks the build, use `.\stop.ps1`.

## Target Workspace Shape

The end state should be a real Cargo workspace with a thin root package and focused subcrates.

Planned extraction order:

1. `teamy_studio_paths`
2. `teamy_studio_win32_support`
3. `teamy_studio_timeline_core`
4. `teamy_studio_audio_core`
5. `teamy_studio_frontend`
6. `teamy_studio_shell_default`
7. `teamy_studio_transcription`
8. `teamy_studio_image_models`
9. `teamy_studio_shell`
10. `teamy_studio_terminal_core`
11. `teamy_studio_observability`
12. `teamy_studio_app_host`
13. `teamy_studio_cli`
14. feature crates such as launcher, audio input, timeline viewer, and cursor-info

This order is intentionally low-risk:

- start with low-dependency utility code
- isolate pure model/compute code before UI orchestration
- move shared shell/runtime code before feature-specific windows
- split feature crates only after the shared host seams are stable

## Current Slice

Current goal:

1. establish the Cargo workspace shape
2. extract `src/paths` into `crates/teamy_studio_paths`
3. keep `crate::paths` stable through a thin re-export shim
4. prove that the first crate split can happen without changing behavior

Current status:

- workspace scaffolding is in place
- `teamy_studio_paths` has been extracted behind a compatibility re-export
- `teamy_studio_win32_support` has been extracted behind a compatibility re-export
- `teamy_studio_audio_core` has been extracted behind a compatibility re-export
- `teamy_studio_frontend` has been extracted behind a compatibility re-export
- `teamy_studio_shell_default` has been extracted behind a compatibility re-export
- `teamy_studio_image_models` has been extracted behind a compatibility re-export
- `teamy_studio_waifu2x_reference` has been extracted behind a compatibility re-export
- `teamy_studio_audio_transcription` has been extracted behind a compatibility re-export
- `teamy_studio_spatial` has been extracted behind a compatibility re-export
- `teamy_studio_jobs` has been extracted behind a compatibility re-export
- `teamy_studio_vt_types` has been extracted behind a compatibility re-export
- `teamy_studio_demo_mode` has been extracted behind a compatibility re-export
- `teamy_studio_windows_audio` has been extracted behind a compatibility re-export
- `teamy_studio_windows_dialogs` has been extracted behind a compatibility re-export
- `teamy_studio_teamy_terminal_engine` has been extracted behind a compatibility re-export
- `teamy_studio_windows_terminal_engine` has been extracted behind a compatibility re-export
- `teamy_studio_windows_terminal_replay` has been extracted behind a compatibility re-export
- `teamy_studio_timeline_core` now owns the compiled `timeline` implementation, with the root crate re-exporting it as `crate::timeline`
- `teamy_studio_whisper_stack` now owns the compiled `model`, `transcription`, and `whisper` implementations, with the root crate preserving `crate::model`, `crate::transcription`, and `crate::whisper` through thin shims
- `teamy_studio_logs` now owns the compiled `logs` implementation, with the root crate preserving `crate::logs` through a thin shim
- `teamy_studio_audio_input` now owns the compiled `windows_audio_input` implementation, with the root crate preserving `crate::app::windows_audio_input` through a thin shim
- fast iteration validation is currently green with `cargo clippy -- -D warnings`
- the root workspace currently includes:
  - `crates/teamy_studio_audio_transcription`
  - `crates/teamy_studio_demo_mode`
  - `crates/teamy_studio_jobs`
  - `crates/teamy_studio_paths`
  - `crates/teamy_studio_win32_support`
  - `crates/teamy_studio_audio_core`
  - `crates/teamy_studio_audio_input`
  - `crates/teamy_studio_frontend`
  - `crates/teamy_studio_shell_default`
  - `crates/teamy_studio_image_models`
  - `crates/teamy_studio_spatial`
  - `crates/teamy_studio_teamy_terminal_engine`
  - `crates/teamy_studio_timeline_core`
  - `crates/teamy_studio_vt_types`
  - `crates/teamy_studio_waifu2x_reference`
  - `crates/teamy_studio_whisper_stack`
  - `crates/teamy_studio_logs`
  - `crates/teamy_studio_windows_audio`
  - `crates/teamy_studio_windows_dialogs`
  - `crates/teamy_studio_windows_terminal_replay`
  - `crates/teamy_studio_windows_terminal_engine`

Next slice:

1. extract the next pure or low-coupling subsystem, likely `timeline` or another utility seam that does not force a redesign
2. preserve the root module path through the same thin re-export pattern
3. keep the extraction as close to an exact move as possible

## Compaction Handoff

If context compacts, assume the following is the current ground truth unless the code says otherwise.

### Landed extractions

- `src/paths/mod.rs` is now a thin shim: `pub use teamy_studio_paths::*;`
- `src/win32_support/mod.rs` is now a thin shim: `pub use teamy_studio_win32_support::*;`
- `src/audio.rs` is now a thin shim: `pub use teamy_studio_audio_core::*;`
- `src/frontend.rs` is now a thin shim: `pub use teamy_studio_frontend::*;`
- `src/shell_default.rs` is now a thin shim: `pub use teamy_studio_shell_default::*;`
- `src/image_model.rs` is now a thin shim: `pub use teamy_studio_image_models::*;`
- `src/logs.rs` is now a thin shim: `pub use teamy_studio_logs::*;`
- `src/model.rs` is now a thin shim: `pub use teamy_studio_whisper_stack::model::*;`
- `src/waifu2x_reference.rs` is now a thin shim: `pub use teamy_studio_waifu2x_reference::*;`
- `src/transcription.rs` is now a thin shim: `pub use teamy_studio_whisper_stack::transcription::*;`
- `src/whisper.rs` is now a thin shim: `pub use teamy_studio_whisper_stack::whisper::*;`
- `src/app/audio_transcription.rs` is now a thin shim: `pub use teamy_studio_audio_transcription::*;`
- `src/app/windows_audio_input.rs` is now a thin shim: `pub use teamy_studio_audio_input::*;`
- `src/app/spatial.rs` is now a thin shim: `pub use teamy_studio_spatial::*;`
- `src/app/jobs.rs` is now a thin shim: `pub use teamy_studio_jobs::*;`
- `src/app/vt_types.rs` is now a thin shim: `pub use teamy_studio_vt_types::*;`
- `src/app/windows_audio.rs` is now a thin shim: `pub use teamy_studio_windows_audio::*;`
- `src/app/windows_demo_mode.rs` is now a thin shim: `pub use teamy_studio_demo_mode::*;`
- `src/app/windows_dialogs.rs` is now a thin shim: `pub use teamy_studio_windows_dialogs::*;`
- `src/app/teamy_terminal_engine.rs` is now a thin shim: `pub use teamy_studio_teamy_terminal_engine::*;`
- `src/app/windows_terminal_engine.rs` is now a thin shim: `pub use teamy_studio_windows_terminal_engine::*;`
- `src/app/windows_terminal_replay.rs` is now a thin shim: `pub use teamy_studio_windows_terminal_replay::*;`
- `src/lib.rs` now re-exports `teamy_studio_timeline_core` as `crate::timeline`
- the moved code lives in:
  - `src/app/audio_transcription_impl.rs`, compiled through `crates/teamy_studio_audio_transcription/src/lib.rs`
  - `src/app/windows_audio_input_impl.rs`, compiled through `crates/teamy_studio_audio_input/src/lib.rs`
  - `src/app/jobs_impl.rs`, compiled through `crates/teamy_studio_jobs/src/lib.rs`
  - `src/app/spatial_impl.rs`, compiled through `crates/teamy_studio_spatial/src/lib.rs`
  - `src/app/vt_types_impl.rs`, compiled through `crates/teamy_studio_vt_types/src/lib.rs`
  - `src/app/windows_audio_impl.rs`, compiled through `crates/teamy_studio_windows_audio/src/lib.rs`
  - `src/app/windows_demo_mode_impl.rs`, compiled through `crates/teamy_studio_demo_mode/src/lib.rs`
  - `src/app/windows_dialogs_impl.rs`, compiled through `crates/teamy_studio_windows_dialogs/src/lib.rs`
  - `src/app/teamy_terminal_engine_impl.rs`, compiled through `crates/teamy_studio_teamy_terminal_engine/src/lib.rs`
  - `src/app/windows_terminal_engine_impl.rs`, compiled through `crates/teamy_studio_windows_terminal_engine/src/lib.rs`
  - `src/app/windows_terminal_replay_impl.rs`, compiled through `crates/teamy_studio_windows_terminal_replay/src/lib.rs`
  - `src/timeline/*`, compiled through `crates/teamy_studio_timeline_core/src/lib.rs`
  - `crates/teamy_studio_paths/src/*`
  - `crates/teamy_studio_win32_support/src/*`
  - `crates/teamy_studio_audio_core/src/lib.rs`
  - `crates/teamy_studio_frontend/src/lib.rs`
  - `src/logs_impl.rs`, compiled through `crates/teamy_studio_logs/src/lib.rs`
  - `crates/teamy_studio_shell_default/src/lib.rs`
  - `src/image_model_impl.rs`, compiled through `crates/teamy_studio_image_models/src/lib.rs`
  - `src/model_impl.rs`, `src/transcription_impl.rs`, and `src/whisper_impl.rs`, compiled through `crates/teamy_studio_whisper_stack/src/lib.rs`
  - `src/waifu2x_reference_impl.rs`, compiled through `crates/teamy_studio_waifu2x_reference/src/lib.rs`

### Intentional non-extraction change

- `src/app/windows_terminal.rs` contains targeted `#[expect(...)]` annotations added only to satisfy the current fast clippy gate.
- Those annotations were preferred over signature rewrites specifically to preserve behavior and code shape.
- Do not "clean up" those signatures during the refactor unless there is a separate deliberate reason.

### Validation state

- current fast iteration gate: `cargo clippy -- -D warnings`
- current status of that fast gate: green after the twenty-one landed extractions above
- last known `.\check-all.ps1` success was earlier in the refactor, before the later `win32_support` and `audio_core` slices, when the user switched us to clippy-only iteration

### Extraction recipe

For the next crate split, follow this exact pattern unless a concrete seam forces something narrower:

1. Add the new crate to `[workspace].members` in `Cargo.toml`.
2. Add a root dependency on the new crate.
3. Create a minimal crate `Cargo.toml` with only the dependencies actually used by the moved code.
4. Copy the existing code as literally as possible into the new crate.
5. Replace the original root module with a thin `pub use new_crate::*;` shim when possible.
6. Only adjust imports when the moved code referred to the old crate root and can no longer compile unchanged.
7. Run `cargo clippy -- -D warnings`.
8. If clippy surfaces unrelated existing issues, prefer narrowly-scoped `#[expect(...)]` annotations over behavior-changing rewrites.

### What not to do

- Do not redesign public call sites just because the code is moving.
- Do not collapse multiple modules into one unless the old code was already that shape.
- Do not mix feature redesign with crate extraction.
- Do not switch back to rewrite-style architecture work just because the failed `event-cutover` branch did that.
- Do not use alternate `$env:CARGO_TARGET_DIR` values.

### Most likely next seams

These are the best current candidates, in rough order of safety:

1. `logging_init`, if we want to finish peeling the root logging surface after `teamy_studio_logs`
2. terminal-adjacent support that does not force a `VtEngineChoice` split by itself
3. a deliberate terminal host extraction only after deciding where shared app types such as `VtEngineChoice`, `TerminalLayout`, and `TerminalSession` should live

Avoid starting with highly entangled orchestration files like:

- `src/app/windows_app.rs`
- `src/app/windows_scene.rs`
- `src/app/windows_terminal.rs`

Those should come later, after more shared foundations have been pulled outward.

Current remaining high-entanglement tier worth treating as one connected problem:

- `src/app/windows_terminal.rs`
- `src/app/windows_terminal_self_test.rs`
- `src/app/windows_cursor_info.rs`
- `src/app/render_verification.rs`
- `src/app/cell_grid.rs`
- `src/app/windows_d3d12_renderer.rs`
- `src/app/windows_scene.rs`
- `src/app/windows_app.rs`

The main enabling blocker for that tier is shared app-local terminal types still rooted in `src/app/mod.rs` and `src/app/windows_terminal.rs`.

## Rules For Future Slices

- Move code first, improve second.
- Keep behavior stable first, abstractions second.
- Preserve public call sites when a thin wrapper can do that cheaply.
- Do not collapse modules together during extraction.
- Keep validation green after every slice before starting the next one.

## Source Material

Use these as the primary planning context:

- `AGENTS.md`
- `docs/notes/`
- this `plan.md`

Relevant architectural notes already reviewed for this refactor include:

- `docs/notes/event-cutover-record-of-decision.md`
- `docs/notes/event-cutover-plan.md`
- `docs/notes/timeline-profiler-plan.md`
- `docs/notes/timeline-display-model-record-of-decision.md`
- `docs/notes/window-language-and-launcher-plan.md`
- `docs/notes/audio-input-inbox-plan.md`

## Definition Of Success

A slice is successful when:

- code moved into a crate with minimal or no behavioral change
- existing call sites continue to work
- `.\check-all.ps1` passes
- the next extraction seam is clearer than before
