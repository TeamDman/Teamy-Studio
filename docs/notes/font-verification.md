# Text Shaping And Offscreen Render E2E Harness Plan

## Goal

Build a single production-path text correctness harness for Teamy-Studio that validates shaping and rendered output end-to-end through the slug-based shader renderer. The harness must be reusable from `cargo test` and from the runtime self-test CLI, and it must replace software-rendered image comparisons as the correctness oracle.

## Current Status

- Done so far:
  - Shared render verification harness added and wired to both `cargo test` and `self-test render-offscreen`.
  - Built-in fixture `basic-terminal-frame` added with a checked-in expected image at `tests/fixtures/render-offscreen/basic-terminal-frame.png`.
  - The shared harness now also records and compares a renderer-generated scene snapshot alongside the PNG golden so structural regressions are caught before pixel-only diffs.
  - `SelfTestRenderOffscreenArgs` extended with fixture selection, fixture listing, and expected-image update support.
  - `render_frame_model_offscreen_image` now renders through the real D3D12 pipeline into an offscreen texture and reads pixels back for comparison.
  - Offscreen verification defaults to the D3D12 WARP adapter for determinism, with an environment override for hardware validation.
  - Default cargo-test coverage now includes the shared render fixture path, and the runtime CLI can intentionally refresh expected artifacts.
  - Legacy `fontdue` visual-oracle tests were demoted to ignored diagnostic-only coverage instead of remaining part of the default correctness story.
  - CLI spec coverage was updated and full `check-all.ps1` validation, including Tracey, passes after the implementation landed.
- Current focus:
  - Expand beyond the first end-to-end render fixture and move structural verification from renderer fragment snapshots toward true shaped-run coverage rather than per-character quads only.
- Remaining work:
  - Keep expanding fixture coverage now that the harness compares both image output and renderer scene snapshots.
  - Replace fragment-level scene snapshots with true shaped-run expectations once the production text path exposes a shaping abstraction richer than per-character `GlyphQuad`s.
  - Expand the fixture corpus to cover ligatures, combining marks, BiDi, fallback, emoji, and complex-script shaping cases.
  - Feed richer scene fixtures from terminal replay or equivalent transcript-driven inputs.
  - Finish retiring or replacing remaining software-path diagnostics where the shader-path harness now covers the same intent.
- Next step:
  - Regenerate and check in the first scene snapshot golden for `basic-terminal-frame`, then add the next tier of shared fixtures on top of that shared PNG-plus-structure harness.

## Constraints And Assumptions

- `src/cli/self_test/self_test_cli.rs` already exposes:
  - `RenderOffscreen(SelfTestRenderOffscreenArgs)`
  - `TerminalReplay(SelfTestTerminalReplayArgs)`
- The harness should extend that existing diagnostics surface instead of creating a parallel path.
- The production text renderer is slug/shader-based. The harness must call that same path.
- The software renderer may remain as a debug tool, but not as the correctness oracle.
- Determinism requires repo-owned font fixtures, fixed render config, fixed surface format, and explicit tolerance rules.
- Cross-machine exact pixel identity may be unrealistic unless the backend is fixed; CI should prefer a canonical backend when possible.
- If the repo already has conventions for fixtures, artifact output, or test snapshots, reuse them.
- If Teamy-Studio is Tracey-enabled, this change is a new behavior area and should get a dedicated spec.

## Product Requirements

1. The same text fixture can be executed:
   - from `cargo test`
   - from the runtime self-test CLI
2. End-to-end validation uses:
   - the production shaper
   - the production slug draw-data path
   - the production graphics pipeline rendering to an offscreen texture
3. The harness validates both:
   - structural shaping results
   - rendered output
4. Fixtures use repo-controlled fonts and explicit layout/render settings; no implicit system fallback in correctness runs.
5. Failures emit actionable artifacts:
   - actual image
   - expected image
   - diff image or diff summary
   - shaping dump or mismatch summary
6. `cargo test` has a small deterministic smoke suite that runs by default.
7. The runtime self-test can run the same fixtures on demand, select fixtures, and dump artifacts to a directory.
8. Existing software-path image comparisons are removed or demoted once the shader-path harness covers the same intent.
9. The harness can expand from string-level cases to transcript- or scene-level cases using the existing terminal replay surface.

## Architectural Direction

- Introduce a shared internal harness module, separate from CLI code and separate from test glue.
- Keep the core runner layered:

  1. **Fixture loading**
     - text input
     - fonts and fallback set
     - size, DPI, and layout params
     - render target params
     - expected shaping snapshot
     - expected rendered golden

  2. **Production-path execution**
     - shape text with the production shaper
     - build slug draw data with the production renderer input path
     - render to an offscreen texture with the real graphics pipeline
     - read back pixels or coverage

  3. **Comparison and artifact emission**
     - compare shaping snapshot
     - compare image or coverage with explicit tolerance
     - emit artifacts on failure or in update mode

  4. **Adapters**
     - cargo-test adapter
     - runtime self-test adapter

- Reuse `SelfTestCommand::RenderOffscreen` as the first CLI integration point.
- Treat `SelfTestCommand::TerminalReplay` as a later source of scene fixtures rather than a separate correctness system.
- Reference study only:
  - `cosmic-text` for shaping corpus ideas and fallback/BiDi coverage
  - `Slug` and `glyphy` for shader text rendering patterns
  - `ghostty` and `microsoft-terminal` for offscreen GPU test-harness ideas

## Tracey Specification Strategy

- Current status:
  - Tracey is enabled in Teamy-Studio, the CLI spec now covers the new render-offscreen flags, and repository validation currently passes with no Tracey errors.
- If Tracey is enabled:
  - Create a dedicated spec for `text shaping and offscreen render correctness harness`.
  - Do not hide this under an unrelated CLI or rendering spec.
- Initial baseline workflow:
  - `tracey query status`
  - `tracey query uncovered`
  - `tracey query unmapped`
  - `tracey query unmapped --path src/cli/self_test`
  - `tracey query validate --deny warnings`
- Follow-up after implementation lands:
  - `tracey query untested`
- Spec coverage expectations if enabled:
  - map the `self-test render-offscreen` CLI surface
  - map the shared harness entrypoints
  - map cargo-test execution behavior and artifact emission
  - map the removal or demotion of software-oracle behavior

## Phased Task Breakdown

### Phase 1 - Inventory and seam selection

**Objective**
- Identify the exact production-path hooks needed to run shaping and offscreen rendering from shared code.

**Status**
- Complete.

**Tasks**
- Read `src/cli/self_test/render_offscreen.rs`.
- Confirm how `src/cli/self_test/self_test_cli.rs` dispatches into the current offscreen path and whether new flags can be added without breaking the current surface.
- Identify:
  - current shaping entrypoint
  - slug draw-data construction entrypoint
  - offscreen render entrypoint
  - current software-render test helpers or image comparison helpers
- Decide where the shared harness module should live.
- Decide the minimum deterministic fixture contract:
  - fonts
  - size
  - DPI
  - surface format
  - blend mode
  - tolerance model

**Definition of done**
- A short design note exists with chosen entrypoints, module location, and fixture contract.
- There is no unresolved ambiguity about which production renderer path the harness will call.
- The first smoke fixture shape is agreed.

### Phase 2 - Shared fixture and runner core

**Objective**
- Build one reusable runner that both tests and CLI can call.

**Status**
- Complete for the first end-to-end render fixture.

**Tasks**
- Add a shared fixture model:
  - fixture id or name
  - text input and segmentation metadata
  - font set and fallback order
  - shaping and layout params
  - render target params
  - expected shaping snapshot reference
  - expected image golden reference
- Add shared runner APIs for:
  - loading fixtures
  - running shaping
  - rendering to offscreen
  - reading back pixels
  - comparing results
  - emitting failure artifacts
- Keep the runner free of CLI parsing and free of `#[test]`-specific concerns.
- Add update-mode support so goldens can be refreshed intentionally.

**Definition of done**
- A single smoke fixture can be executed from library code.
- The execution path uses the production shaper and shader renderer only.
- The runner can emit a machine-readable result plus human-usable artifacts.

### Phase 3 - Cargo test integration

**Objective**
- Make end-to-end correctness part of normal Rust test workflows.

**Status**
- Complete for the initial smoke fixture; corpus expansion is still pending.

**Tasks**
- Add a cargo-test adapter that invokes the shared runner.
- Start with a default smoke suite:
  - ASCII or basic Latin
  - one ligature case
  - one combining-mark case
  - one non-Latin complex-script case chosen after shaping entrypoint review
- Ensure test output reports:
  - fixture name
  - shaping mismatches
  - image mismatch summary
  - artifact output path
- Define the golden update workflow for local development.
- Decide whether larger GPU cases are default, ignored, or feature-gated.

**Definition of done**
- `cargo test` runs a deterministic smoke set with actionable failures.
- Developers can intentionally refresh expected outputs without hand-editing fixtures.
- No default test depends on the software render path as its oracle.

### Phase 4 - Runtime self-test integration

**Objective**
- Expose the same harness through the runtime diagnostic surface.

**Status**
- Complete for fixture selection, listing, and expected-image update flows.

**Tasks**
- Extend `SelfTestRenderOffscreenArgs` and `src/cli/self_test/render_offscreen.rs` to call the shared runner.
- Add CLI options for:
  - running all fixtures or selected fixtures
  - listing fixtures
  - output directory for artifacts
  - update mode for expected outputs, if allowed
  - machine-readable summary output
- Keep `src/cli/self_test/self_test_cli.rs` unchanged unless the current subcommand surface proves insufficient.
- Ensure runtime self-test and cargo-test adapters share the same fixture files and comparison code.

**Definition of done**
- `self-test render-offscreen` can run the shared fixture set.
- Failures from CLI runs produce the same class of artifacts as `cargo test`.
- There is no duplicated comparison logic between the test adapter and the CLI adapter.

### Phase 5 - Corpus expansion and scene-level correctness

**Objective**
- Expand from smoke cases to the failure modes that matter for terminal and text correctness.

**Status**
- In progress.

**Tasks**
- Add shaping and render fixtures for:
  - BiDi
  - emoji ZWJ and variation selectors
  - combining marks and mark positioning
  - fallback-font selection
  - Arabic or another joining script
  - Devanagari or another complex-script shaping case
  - clipping, overflow, or similar behavior if it is part of product behavior
- Add scene-level fixtures driven from transcript or terminal-state inputs.
- Evaluate whether `TerminalReplay` can feed the same harness for full-scene end-to-end cases.
- Port or replace existing software-path coverage with equivalent shader-path fixtures.

**Definition of done**
- The fixture corpus covers the dominant shaping and render regressions the team cares about.
- At least one scene-level fixture exercises more than a single text run.
- Software-path visual comparisons are removed, ignored, or explicitly marked debug-only.

### Phase 6 - CI hardening, docs, and coverage cleanup

**Objective**
- Make the harness sustainable in CI and easy to operate locally.

**Status**
- Partially complete; deterministic WARP-backed validation is in place and `check-all.ps1` is green, but broader workflow documentation and corpus hardening remain.

**Tasks**
- Lock the CI render configuration:
  - backend selection
  - surface format
  - color space
  - blend state
  - AA mode
  - readback format
- Prefer a deterministic graphics backend for CI when available; on Windows, evaluate the existing runtime backend first and only fall back to a software GPU adapter such as WARP if needed to preserve the real graphics pipeline.
- Document:
  - how to run the smoke suite
  - how to run the CLI self-test
  - how to update goldens
  - how to inspect emitted artifacts
- If Tracey is enabled, add mappings and close uncovered or unmapped gaps for the new harness surface.

**Definition of done**
- CI can run the intended smoke set reliably.
- Local developer workflow is documented.
- Spec and coverage debt introduced by the harness is closed or explicitly tracked.

## Recommended Implementation Order

1. Phase 1 - Inventory and seam selection
2. Phase 2 - Shared fixture and runner core
3. Phase 3 - Cargo test integration with one smoke fixture
4. Phase 4 - Runtime self-test integration through `render-offscreen`
5. Phase 5 - Corpus expansion, transcript or scene coverage, software-oracle retirement
6. Phase 6 - CI hardening, docs, and Tracey cleanup

## Open Decisions

- Should shaping expectations and image expectations live in one fixture manifest or in paired files?
- Should image comparison use full RGBA or normalized alpha or coverage only?
- Which backend should be forced in CI for acceptable determinism?
- Should larger GPU fixtures run by default in `cargo test`, under `#[ignore]`, or behind a feature or env gate?
- Should transcript or scene fixtures be owned by `render-offscreen`, `terminal-replay`, or a shared fixture namespace?
- Is golden updating allowed from the runtime self-test command, or only from development and test workflows?

## First Concrete Slice (Completed)

1. Inspect `src/cli/self_test/render_offscreen.rs` and document its current inputs and outputs.
2. Locate the production text shaping entrypoint and the production offscreen render entrypoint.
3. Add a shared harness module with the smallest possible surface:
   - `Fixture`
   - `RunOptions`
   - `RunResult`
   - `run_fixture(&Fixture, &RunOptions)`
4. Create one repo-owned smoke fixture:
   - fixed font
   - simple ASCII text
   - fixed surface config
   - expected shaping snapshot
   - expected rendered golden
5. Add one cargo test that runs the fixture.
6. Route `self-test render-offscreen` to the same runner for that single fixture.

**Exit criteria for the first slice**
- One fixture passes through the production shaper and shader offscreen renderer from both `cargo test` and the runtime self-test path.
- The result can be compared and produces readable artifacts on failure.
- No software render oracle is involved.