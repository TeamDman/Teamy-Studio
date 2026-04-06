# Slug Font Renderer Notes

Goal: capture the current Teamy Studio text renderer design, its known fidelity gaps, and the shortest path to debugging the visible artifacting.

## Current Architecture

- Teamy Studio no longer uses the old fixed startup glyph atlas for terminal text.
- The D3D12 text path now loads the installed `CaskaydiaCove Nerd Font Mono` through `fontdb`.
- Glyph outlines are parsed with `ttf-parser` and converted into quadratic curve records.
- The GPU text path evaluates analytic coverage from those curves in `src/app/windows_panel_shaders.hlsl`.
- Terminal text and notebook/output text still share the same renderer backend, but their interactive scale factors are now tracked independently in the window state.

## What This Is And Is Not

What it is:
- a real outline-driven renderer backed by the installed font
- enough of a Slug-style curve pipeline to remove the atlas-slot hacks and render Powerline and Unicode glyphs from the actual font outlines

What it is not yet:
- a full Slug implementation
- a band-accelerated renderer
- a faithful port of Slug's vertex dilation and Jacobian-aware raster contract

## Known Limitations

- We are not using band-acceleration tables yet.
- We currently walk all curves for the glyph during coverage evaluation instead of using Slug's band partitioning.
- The current shader path uses a simplified coverage formulation rather than the full Slug pipeline.
- Glyph placement still leans on cell metrics more than glyph-specific bounds and side bearings.
- Cubic outlines are not handled correctly yet: `QuadraticCurveBuilder::curve_to(...)` currently degrades cubic segments into straight lines.

## First-Principles Artifacting Analysis

Observed symptom:
- Diagonal and complex strokes show visible artifacting that becomes easier to see when zooming in.

Likely causes ranked by expected visual impact:

1. Cubic outlines are currently wrong.
- If a glyph contains cubic segments and the builder collapses them into line segments, the resulting curve field is not an approximation of the original outline. It is a geometry error.
- That can directly produce broken diagonals, kinks, missing curvature, and uneven coverage.

2. The renderer does not yet implement Slug's full dilation and Jacobian contract.
- Slug's published pipeline is not just curve evaluation in the pixel shader.
- The quality story depends on how glyph bounds, dilation, and local transform scale are carried into the coverage stage.
- Our simplified mapping from cell-space UVs to font-space coordinates is plausible, but it is not yet proven correct for every stroke orientation and zoom level.

3. Glyph layout still uses coarse cell metrics.
- The current placement path is good enough to get text on screen, but it does not yet fully respect glyph-specific bounds.
- That can bias sampling windows and make edge behavior look worse, especially for narrow punctuation and diagonals such as `/`.

4. Missing band tables are a performance problem first.
- The lack of band acceleration should mainly affect cost, not baseline correctness.
- It is worth implementing later, but it is not the first thing to blame for the visible artifacts in the screenshots.

## Snapshot Workflow

The codebase now exposes an offscreen glyph snapshot entrypoint:
- `teamy_studio::app::write_slug_snapshot_png(...)`

Purpose:
- render a single glyph such as `/` to a PNG without opening a window
- make debugging repeatable and comparable across renderer changes

Current harness:
- `cargo test snapshot_single_glyph_slash_png -- --ignored`

Artifact output:
- `target/test-artifacts/slug/slash-256.png`

Why this matters:
- the artifacting problem should be debugged on a single large glyph first
- once `/` is clean, the same workflow can be reused for `\`, `>`, box drawing glyphs, and Powerline symbols

## Recommended Next Work

1. Fix cubic outline handling in `QuadraticCurveBuilder::curve_to(...)`.
2. Add a second snapshot case for a Powerline glyph and a box-drawing glyph.
3. Move glyph placement from cell-only assumptions toward glyph bounds and side bearings.
4. Add band-acceleration tables after geometry correctness is established.