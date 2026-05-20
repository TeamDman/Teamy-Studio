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

1. keep the new app-host and CLI boundaries stable
2. decide how much additional root shim collapse we want beyond the compile-time win target
3. run a slower final verification checkpoint when we want to call the refactor complete

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
- `teamy_studio_observability` now owns the compiled `logging_init` implementation, with the root crate preserving `crate::logging_init` through a thin adapter over `GlobalArgs`
- `teamy_studio_audio_input` now owns the compiled `windows_audio_input` implementation, with the root crate preserving `crate::app::windows_audio_input` through a thin shim
- `teamy_studio_terminal_core` now owns the compiled `windows_terminal` and `windows_terminal_self_test` implementations, plus the shared `VtEngineChoice` contract, with the root crate preserving `crate::app::windows_terminal`, `crate::app::windows_terminal_self_test`, and `crate::app::VtEngineChoice`
- `teamy_studio_shell` now owns the compiled `cell_grid`, `windows_scene`, and `windows_d3d12_renderer` implementations, with the root crate preserving those module paths through thin shims
- `teamy_studio_cursor_info` now owns the compiled `windows_cursor_info` implementation, including `CursorInfoVirtualSession`, with the root crate preserving `crate::app::windows_cursor_info`
- `render_verification` now lives in `teamy_studio_shell`
- `teamy_studio_app_host` now owns the compiled `windows_app` implementation, with the root crate preserving `crate::app::windows_app` through a thin shim and re-exporting `TerminalThroughputBenchmarkMode` / `TerminalWindowSummary`
- `teamy_studio_cli` now owns the compiled `app` and `cli` surface trees, with the root crate preserving `crate::app` and `crate::cli` through thin shims
- there are no remaining crate `#[path = "../../../src/..." ]` includes into root `src/`
- dead root copies of `paths` and `win32_support` implementation files have been removed now that those crates own their source locally
- fast iteration validation is currently green with `cargo clippy -- -D warnings`
- the root workspace currently includes:
  - `crates/teamy_studio_app_host`
  - `crates/teamy_studio_audio_transcription`
  - `crates/teamy_studio_cli`
  - `crates/teamy_studio_cursor_info`
  - `crates/teamy_studio_demo_mode`
  - `crates/teamy_studio_jobs`
  - `crates/teamy_studio_logs`
  - `crates/teamy_studio_observability`
  - `crates/teamy_studio_paths`
  - `crates/teamy_studio_shell`
  - `crates/teamy_studio_win32_support`
  - `crates/teamy_studio_audio_core`
  - `crates/teamy_studio_audio_input`
  - `crates/teamy_studio_frontend`
  - `crates/teamy_studio_shell_default`
  - `crates/teamy_studio_image_models`
  - `crates/teamy_studio_spatial`
  - `crates/teamy_studio_teamy_terminal_engine`
  - `crates/teamy_studio_terminal_core`
  - `crates/teamy_studio_timeline_core`
  - `crates/teamy_studio_vt_types`
  - `crates/teamy_studio_waifu2x_reference`
  - `crates/teamy_studio_whisper_stack`
  - `crates/teamy_studio_windows_audio`
  - `crates/teamy_studio_windows_dialogs`
  - `crates/teamy_studio_windows_terminal_replay`
  - `crates/teamy_studio_windows_terminal_engine`

Next slice:

1. if desired, clean up stale historical notes that still mention the pre-flattened root shim files
2. otherwise treat the crate-split objective as largely complete and do a final `\.\check-all.ps1` checkpoint once ready

See:

- `docs/notes/crate-refactor-handoff.md`

## Compaction Handoff

If context compacts, assume the following is the current ground truth unless the code says otherwise.

### Landed extractions

- `src/lib.rs` now inlines the root compatibility modules for `app`, `audio`, `cli`, `frontend`, `image_model`, `logging_init`, `logs`, `model`, `paths`, `shell_default`, `transcription`, `waifu2x_reference`, `whisper`, and `win32_support`
- `teamy_studio_cli` now owns the shim-heavy `app` surface that used to live under `src/app/*`, with the root crate preserving `crate::app` through the inline compatibility module in `src/lib.rs`
- `src/lib.rs` now re-exports `teamy_studio_timeline_core` as `crate::timeline`
- the moved code lives in:
  - `crates/teamy_studio_cli/src/app/*`, compiled through `crates/teamy_studio_cli/src/lib.rs`
  - `crates/teamy_studio_cli/src/cli/*`, compiled through `crates/teamy_studio_cli/src/lib.rs`
  - `crates/teamy_studio_audio_transcription/src/audio_transcription_impl.rs`, compiled through `crates/teamy_studio_audio_transcription/src/lib.rs`
  - `crates/teamy_studio_app_host/src/windows_app_impl.rs`, compiled through `crates/teamy_studio_app_host/src/lib.rs`
  - `crates/teamy_studio_shell/src/cell_grid.rs`, `crates/teamy_studio_shell/src/windows_scene.rs`, and `crates/teamy_studio_shell/src/windows_d3d12_renderer.rs`, compiled through `crates/teamy_studio_shell/src/lib.rs`
  - `crates/teamy_studio_cursor_info/src/windows_cursor_info_impl.rs`, compiled through `crates/teamy_studio_cursor_info/src/lib.rs`
  - `crates/teamy_studio_audio_input/src/windows_audio_input_impl.rs`, compiled through `crates/teamy_studio_audio_input/src/lib.rs`
  - `crates/teamy_studio_jobs/src/jobs_impl.rs`, compiled through `crates/teamy_studio_jobs/src/lib.rs`
  - `crates/teamy_studio_spatial/src/spatial_impl.rs`, compiled through `crates/teamy_studio_spatial/src/lib.rs`
  - `crates/teamy_studio_vt_types/src/vt_types_impl.rs`, compiled through `crates/teamy_studio_vt_types/src/lib.rs`
  - `crates/teamy_studio_windows_audio/src/windows_audio_impl.rs`, compiled through `crates/teamy_studio_windows_audio/src/lib.rs`
  - `crates/teamy_studio_demo_mode/src/windows_demo_mode_impl.rs`, compiled through `crates/teamy_studio_demo_mode/src/lib.rs`
  - `crates/teamy_studio_windows_dialogs/src/windows_dialogs_impl.rs`, compiled through `crates/teamy_studio_windows_dialogs/src/lib.rs`
  - `crates/teamy_studio_teamy_terminal_engine/src/teamy_terminal_engine_impl.rs`, compiled through `crates/teamy_studio_teamy_terminal_engine/src/lib.rs`
  - `crates/teamy_studio_terminal_core/src/windows_terminal_impl.rs` and `crates/teamy_studio_terminal_core/src/windows_terminal_self_test_impl.rs`, compiled through `crates/teamy_studio_terminal_core/src/lib.rs`
  - `crates/teamy_studio_windows_terminal_engine/src/windows_terminal_engine_impl.rs`, compiled through `crates/teamy_studio_windows_terminal_engine/src/lib.rs`
  - `crates/teamy_studio_windows_terminal_replay/src/windows_terminal_replay_impl.rs`, compiled through `crates/teamy_studio_windows_terminal_replay/src/lib.rs`
  - `crates/teamy_studio_shell/src/render_verification.rs`, compiled through `crates/teamy_studio_shell/src/lib.rs`
  - `crates/teamy_studio_timeline_core/src/timeline_impl/*`, compiled through `crates/teamy_studio_timeline_core/src/lib.rs`
  - `crates/teamy_studio_paths/src/*`
  - `crates/teamy_studio_win32_support/src/*`
  - `crates/teamy_studio_audio_core/src/lib.rs`
  - `crates/teamy_studio_frontend/src/lib.rs`
  - `crates/teamy_studio_logs/src/logs_impl.rs`, compiled through `crates/teamy_studio_logs/src/lib.rs`
  - `crates/teamy_studio_observability/src/logging_init_impl.rs`, compiled through `crates/teamy_studio_observability/src/lib.rs`
  - `crates/teamy_studio_shell_default/src/lib.rs`
  - `crates/teamy_studio_image_models/src/image_model_impl.rs`, compiled through `crates/teamy_studio_image_models/src/lib.rs`
  - `crates/teamy_studio_whisper_stack/src/model.rs`, `crates/teamy_studio_whisper_stack/src/transcription.rs`, and `crates/teamy_studio_whisper_stack/src/whisper.rs`, compiled through `crates/teamy_studio_whisper_stack/src/lib.rs`
  - `crates/teamy_studio_waifu2x_reference/src/waifu2x_reference_impl.rs`, compiled through `crates/teamy_studio_waifu2x_reference/src/lib.rs`

### Intentional non-extraction change

- `crates/teamy_studio_terminal_core/src/windows_terminal_impl.rs` contains targeted `#[expect(...)]` annotations added only to satisfy the current fast clippy gate.
- Those annotations were preferred over signature rewrites specifically to preserve behavior and code shape.
- Do not "clean up" those signatures during the refactor unless there is a separate deliberate reason.

### Validation state

- current fast iteration gate: `cargo clippy -- -D warnings`
- current status of that fast gate: green after the landed extractions and the follow-up physical relocation pass into `crates/`
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

1. documentation cleanup for stale historical path references outside the active handoff docs, if we care about those notes staying navigable
2. any final polish inside `src/lib.rs` if we want the root bootstrap to be even smaller or more explicit
3. final whole-repo verification with `\.\check-all.ps1` when we are ready for a slower checkpoint

Avoid unnecessary churn in the already-extracted host/shell layers unless there is a concrete reason:

- `crates/teamy_studio_app_host/src/windows_app_impl.rs`
- `crates/teamy_studio_shell/src/windows_scene.rs`
- `crates/teamy_studio_terminal_core/src/windows_terminal_impl.rs`
- `crates/teamy_studio_cli/src/cli/audio/transcribe/audio_transcribe_cli.rs`

Current remaining high-entanglement tier worth treating as one connected problem:

- `src/lib.rs`
- `crates/teamy_studio_cli/src/lib.rs`

After the physical relocation pass, the CLI extraction, and the root shim collapse, the remaining root work is primarily bootstrap polish and final verification rather than moving crate-owned implementation bodies.

The main crate-split blocker tier has been cleared; the remaining work is mostly optional cleanup and final verification.

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
