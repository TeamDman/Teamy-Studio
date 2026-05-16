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

Those tests live in `src\app\windows_d3d12_renderer.rs`.

### Built-In Self-Test Commands

After the final affine/fixed-`w` shader change:

- `cargo run -- self-test render-offscreen --fixture transformed-text-plane` completes rendering and reaches normal expected-output checking
- its current failure mode is only that the repo does not have checked-in expected artifacts for that fixture yet
- `cargo run -- self-test render-offscreen --fixture induce-havoc` with `TEAMY_INDUCE_HAVOC_PROFILE=extreme` and `TEAMY_INDUCE_HAVOC_ITERATIONS=1` completes all four phases and produces a normal `RenderOffscreenSelfTestReport`

The important distinction is that these are no longer failing with device removal or queue hangs.

## Important Caveat

The offscreen validation is now much healthier than it was at the start of the investigation, but interactive window behavior still needs explicit confirmation whenever the shader path changes.

At one point during the investigation, the user still observed the live text playground crashing on rotation even though some offscreen checks had improved. The final affine transformed-text change was applied after that report, and the offscreen fixtures now survive, but live manual verification remains the authoritative check for the actual playground UX.

In other words:

- offscreen transformed-text crash reproduction is fixed by current evidence
- the live interactive playground should still be rechecked manually after this change set

## What This Note Did Not Capture Before

The previous version of this note stopped at the initial hypothesis stage. It did not capture:

- that Teamy-Studio already had a useful built-in reproducer in `self-test render-offscreen`
- that `induce-havoc` is the right harness for repeated transformed-text stress
- that several early numerical guards improved the reduced repro but did not fully fix Teamy-Studio
- that the strongest surviving culprit became perspective handling inside transformed text rather than just "small yaw"
- that the applied fix was not only guard logic, but an actual pipeline simplification: transformed text now renders without per-vertex perspective `w` in the shader path
- that Teamy-Studio now has explicit regression tests for both the near-identity repro and the regular rotated transformed-text fixture

## Current Bottom Line

We no longer just have a reduced standalone repro. We now also have a Teamy-Studio-native validation story:

- a minimal external repro in `graphics-driver-crash`
- a near-identity Teamy-Studio regression fixture
- the existing `transformed-text-plane` self-test fixture
- the existing `induce-havoc` stress fixture

The current evidence points to the driver hang being triggered by Teamy-Studio's old perspective-style transformed text path rather than by generic renderer infrastructure. The applied fix was to render transformed text through an affine shader path with direct interpolated UVs and derivative-free transformed coverage. That change removes the device-removal failure from the current offscreen transformed-text checks and gives us regression coverage to keep the issue from silently returning.
