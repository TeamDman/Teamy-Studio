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

1. `77c5ef3` `Extract terminal and shell crates`
2. `e26e1f7` `Document remaining terminal extraction blockers`
3. `f7a3619` `Extract model, logs, and audio input crates`
4. `7e3288e` `Extract timeline and replay crates`
5. `4d5fb9e` `Extract terminal support crates`
6. `bad1e8f` `Split support modules into workspace crates`

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
- `teamy_studio_cursor_info`
  - owns [src/app/windows_cursor_info_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/app/windows_cursor_info_impl.rs)
  - includes `CursorInfoVirtualSession`, which is still used by the app host

The old module paths remain alive through thin shims in `src/` and `src/app/`.

## What Is Still Root-Local

The root crate still directly owns the last major orchestration layer:

- [src/app/windows_app.rs](/G:/Programming/Repos/Teamy-Studio/src/app/windows_app.rs)
- [src/app/render_verification.rs](/G:/Programming/Repos/Teamy-Studio/src/app/render_verification.rs)
- [src/app/render_verification_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/app/render_verification_impl.rs)
- [src/app/mod.rs](/G:/Programming/Repos/Teamy-Studio/src/app/mod.rs)
- [src/logging_init.rs](/G:/Programming/Repos/Teamy-Studio/src/logging_init.rs)
- [src/cli](/G:/Programming/Repos/Teamy-Studio/src/cli)
- [src/lib.rs](/G:/Programming/Repos/Teamy-Studio/src/lib.rs)

Of these, the real remaining monolith is `windows_app.rs`.

## The Main Remaining Blocker

`render_verification` was intentionally **not** moved into `teamy_studio_shell`.

Reason:

- [src/app/render_verification_impl.rs](/G:/Programming/Repos/Teamy-Studio/src/app/render_verification_impl.rs) still calls `build_text_rendering_plane_verification_geometry(...)`
- that helper currently lives in [src/app/windows_app.rs](/G:/Programming/Repos/Teamy-Studio/src/app/windows_app.rs)

The relevant helper cluster inside `windows_app.rs` includes:

- `build_text_rendering_plane_verification_geometry(...)`
- `TextRenderingViewportState`
- `text_rendering_projection_basis(...)`
- `rotate_text_rendering_vector_3d(...)`
- `inverse_rotate_text_rendering_vector_3d(...)`
- `build_text_rendering_glyph_instances(...)`

These are the key seam to resolve before the final host extraction becomes clean.

## Recommended Finish Sequence

### 1. Finish the shell seam

Goal:

- make `render_verification` fully independent of `windows_app`

Recommended move:

1. move the text-rendering verification helper cluster out of `windows_app.rs`
2. place it in `teamy_studio_shell`
3. update `render_verification_impl.rs` to use the shell-owned helpers
4. then move `render_verification_impl.rs` into `teamy_studio_shell`
5. turn [src/app/render_verification.rs](/G:/Programming/Repos/Teamy-Studio/src/app/render_verification.rs) back into a thin shim

Guideline:

- keep the helper code intact
- move the helper cluster together
- do not redesign the text-rendering feature while extracting it

### 2. Extract the app host

Goal:

- move [src/app/windows_app.rs](/G:/Programming/Repos/Teamy-Studio/src/app/windows_app.rs) into a new crate, likely `teamy_studio_app_host`

Recommended pattern:

1. move `src/app/windows_app.rs` to `src/app/windows_app_impl.rs`
2. add `crates/teamy_studio_app_host`
3. compile `windows_app_impl.rs` through that crate
4. leave [src/app/windows_app.rs](/G:/Programming/Repos/Teamy-Studio/src/app/windows_app.rs) as a thin shim
5. keep [src/app/mod.rs](/G:/Programming/Repos/Teamy-Studio/src/app/mod.rs) as the public facade initially

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

- do not try to redesign `app/mod.rs` and `windows_app.rs` at the same time
- move the code first, then decide whether the facade should shrink further

### 3. Optional observability cleanup

After `windows_app` is out, consider extracting:

- [src/logging_init.rs](/G:/Programming/Repos/Teamy-Studio/src/logging_init.rs)

Likely crate:

- `teamy_studio_observability`

This is lower-risk and lower-priority than finishing the app host.

### 4. Optional CLI/root thinning

If we want to complete the original workspace shape beyond compile-time wins:

- keep shrinking [src/cli](/G:/Programming/Repos/Teamy-Studio/src/cli)
- possibly create `teamy_studio_cli`

This is not required to finish the core compile-time-oriented transition.

## Recommended Order For Another Agent

If another agent is continuing from here, the safest order is:

1. resolve the `render_verification` <-> `windows_app` helper dependency
2. move `render_verification` fully into `teamy_studio_shell`
3. extract `windows_app.rs` into `teamy_studio_app_host`
4. run `cargo clippy -- -D warnings`
5. only after that consider `logging_init` or CLI cleanup

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
- `render_verification` currently uses `include!("render_verification_impl.rs");` so it still sees the old `super::...` parent structure.

These are all preservation-first tactics, not architectural mistakes.

## Definition Of Done

The transition is "finished enough" when:

1. `render_verification` no longer depends on helper code inside `windows_app`
2. `windows_app.rs` is extracted into its own crate with a thin root shim
3. `cargo clippy -- -D warnings` still passes
4. `.\check-all.ps1` passes at the final checkpoint
5. behavior is preserved

## If You Need A Short Version

The next agent should:

1. move the text-rendering verification helper cluster out of `windows_app.rs`
2. complete the `render_verification` move into `teamy_studio_shell`
3. extract `windows_app.rs` into `teamy_studio_app_host`
4. avoid rewrites the entire time
