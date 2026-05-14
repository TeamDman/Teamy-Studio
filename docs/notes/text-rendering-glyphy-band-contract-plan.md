# Text Rendering Glyphy Band Contract Plan

## Goal

Make Teamy-Studio's outline text renderer behave like the intended glyphy-style reference path for both flat and transformed text by replacing the simplified per-band curve-selection contract with directional band metadata and split-based ray selection.

The target outcome is not a cosmetic tweak. The target is a renderer contract where:

- slug snapshots such as `g-256.png` no longer show the internal false vertical-line artifacts the user called out
- transformed text uses the same coverage semantics as flat text instead of inheriting a broken subset-selection path
- the offscreen correctness harness can measure real progress against stable artifacts instead of masking structural defects with ad hoc shader heuristics

## Current Status

- Done so far:
	- Built and expanded the offscreen render verification harness and exposed the transformed-text fixture through `self-test render-offscreen`.
	- Added a zero-angle transformed correctness invariant and related CPU reference helpers.
	- Confirmed the offscreen path had a real bug where transformed projection data was not uploaded; that plumbing is fixed.
	- Confirmed the CPU band-pruning path is internally self-consistent: `banded_cpu_coverage_matches_full_curve_walk` passes even for suspect glyphs such as `g` and `6`.
	- Regenerated the slug and fontdue diagnostic artifacts and verified that the user-reported `g-256.png` defect comes from the shared slug coverage path, not only from transformed projection.
	- Compared Teamy-Studio's banded coverage implementation against glyphy's fragment and encode paths and identified the controlling gap: Teamy-Studio stores only one sorted curve list per band, while glyphy stores a split value plus both descending and ascending lists for directional ray selection.
- Current focus:
	- Port glyphy's directional band-header contract and split-selection rule into Teamy-Studio's band buffer, CPU coverage path, and HLSL shader.
- Remaining work:
	- Replace the current two-u32 band header with a four-u32 directional band header.
	- Emit both descending and ascending curve-index lists per horizontal and vertical band.
	- Implement the glyphy split search so each band chooses the better ray direction at sample time.
	- Update CPU and shader coverage logic to consume the new header and choose left/right traversal per sample.
	- Re-baseline transformed-text sampling once the shared coverage contract is correct.
	- Add regression coverage that directly measures the `g` artifact and the transformed-text fixture after the contract lands.
- Next step:
	- Land the new band data format and make the CPU coverage helpers consume it first, then port the matching shader logic.

## Constraints And Assumptions

- The repo requires `./check-all.ps1` for broad validation; do not rely on alternate cargo target directories.
- If local executables are locked, `./stop.ps1` is the approved cleanup path.
- The text renderer currently lives in `src/app/windows_d3d12_renderer.rs` and `src/app/windows_panel_shaders.hlsl`; the redesign should stay rooted there instead of creating a parallel text path.
- The current offscreen harness and slug snapshot workflow are valuable and should remain the primary correctness loop while this work lands.
- The current transformed-text zero-angle invariant is informative but not yet the sole decision-maker because it still depends on a CPU reference model that shares some assumptions with the live path.
- Existing exploratory transformed-text edits are acceptable to replace wholesale if the new coverage contract renders them unnecessary.

## Product Requirements

1. Large slug snapshots for glyphs such as `g` must stop showing the internal false line artifacts currently visible in `target/test-artifacts/slug/g-256.png`.
2. Flat text and transformed text must share one coverage contract; transformed text must not rely on a second-class approximation path.
3. The offscreen transformed-text fixture must become visually stable enough to promote to a checked-in expected image and scene snapshot.
4. Coverage improvements must be expressed through production data structures and shader inputs, not only through diagnostic-only code paths.
5. The render verification harness must gain regression checks that directly target the shared coverage defect, not only broad end-to-end images.
6. The final validation loop must include focused tests plus `./check-all.ps1` before the work is considered done.

## Architectural Direction

- Keep the current curve representation and glyph loading pipeline.
- Replace only the band-selection contract and its consumers.
- Move from the current Teamy-Studio band format:
	- per band: `(count, index_start)`
	- one pre-sorted list for horizontal bands and one for vertical bands
- To a glyphy-like band format:
	- per band: `(count, descending_index_start, ascending_index_start, split_value_bits)`
	- descending list sorted by `max_x` for horizontal and `max_y` for vertical
	- ascending list sorted by `min_x` for horizontal and `min_y` for vertical
	- split value selected to minimize the worse of left-ray and right-ray curve counts for that band
- Teach both the CPU and HLSL coverage evaluators to:
	- choose ray direction from the split value and sample coordinate
	- use the correct list and early-exit predicate for that direction
	- use symmetric coverage accumulation (`0.5 - r` vs `r + 0.5`) just like glyphy
- After the shared coverage path is corrected, simplify transformed text so it consumes that same coverage contract without extra geometry heuristics unless a later focused check proves they are still needed.

## Tracey Specification Strategy

- Current status:
	- Tracey is enabled for the repo, but there is no dedicated text-rendering or font-rendering spec yet in `.config/tracey/config.styx`.
- Strategy:
	- Create a dedicated product spec for text rendering correctness rather than hiding this under generic behavior or windowing specs.
	- The spec should cover:
		- slug snapshot correctness and diagnostic artifact workflows
		- offscreen transformed-text verification behavior
		- the shared glyph coverage contract used by flat and transformed text
- Baseline commands:
	- `tracey query status`
	- `tracey query uncovered`
	- `tracey query unmapped --path src/app/windows_d3d12_renderer.rs`
	- `tracey query unmapped --path src/app/windows_panel_shaders.hlsl`
	- `tracey query validate --deny warnings`
- Follow-up after implementation stabilizes:
	- `tracey query untested`

## Phased Task Breakdown

### Phase 1 - Directional Band Contract In Rust

**Objective**
- Change the serialized band buffer to carry the same directional metadata the final coverage code needs.

**Tasks**
- Add a small internal representation for a band header with:
	- curve count
	- descending-list offset
	- ascending-list offset
	- split value
- Rewrite `append_slug_band_data(...)` to build both descending and ascending band lists.
- Port glyphy's split-search rule for horizontal and vertical bands.
- Update the CPU band-data readers to understand the new header layout.
- Preserve current `SlugGlyph` public shape where possible so the rest of the renderer does not churn unnecessarily.

**Definition of done**
- The band buffer layout is no longer `(count, start)`.
- CPU coverage code can read the new headers without panics or out-of-bounds access.
- Focused CPU parity tests still pass under the new format.

### Phase 2 - Directional Coverage In CPU Helpers

**Objective**
- Make the CPU snapshot path and CPU reference helpers use the same directional selection logic as glyphy.

**Tasks**
- Replace the current single-direction early-exit logic in `cpu_slug_coverage*` helpers.
- Add helpers that choose left/right traversal from the band split.
- Port the symmetric coverage formulas for leftward rays.
- Keep banded-vs-full-curve parity tests up to date under the new semantics.
- Add a regression that quantifies the `g` artifact against `fontdue` or against a repo-owned expected snapshot.

**Definition of done**
- CPU slug snapshots improve on the previously reported `g` artifact.
- CPU-focused tests stay green.
- The new regression can fail when the old artifact returns.

### Phase 3 - Directional Coverage In HLSL

**Objective**
- Port the same directional contract into the production shader path.

**Tasks**
- Update HLSL band-header loading from `uint2` to a directional header.
- Teach the shader to choose direction per sample from the split value.
- Port glyphy's early-exit rules and symmetric left/right coverage accumulation.
- Remove temporary transformed-only workarounds that become obsolete once the shared coverage path is correct.
- Keep shader entry-point compilation tests passing.

**Definition of done**
- Flat text visibly tracks the corrected CPU slug path.
- Shader compile tests pass.
- The production render path no longer depends on the simplified one-direction band traversal.

### Phase 4 - Rebaseline Transformed Text

**Objective**
- Re-evaluate transformed text once the shared coverage contract is correct.

**Tasks**
- Simplify transformed pixel sampling to the smallest contract consistent with the corrected shared coverage path.
- Re-run the zero-angle transformed invariant and inspect the transformed fixture image.
- Decide whether remaining transformed drift is now strictly projection/homography-related or already resolved by the coverage fix.
- Check in the transformed fixture golden once it is stable.

**Definition of done**
- The transformed fixture no longer shows the user-reported dither / hole artifact.
- The zero-angle transformed invariant is either passing or narrowed to a clearly transformed-only defect.

### Phase 5 - Spec, Docs, And Full Validation

**Objective**
- Finish the work as a maintainable renderer contract, not as a local patch set.

**Tasks**
- Add or extend the text-rendering spec under `docs/spec/product/`.
- Map the relevant implementation surfaces in Tracey.
- Update `docs/notes/slug-font-renderer.md` so it describes the new band contract instead of the now-obsolete simplified version.
- Run focused tests, artifact regeneration, Tracey validation, and `./check-all.ps1`.

**Definition of done**
- Specs and notes describe the implemented contract.
- Tracey validation passes.
- Broad repo validation passes.

## Recommended Implementation Order

1. Redesign the Rust band-buffer format.
2. Port the CPU coverage readers and keep CPU parity tests green.
3. Add the `g`-focused regression.
4. Port the matching HLSL directional coverage logic.
5. Re-assess transformed text only after the shared coverage path is correct.
6. Finish spec, notes, and full validation.

## Open Decisions

- Whether the current transformed CPU reference should remain the oracle after the shared coverage fix, or whether it should be replaced with a different fixture comparison.
- Whether 2x2 supersampling remains necessary after the directional band contract lands, or whether it should be scaled back once the structural coverage defect is removed.
- Whether to encode split values as raw `f32` bits in the band buffer or adopt a more glyphy-like quantized representation.
- Whether the final `g` regression should compare against `fontdue` numerically or against a checked-in expected snapshot produced by Teamy-Studio itself.

## First Concrete Slice

1. Introduce a directional band-header layout in `src/app/windows_d3d12_renderer.rs`.
2. Rewrite `append_slug_band_data(...)` to emit both descending and ascending lists plus per-band split values.
3. Update CPU readers and focused tests to use the new header.
4. Only then move to the shader.
