# Slug Font Renderer Notes

Goal: capture the current Teamy Studio text renderer design, its known fidelity gaps, and the shortest path to debugging the visible artifacting.

## Current Architecture

- Teamy Studio no longer uses the old fixed startup glyph atlas for terminal text.
- The D3D12 text path now loads the installed `CaskaydiaCove Nerd Font Mono` through `fontdb`.
- Glyph outlines are parsed with `ttf-parser` and converted into quadratic curve records.
- The GPU text path evaluates analytic coverage from those curves in `src/app/windows_panel_shaders.hlsl`.
- The text vertex path now carries per-vertex normals, an inverse-Jacobian mapping, and viewport data so glyph quads can be dilated in a more Slug-like way in the vertex shader.
- Terminal text and notebook/output text still share the same renderer backend, but their interactive scale factors are now tracked independently in the window state.

## What This Is And Is Not

What it is:
- a real outline-driven renderer backed by the installed font
- enough of a Slug-style curve pipeline to remove the atlas-slot hacks and render Powerline and Unicode glyphs from the actual font outlines

What it is not yet:
- a full Slug implementation
- a band-accelerated renderer
- a faithful port of Slug's full banded data model and matrix-driven shader contract

## Known Limitations

- We are not using band-acceleration tables yet.
- We currently walk all curves for the glyph during coverage evaluation instead of using Slug's band partitioning.
- The current shader path still uses a simplified all-curves coverage formulation rather than the full Slug band pipeline.
- Flat stems and flat caps on glyphs like `b` still show softer / denser edge pixels than the VS Code terminal reference, so the remaining fidelity gap is not explained by quad padding alone.
- Cubic outlines are now recursively approximated as quadratic segments, but this path still needs validation against fonts that rely heavily on cubic CFF outlines.

## First-Principles Artifacting Analysis

Observed symptom:
- Diagonal and complex strokes show visible artifacting that becomes easier to see when zooming in.
- Flat vertical and horizontal edges on glyphs like `b` also show nonuniform antialiasing compared with reference terminal renderers.

Likely causes ranked by expected visual impact:

1. Cubic outline conversion still needs validation.
- Cubic segments are now subdivided into quadratic approximations instead of being collapsed to lines.
- This removes a known geometry error, but it still needs targeted verification against fonts that actually exercise the cubic path.

2. The renderer still does not implement Slug's full banded pipeline.
- A more Slug-like per-vertex dilation / inverse-Jacobian path is now in place.
- However, band selection, sorted curve subsets, and the exact reference shader contract are still missing.
- Remaining flat-edge softness suggests the next correctness gap is deeper than quad expansion alone.

3. Sample-window alignment may still not match reference rasterizers.
- Glyph-specific bounds are now used for quad mapping, but there may still be baseline or sample-window mismatches relative to production terminal renderers.
- That can show up most clearly on stems and flat edges where the expected result is visually unforgiving.

4. Missing band tables are a performance problem first.
- The lack of band acceleration should mainly affect cost, not baseline correctness.
- It is worth implementing later, but it is not the first thing to blame for the visible artifacts in the screenshots.

## Snapshot Workflow

The codebase now exposes an offscreen glyph snapshot entrypoint:
- `teamy_studio::app::write_slug_snapshot_png(...)`

Additional debug references now exist:
- the ignored integration snapshot harness emits `/`, `b`, `r`, and the Unicode sheet
- renderer unit tests can also emit `fontdue` reference rasters for selected glyphs so we can compare our shader path against an independent CPU rasterizer

Purpose:
- render a single glyph such as `/` to a PNG without opening a window
- make debugging repeatable and comparable across renderer changes

Current harness:
- `cargo test --test slug_snapshot -- --ignored --nocapture`

Artifact output:
- `target/test-artifacts/slug/slash-256.png`
- `target/test-artifacts/slug/b-256.png`
- `target/test-artifacts/slug/r-256.png`
- `target/test-artifacts/slug/unicode-sheet.png`
- `target/test-artifacts/slug/unicode-sheet-index.txt`
- `target/test-artifacts/slug/b-fontdue-256.png`
- `target/test-artifacts/slug/r-fontdue-256.png`

Why this matters:
- the artifacting problem should be debugged on a single large glyph first
- once `/` is clean, the same workflow can be reused for `\`, `>`, box drawing glyphs, and Powerline symbols

## Recommended Next Work

1. Use the new `fontdue` reference rasters to compare stem edges and isolate whether the remaining mismatch is geometry placement or coverage accumulation.
2. Implement the remaining Slug band-selection / sorted-curve machinery instead of walking every curve for every pixel.
3. Add reduced regression cases for flat-edge glyphs like `b` once we can express the expected behavior robustly.
4. Validate the new cubic-to-quadratic path against a font that actually uses cubic outlines.