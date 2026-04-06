# Slug Font Renderer Notes

Goal: capture the current Teamy Studio text renderer design, its known fidelity gaps, and the shortest path to debugging the visible artifacting.

## Current Architecture

- Teamy Studio no longer uses the old fixed startup glyph atlas for terminal text.
- The D3D12 text path now loads the installed `CaskaydiaCove Nerd Font Mono` through `fontdb`.
- Glyph outlines are parsed with `ttf-parser` and converted into quadratic curve records.
- The GPU text path evaluates analytic coverage from those curves in `src/app/windows_panel_shaders.hlsl`.
- The text vertex path now uses object-space glyph quads, per-vertex normals, an inverse-Jacobian mapping, and a Slug-style matrix/viewport constant buffer so dilation happens in the same coordinate space as the original Slug shaders.
- Glyphs now also carry precomputed band tables and band transforms so both the shader path and the CPU snapshot path consume horizontal and vertical curve subsets instead of walking every curve for every sample.
- Terminal text and notebook/output text still share the same renderer backend, but their interactive scale factors are now tracked independently in the window state.

## What This Is And Is Not

What it is:
- a real outline-driven renderer backed by the installed font
- enough of a Slug-style curve pipeline to remove the atlas-slot hacks and render Powerline and Unicode glyphs from the actual font outlines

What it is not yet:
- a full Slug implementation
- a faithful port of Slug's full banded data model and matrix-driven shader contract

## Known Limitations

- We now use per-glyph band tables to select curve subsets, but the storage format is still a simplified packed buffer rather than a faithful port of Slug's original band texture layout.
- The current shader path now follows the horizontal-band and vertical-band split and uses a Slug-style matrix/viewport constant buffer, but it still uses a reduced contract compared with the original packed texture/flag pipeline.
- Flat stems and flat caps on glyphs like `b` still show softer / denser edge pixels than the VS Code terminal reference, so the remaining fidelity gap is not explained by quad padding alone.
- Cubic outlines are now recursively approximated as quadratic segments, but this path still needs validation against fonts that rely heavily on cubic CFF outlines.
- The previous line-only curve special case has been removed so both CPU and GPU coverage paths now rely on the same quadratic math that the original Slug shader uses.

## First-Principles Artifacting Analysis

Observed symptom:
- Diagonal and complex strokes show visible artifacting that becomes easier to see when zooming in.
- Flat vertical and horizontal edges on glyphs like `b` also show nonuniform antialiasing compared with reference terminal renderers.

Likely causes ranked by expected visual impact:

1. Cubic outline conversion still needs validation.
- Cubic segments are now subdivided into quadratic approximations instead of being collapsed to lines.
- This removes a known geometry error, but it still needs targeted verification against fonts that actually exercise the cubic path.

2. The renderer still does not implement Slug's full reference contract.
- A more Slug-like object-space dilation / inverse-Jacobian / matrix path is now in place.
- Band selection and sorted curve subsets are now in place.
- However, the exact reference texture layout, shader flags, and full matrix-driven contract are still missing.
- Remaining flat-edge softness suggests the next correctness gap is deeper than quad expansion alone.

3. Sample-window alignment may still not match reference rasterizers.
- Glyph-specific bounds are now used for quad mapping, but there may still be baseline or sample-window mismatches relative to production terminal renderers.
- That can show up most clearly on stems and flat edges where the expected result is visually unforgiving.

4. The remaining fidelity gap is now more likely in coverage details than coarse data flow.
- We have moved from full-glyph curve walks to banded subset evaluation in both CPU and GPU code.
- If flat-edge mismatch persists, the next likely causes are sample alignment, dilation math, or differences from Slug's exact data contract rather than the absence of band partitioning itself.

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
2. Compare the new banded snapshot outputs and `fontdue` diffs for `b` and `r` to see whether the remaining error is concentrated along sample-window alignment or dilation edges.
3. Add reduced regression cases for flat-edge glyphs like `b` once we can express the expected behavior robustly.
4. Validate the new cubic-to-quadratic path against a font that actually uses cubic outlines.
5. If fidelity is still off after that, move closer to Slug's original packed flags / matrix contract instead of further tweaking broad geometry heuristics.