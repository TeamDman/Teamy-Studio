# Teamy Studio Crate Refactor Handoff

## Purpose

This document is the operational handoff for finishing the crate split on `retry-cratify`.

It is intentionally focused on:

- current ground truth
- preservation constraints
- what has already been extracted
- what still remains in the root crate
- the recommended sequence for finishing the transition

This is **not** a rewrite plan.

## Non-Negotiable Constraints

- Preserve code as-is to the greatest extent possible.
- Prefer exact file moves plus thin re-export shims over rewrites.
- Do not delete or "simplify" lines that look unimportant.
- Do not redesign APIs during extraction unless required to compile across a crate boundary.
- Keep using `cargo clippy -- -D warnings` for fast iteration.
- Do not use alternate `$env:CARGO_TARGET_DIR` values.
- If a locked `.exe` blocks the build, use `.\stop.ps1`.

## Current State

Branch:

- `retry-cratify`

Latest relevant commits:

1. `35d1ee0` `Add crate refactor handoff`
2. `77c5ef3` `Extract terminal and shell crates`
3. `e26e1f7` `Document remaining terminal extraction blockers`
4. `f7a3619` `Extract model, logs, and audio input crates`
5. `7e3288e` `Extract timeline and replay crates`
6. `4d5fb9e` `Extract terminal support crates`

Validation:

- `cargo clippy -- -D warnings` is green
- expected warning still present: future incompatibility note for `nom v3.2.1`
- the worktree was clean when this handoff was written

## Workspace Members

These crates already exist and compile:

- `teamy_studio_paths`
- `teamy_studio_win32_support`
- `teamy_studio_audio_core`
- `teamy_studio_frontend`
- `teamy_studio_shell_default`
- `teamy_studio_image_models`
- `teamy_studio_waifu2x_reference`
- `teamy_studio_audio_transcription`
- `teamy_studio_spatial`
- `teamy_studio_jobs`
- `teamy_studio_vt_types`
- `teamy_studio_demo_mode`
- `teamy_studio_windows_audio`
- `teamy_studio_windows_dialogs`
- `teamy_studio_teamy_terminal_engine`
- `teamy_studio_windows_terminal_engine`
- `teamy_studio_windows_terminal_replay`
- `teamy_studio_timeline_core`
- `teamy_studio_whisper_stack`
- `teamy_studio_logs`
- `teamy_studio_observability`
- `teamy_studio_audio_input`
- `teamy_studio_terminal_core`
- `teamy_studio_shell`
- `teamy_studio_cursor_info`

## What Those Crates Own

High-signal ownership map:

- `teamy_studio_whisper_stack`
  - owns [src/model_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/model_impl.rs), [src/transcription_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/transcription_impl.rs), and [src/whisper_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/whisper_impl.rs)
- `teamy_studio_logs`
  - owns [src/logs_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/logs_impl.rs)
- `teamy_studio_observability`
  - owns [src/logging_init_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/logging_init_impl.rs)
- `teamy_studio_audio_input`
  - owns [src/app/windows_audio_input_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/app/windows_audio_input_impl.rs)
- `teamy_studio_terminal_core`
  - owns [src/app/windows_terminal_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/app/windows_terminal_impl.rs)
  - owns [src/app/windows_terminal_self_test_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/app/windows_terminal_self_test_impl.rs)
  - owns the shared `VtEngineChoice` contract
- `teamy_studio_shell`
  - owns [src/app/cell_grid_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/app/cell_grid_impl.rs)
  - owns [src/app/windows_scene_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/app/windows_scene_impl.rs)
  - owns [src/app/windows_d3d12_renderer_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/app/windows_d3d12_renderer_impl.rs)
  - owns [src/app/render_verification_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/app/render_verification_impl.rs)
- `teamy_studio_cursor_info`
  - owns [src/app/windows_cursor_info_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/app/windows_cursor_info_impl.rs)
  - includes `CursorInfoVirtualSession`, which is still used by the app host
- `teamy_studio_app_host`
  - owns [src/app/windows_app_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/app/windows_app_impl.rs)
  - owns `TerminalThroughputBenchmarkMode` and `TerminalWindowSummary`

The old module paths remain alive through thin shims in `src/` and `src/app/`.

## What Is Still Root-Local

The root crate still directly owns the last major orchestration layer:

- [src/app/mod.rs](/G:/Programming/Repos/Teamy-Studio/src/app/mod.rs)
- [src/cli](/G:/Programming/Repos/Teamy-Studio/src/cli)
- [src/lib.rs](/G:/Programming/Repos/Teamy-Studio/src/lib.rs)

Of these, the main remaining non-crate-split work is the CLI surface and any optional additional root thinning.

## Main Result

The previous blocker between `render_verification` and `windows_app` has been resolved.

What changed:

- shell-owned renderer code now exposes `build_text_rendering_plane_verification_geometry(...)`
- [src/app/render_verification_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/app/render_verification_impl.rs) now calls the shell-owned helper
- `render_verification` now compiles through `teamy_studio_shell`
- `windows_app.rs` now compiles through `teamy_studio_app_host`

## Recommended Finish Sequence

### 1. Stabilize the new app-host boundary

Goal:

- keep `teamy_studio_app_host` stable while we decide whether more host-local types should move into it

Expected dependencies for `teamy_studio_app_host`:

- `teamy_studio_terminal_core`
- `teamy_studio_shell`
- `teamy_studio_cursor_info`
- `teamy_studio_audio_input`
- `teamy_studio_audio_transcription`
- `teamy_studio_logs`
- `teamy_studio_timeline_core`
- `teamy_studio_whisper_stack`
- `teamy_studio_windows_audio`
- `teamy_studio_windows_dialogs`
- `teamy_studio_win32_support`
- `teamy_studio_paths`

Important:

- do not rush to redesign `app/mod.rs`
- treat the current app-host move as successful unless a concrete reason appears to thin it further

### 2. Optional CLI/root thinning

If we want to complete the original workspace shape beyond compile-time wins:

- keep shrinking [src/cli](/G:/Programming/Repos/Teamy-Studio/src/cli)
- possibly create `teamy_studio_cli`

This is not required to finish the core compile-time-oriented transition.

### 3. Final slower verification

When the optional cleanup churn is done:

- run [check-all.ps1](/G:/Programming/Repos/Teamy-Studio/check-all.ps1)

## Recommended Order For Another Agent

If another agent is continuing from here, the safest order is:

1. keep the current app-host and shell boundaries intact
2. decide whether CLI/root thinning is worth the churn
3. run the slower final verification checkpoint when ready
4. run `cargo clippy -- -D warnings`
5. run `\.\check-all.ps1` at the final slower checkpoint

## Files To Treat Carefully

These are the files most likely to regress behavior if "cleaned up":

- [src/app/windows_app.rs](/G:/Programming/Repos/Teamy-Studio/src/app/windows_app.rs)
- [src/app/windows_terminal_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/app/windows_terminal_impl.rs)
- [src/app/windows_scene_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/app/windows_scene_impl.rs)
- [src/app/windows_d3d12_renderer_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/app/windows_d3d12_renderer_impl.rs)
- [src/app/windows_audio_input_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/app/windows_audio_input_impl.rs)
- [src/model_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/model_impl.rs)
- [src/whisper_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/whisper_impl.rs)

If a compile error appears after a move:

- prefer fixing imports
- prefer widening `pub(crate)` to `pub` only when forced by a crate boundary
- prefer adding or removing narrow `#[expect(...)]` annotations only when clippy expectations drift
- do not rewrite the implementation body unless there is no other path

## Validation Protocol

Fast iteration:

```powershell
cargo clippy -- -D warnings
```

Larger checkpoint:

```powershell
.\check-all.ps1
```

If build output is blocked by a running executable:

```powershell
.\stop.ps1
```

## Known Weirdness That Is Intentional

- Some moved modules are compiled from `*_impl.rs` files via crate-local `#[path = ...]` includes.
- Some root modules are one-line re-export shims on purpose.
- Some old `#[expect(...)]` annotations had to be removed when they became unfulfilled after extraction.
- `windows_app.rs` now re-exports `teamy_studio_app_host::windows_app::*;`
- `render_verification.rs` now re-exports `teamy_studio_shell::render_verification::*;`
- `logging_init.rs` now converts `GlobalArgs` into `teamy_studio_observability::LoggingConfig` and forwards to the extracted implementation.
- several shim modules use narrow `#[expect(unused_imports, reason = ...)]` annotations because they now exist only to preserve old module paths

These are all preservation-first tactics, not architectural mistakes.

## Definition Of Done

The transition is "finished enough" when:

1. `cargo clippy -- -D warnings` still passes
2. `.\check-all.ps1` passes at the final checkpoint
3. behavior is preserved
4. we are satisfied with the remaining root-local CLI/observability surface

## If You Need A Short Version

The next agent should:

1. treat the crate split as largely complete
2. optionally thin the CLI/root facade
3. run `\.\check-all.ps1` when ready for the slower final checkpoint
4. avoid rewrites the entire time
