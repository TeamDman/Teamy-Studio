# Graphics Crash On Text Rotate

## Situation

We have a standalone self-contained repro in `G:\Programming\Repos\graphics-driver-crash` for the same class of GPU crash that Teamy-Studio hits when rendering rotated/transformed text.

The repro implementation lives in `G:\Programming\Repos\graphics-driver-crash\src\main.rs`.

The current checked-in default repro is already heavily compressed:

- text: `il`
- repeat draws: `1`
- font size: `0.35`
- yaw: `-0.00009`
- pitch: `0.0`

Plain `cargo run` in `G:\Programming\Repos\graphics-driver-crash` still reproduces device removal with `DXGI_ERROR_DEVICE_HUNG` / `0x887A0006`.

## What The Repro Tells Us

- This is no longer a workload-size problem. The crash does not require many draws, many glyphs, large text, readback, worker fanout, or PNG output.
- A single draw is sufficient.
- The crash does not require a large transform. A very small nonzero yaw is enough.
- Pitch is not required in the reduced repro.
- Zero transform is a clean success control.
- Single glyph controls were observed to complete cleanly, while at least some mixed two-glyph cases reproduce.

The important interpretation is that the failure now looks much more like a transformed-text correctness bug than a general GPU pressure problem. The repro is small enough that resource exhaustion, command list size, and output-copy paths are poor explanations.

## Teamy-Studio Relevance

The most relevant Teamy-Studio code paths are:

- `G:\Programming\Repos\Teamy-Studio\src\app\windows_d3d12_renderer.rs`
- `G:\Programming\Repos\Teamy-Studio\src\app\windows_panel_shaders.hlsl`
- `G:\Programming\Repos\Teamy-Studio\src\app\render_verification.rs`

The standalone repro was deliberately derived from the transformed text path used by Teamy-Studio, so the fix should be sought in the transformed text pipeline rather than in generic D3D12 setup.

## Likely Fix Surface

The reduced repro suggests we should focus on math or data-contract issues that only appear once the transformed text path is active, even when the transform is almost identity.

Most likely areas to inspect first:

- transformed local-point reconstruction / inverse homography handling
- transformed glyph UV remapping and local bounds handling
- `SlugDilate(...)` and related jacobian / normal usage
- band traversal and coverage evaluation in the transformed branch
- derivative-sensitive logic such as `fwidth(renderCoord)` when transformed text is nearly but not exactly untransformed
- CPU-to-GPU contract mismatches in transformed text vertex packing

The strongest clue is that `yaw = 0` succeeds while a tiny nonzero yaw fails. That points toward a precision-sensitive or branch-sensitive issue in transformed text evaluation, not toward text content size or render target size.

## Practical Guidance For Fixing Teamy-Studio

The safest approach is likely:

1. Use `G:\Programming\Repos\graphics-driver-crash` to keep shrinking and characterizing the failure.
2. In Teamy-Studio, compare the transformed-text branch against the non-transformed text branch and look for assumptions that are only valid away from the near-identity case.
3. Prioritize guards or math changes that make the transformed path numerically stable near zero rotation.
4. Use `G:\Programming\Repos\Teamy-Studio\src\app\render_verification.rs` to add or extend a regression check once a fix candidate exists.

## Current Bottom Line

We now have a small standalone repro in `G:\Programming\Repos\graphics-driver-crash` that demonstrates the Teamy-Studio crash without requiring the full application. That materially lowers the cost of investigating the bug and strongly suggests the eventual Teamy-Studio fix belongs in transformed text math / shader correctness, not in broad renderer infrastructure.
