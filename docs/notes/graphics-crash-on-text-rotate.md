# Graphics Crash On Text Rotate

## Situation

We have two useful repro surfaces for the same transformed-text GPU failure:

1. A standalone minimal repro in `G:\Programming\Repos\graphics-driver-crash`.
2. Teamy-Studio's built-in offscreen self-test infrastructure, especially the transformed-text fixtures and the `induce-havoc` diagnostic fixture.

The original standalone repro lives in `G:\Programming\Repos\graphics-driver-crash\src\main.rs` and was reduced to a very small failing case:

- text: `il`
- repeat draws: `1` in the reduced analysis writeup, though the checked-in repro also reproduced with larger repeat counts
- font size: `0.35`
- yaw: `-0.00009`
- pitch: `0.0`

Before the Teamy-Studio fix described below, plain `cargo run` in `G:\Programming\Repos\graphics-driver-crash` reproduced device removal with `DXGI_ERROR_DEVICE_HUNG` / `0x887A0006`.

## What The Reduced Repro Established

The standalone repro ruled out a lot of broad explanations:

- this was not primarily a workload-size problem
- many draws were not required
- large text was not required
- readback and PNG output were not required
- pitch was not required
- a tiny nonzero yaw was enough
- zero transform remained a clean control

That made the bug look much more like a transformed-text correctness problem than a general D3D12 pressure or queue-size problem.

## Teamy-Studio Surfaces That Mattered

The most relevant Teamy-Studio files for this investigation were:

- `G:\Programming\Repos\Teamy-Studio\src\app\windows_panel_shaders.hlsl`
- `G:\Programming\Repos\Teamy-Studio\src\app\windows_d3d12_renderer.rs`
- `G:\Programming\Repos\Teamy-Studio\src\app\render_verification.rs`
- `G:\Programming\Repos\Teamy-Studio\src\cli\self_test\render_offscreen\self_test_render_offscreen_cli.rs`
- `G:\Programming\Repos\Teamy-Studio\src\app\windows_app.rs`
- `G:\Programming\Repos\Teamy-Studio\src\app\windows_scene.rs`

The standalone repro was deliberately derived from Teamy-Studio's transformed text pipeline, so the fix surface stayed in transformed text rendering rather than generic device setup.

## Existing Self-Test Infrastructure

Teamy-Studio already had the right diagnostic harness for this investigation: `self-test render-offscreen`.

The CLI surface is defined in `src\cli\self_test\render_offscreen\self_test_render_offscreen_cli.rs` and supports:

- `--fixture <name>` to run a built-in fixture
- `--list-fixtures` to enumerate them
- `--artifact-output <path>` to write artifacts
- `--update-expected` for normal expected-image fixtures

Important built-in fixtures in `src\app\render_verification.rs`:

- `basic-terminal-frame`
- `transformed-text-plane`
- `induce-havoc`

`induce-havoc` is diagnostic-only and intentionally bypasses expected-image checking. It alternates across multiple phases. In `TEAMY_INDUCE_HAVOC_PROFILE=extreme`, the sequence is:

- `basic-terminal-frame`
- `dense-transformed-text-plane-a`
- `dense-transformed-text-plane-b`
- `transformed-text-plane`

Useful commands during investigation:

```powershell
cargo run -- self-test render-offscreen --fixture transformed-text-plane

$env:TEAMY_INDUCE_HAVOC_PROFILE='extreme'
$env:TEAMY_INDUCE_HAVOC_ITERATIONS='1'
cargo run -- self-test render-offscreen --fixture induce-havoc
```

## What We Observed In Teamy-Studio Before The Final Fix

Before the final shader change, several intermediate changes improved the reduced repro but did not fully fix Teamy-Studio:

- finite-value guards around transformed render coordinates helped the standalone repro
- derivative guards in the debug overlay path were necessary but not sufficient
- the interactive text playground could still crash when text was rotated
- the existing `transformed-text-plane` and `induce-havoc` self-tests were still failing in Teamy-Studio even after the standalone repro stopped hanging

That was an important finding: the reduced repro and the full Teamy-Studio transformed-text path were related, but not identical. The remaining failure depended on something still present in Teamy-Studio's real transformed pipeline.

## Strongest Working Hypothesis That Emerged

By the time the offscreen Teamy-Studio fixtures were isolated, the strongest remaining clue was no longer just "transformed text" in general, but specifically the perspective path inside transformed text:

- transformed glyphs were emitted with varying per-vertex clip-space `w`
- the pixel shader was evaluating transformed text through a specialized transformed branch
- derivative-sensitive coverage logic and perspective-correct interpolation were both in play
- the smaller `transformed-text-plane` fixture still crashed before readback, so the remaining trigger was not exclusive to the dense havoc scenes

The most useful interpretation was that the driver did not like the combination of perspective-transformed glyph quads and the transformed slug evaluation path, even after finite guards were added.

## The Fix That Actually Moved Teamy-Studio

The final change that stopped the built-in Teamy-Studio transformed-text crashes was to make the transformed text path affine in shader space instead of perspective-correct.

In `src\app\windows_panel_shaders.hlsl`:

- transformed glyph vertices now call `transformed_clip_from_screen(position, 1.0)` instead of using per-vertex `corner_w`
- the transformed text pixel path now uses the directly interpolated `input.uv`
- transformed coverage uses `slug_coverage_transformed(...)`, a derivative-free helper that calls the shared coverage routine with a fixed `pixelsPerEm = float2(1.0, 1.0)`
- finite guards remain in the transformed branch and debug path

This means the transformed text path is still rendered from the transformed quad positions, but the GPU is no longer being asked to do perspective-correct interpolation for glyph UV evaluation inside this path.

The relevant points in the shader now are:

- `VSMain(...)` transformed branch uses fixed `w = 1.0`
- `slug_coverage_transformed(...)` exists as a transformed-specific fallback
- `PSMain(...)` selects `slug_coverage_transformed(...)` for transformed glyphs

## Current Validation State

### Standalone Repro

After the early shader guard work, `cargo run` in `G:\Programming\Repos\graphics-driver-crash` completed without device removal for the reduced `il` repro.

### Teamy-Studio Offscreen Tests

These checks now succeed far enough to prove the driver-hang path is gone:

- `cargo test --lib near_identity_transformed_text_offscreen_render_completes`
- `cargo test --lib rotated_transformed_text_offscreen_render_completes`
- `cargo test --release --lib strong_yaw_zoomed_transformed_text_matches_cpu_glyph_reference`
- `cargo test --release --lib large_lorem_strong_yaw_transformed_text_offscreen_render_completes`

Those tests live in `src\app\windows_d3d12_renderer.rs`.

### Built-In Self-Test Commands

After the final affine/fixed-`w` shader change:

- `cargo run -- self-test render-offscreen --fixture transformed-text-plane` completes rendering and reaches normal expected-output checking
- its current failure mode is only that the repo does not have checked-in expected artifacts for that fixture yet
- `cargo run -- self-test render-offscreen --fixture induce-havoc` with `TEAMY_INDUCE_HAVOC_PROFILE=extreme` and `TEAMY_INDUCE_HAVOC_ITERATIONS=1` completes all four phases and produces a normal `RenderOffscreenSelfTestReport`

The important distinction is that these are no longer failing with device removal or queue hangs.

## What Happened Next

The affine/fixed-`w` shader change was a real milestone, but it was not the end of the investigation.

Two more things happened after the original version of this note would have stopped:

1. We had to recover transformed-text correctness after the first crash-oriented shader simplification.
2. We still had a live-only crash in the interactive window even after the offscreen fixtures were healthy.

### Later Transformed-Text Correctness Work

After the initial affine workaround, the interactive text plane showed visual problems under stronger yaw and zoom:

- bowed glyph cross-bars
- foveated-looking distortion when rotating left and right
- vanishing columns / missing parts of glyphs at angle
- structured ring or shadow artifacts

The validated transformed-text path eventually moved away from the earliest affine shortcut and settled on a more correct formulation:

- per-pixel transformed render-coordinate reconstruction
- a per-glyph inverse-homography buffer uploaded from `src\app\windows_d3d12_renderer.rs`
- a transformed path that walks all curves for coverage in the difficult strong-yaw cases

That later work is why the current transformed-text regression coverage is broader than the older note captured.

### The Remaining Live Crash

Even after the offscreen fixtures survived, the live Teamy-Studio window could still crash the GPU driver during vigorous text rotation, especially in release builds.

The key diagnostic improvement was to thread `GetDeviceRemovedReason` into the live render-thread failure path instead of only reporting the top-level HRESULT. Once that was wired through, the live failure became much more specific:

- the live window reported device removal while waiting for frame latency
- `GetDeviceRemovedReason` returned `0x887A0007`
- the message text indicated that the GPU had received invalid commands

That changed the working theory. The remaining issue no longer looked like only a shader-math problem. It looked like a renderer-side lifetime or synchronization problem that only showed up under continuous interactive redraw.

## The Renderer-Side Fix That Appears To Have Closed The Live Crash

The strongest live-only bug we found was shared upload-buffer reuse across frames.

`src\app\windows_d3d12_renderer.rs` uses shared resources for per-frame uploads, including:

- curve data
- band data
- transformed glyph inverse data
- vertex data

Those buffers were being rewritten for a new frame before the previously submitted frame was guaranteed to be finished on the GPU. That is much easier to trigger in the live rotating text plane than in the offscreen harness, because offscreen verification naturally renders one frame at a time.

The applied fix was to serialize reuse of those shared upload buffers by waiting for the previously submitted frame to complete before building and uploading the next live frame.

Concretely, the renderer now has a `wait_for_last_submitted_frame()` gate, and the live paths call it before they overwrite shared upload resources in:

- `render_fragments(...)`
- `render_frame_model(...)`

That change is much more consistent with the final live symptom (`0x887A0007` invalid commands while rotating continuously) than the earlier offscreen-only crash theory.

## Important Caveat

Historically, the most important lesson from this investigation was that offscreen health did not guarantee live interactive safety.

At one point during the investigation, the user still observed the live text playground crashing on rotation even though some offscreen checks had improved. That turned out to matter: the remaining bug was real, and it lived in the interactive renderer path rather than in the narrow offscreen reproducer.

In other words:

- offscreen transformed-text crash reproduction was an important milestone, not the full finish line
- live interactive playground validation remained the authoritative check
- the current renderer/upload-buffer fix should still be treated that way in future regressions too

## What This Note Did Not Capture Before

The previous version of this note stopped at the initial hypothesis stage. It did not capture:

- that Teamy-Studio already had a useful built-in reproducer in `self-test render-offscreen`
- that `induce-havoc` is the right harness for repeated transformed-text stress
- that several early numerical guards improved the reduced repro but did not fully fix Teamy-Studio
- that the strongest surviving culprit became perspective handling inside transformed text rather than just "small yaw"
- that the applied fix was not only guard logic, but an actual pipeline simplification: transformed text now renders without per-vertex perspective `w` in the shader path
- that Teamy-Studio now has explicit regression tests for both the near-identity repro and the regular rotated transformed-text fixture
- that later transformed-text correctness work replaced the first crash-oriented shortcut with a more accurate per-pixel reconstruction path
- that `GetDeviceRemovedReason` in the live renderer eventually pointed at `0x887A0007` invalid commands rather than the original offscreen `DXGI_ERROR_DEVICE_HUNG` symptom
- that the remaining live crash appears to have been caused by reusing shared upload buffers before the previous frame had finished on the GPU
- that the renderer now waits for the previously submitted frame before overwriting shared upload data for the next live frame
- that a heavier release-mode transformed-text regression now exists to approximate the real large text-plane workload
- that the latest manual live verification indicates the interactive crash is fixed

## Current Bottom Line

We no longer just have a reduced standalone repro or a single shader-side theory. We now have a staged history that matters:

- a minimal external repro in `graphics-driver-crash`
- a set of Teamy-Studio offscreen transformed-text regressions, including strong-yaw and heavier large-text coverage
- the existing `transformed-text-plane` and `induce-havoc` self-test fixtures
- a live renderer diagnostic path that reports `GetDeviceRemovedReason`
- a renderer-side synchronization fix for shared upload-buffer reuse between frames

The historical evidence still supports the original conclusion that the old transformed-text shader path was a real part of the problem. But the full story is broader: the final live failure also exposed a renderer-side frame-lifetime bug that offscreen checks did not naturally exercise.

The current best reading is:

- old perspective-style transformed text triggered the original offscreen driver-hang class of failures
- later correctness work restored transformed rendering quality without reintroducing that offscreen crash
- the last live interactive crash appears to have been caused by shared upload-buffer reuse before GPU completion
- waiting for the previous submitted frame before reusing those upload buffers appears to have fixed the live rotation crash

At the time of this update, the user has manually verified that the latest renderer-side change seems to have fixed the live crash. That should be treated as the strongest current signal, with the offscreen regressions and self-test fixtures serving as the supporting regression net rather than the only source of truth.
