We are working on converting waifu2x to the Burn framework for Rust ML inference. I've already done something similar for Whisper.

source
G:\Programming\Repos\nunif\waifu2x
release download
C:\Users\TeamD\Downloads\nunif-windows

our code
G:\Programming\Repos\Teamy-Studio

sample image
"C:\Users\TeamD\Downloads\annicuration\assets\i adore you.png"

NUNIF_HOME
G:\Programming\Caches\NUNIF_HOME

# Waifu2x Burn Image Pipeline Plan

## Goal

Convert the upstream nunif waifu2x image pipeline for still images into a Rust/Burn implementation in Teamy Studio and prove it through a CLI command:

```powershell
teamy-studio image upscale <image-path> [output-path]
```

The current vertical slice is single-image upscaling for low-resolution scrapbooking assets: stickers, paper graphics, decorative illustration assets, and other art-like images where clean edges and transparent alpha preservation matter.

The broader follow-up goal is to capture the rest of the upstream still-image waifu2x pipeline as faithfully as practical in Rust: denoise-only, denoise-plus-upscale model selection, more upstream model families and domains, native checkpoint-driven scale variants, and the remaining image-only quality/runtime knobs that matter for still assets. Video is explicitly out of scope for this plan.

The final runtime and model preparation behavior must be entirely Rust-based. Python is allowed only as a temporary development/reference harness for comparing layer logic while porting the model.

## Current Status

Done so far:

- Reviewed Teamy Studio CLI, model cache, Whisper Burn conversion, and Tracey/spec conventions.
- Reviewed nunif waifu2x CLI, model families, model downloader, `Waifu2x` runtime, alpha handling, and tiled render entry points.
- Settled MVP scope through design grilling.
- Added `docs/spec/product/image-upscale.md` and registered `teamy-studio-image-upscale` in `.config/tracey/config.styx`.
- Scaffolded the `image` CLI group with `image upscale`, `image model list`, `image model prepare`, and `image model show`.
- Scaffolded `self-test image-upscale-reference` for the future Python/nunif comparison harness.
- Implemented `image upscale` argument defaults and early output path/output format validation, including explicit format/path conflict bailing before model preparation or inference.
- Added focused CLI surface tests for image help and output format conflict behavior.
- Added a Teamy-owned `uv` Python reference project under `python/waifu2x-reference`.
- Configured the reference harness to import local nunif from `G:\Programming\Repos\nunif` and use `G:\Programming\Caches\NUNIF_HOME` for downloaded model/cache state.
- Verified the reference environment imports torch `2.11.0+cu128`, torchvision `0.26.0+cu128`, Pillow `12.2.0`, local `waifu2x.hub`, and sees CUDA on the RTX 4090.
- Downloaded nunif waifu2x pretrained models into `G:\Programming\Caches\NUNIF_HOME` and verified the default art scale model loads as `waifu2x.swin_unet_2x` with scale `2`, offset `16`, blend size `8`, tile size `256`, batch size `4`, and `3,758,304` parameters.
- Added Rust-side `waifu2x_reference` helpers that call the Python reference harness through `uv`, matching the successful Whisper pattern.
- Wired `self-test image-upscale-reference` to run the import check and model report through the Rust helper.
- Added a deterministic 4x4 RGBA reference fixture path and Rust-side parsed JSON report structs for import, model, fixture, and tensor reports.
- Added optional real-image tensor inspection to `self-test image-upscale-reference`, including optional `.npz` tensor dumping for local assets.
- Added a deterministic 64x64 model layer report in the Python reference harness, capturing input, first convolutions, major `swin_unet_2x` stages, final `to_image`, and offset-cropped output summaries for Rust/Burn comparison.
- Wired Rust-side layer report parsing into `self-test image-upscale-reference` and verified the default art scale model produces a 1x3x96x96 offset-cropped output from the 1x3x64x64 deterministic input.
- Added Rust-side image model metadata/cache helpers for `{cache_home}/models/image/waifu2x-art-2x/`, including known-model metadata for nunif `swin_unet/art/scale2x.pth`.
- Wired `image model list`, `image model show`, and `image model prepare` to structured reports under the managed image model directory.
- Extended `image model prepare` into a Rust-only source-artifact preparation path that downloads `waifu2x_pretrained_models_20250502.zip`, extracts `pretrained_models/swin_unet/art/scale2x.pth`, and reports archive/checkpoint paths and sizes under `{cache_home}/models/image/waifu2x-art-2x/source/`.
- Verified the managed prepare/show flow end to end with `cargo run -- image model prepare waifu2x-art-2x --overwrite` and `cargo run -- image model show waifu2x-art-2x`, confirming a `437,581,750` byte source archive and `15,238,093` byte checkpoint in Teamy's cache.
- Added a Rust-side waifu2x checkpoint summary probe on top of `burn-store::pytorch::PytorchReader`, surfaced through `image model show` and `image model prepare`.
- Verified the extracted `scale2x.pth` is a ZIP-format PyTorch checkpoint with top-level `state_dict`, checkpoint name `waifu2x.swin_unet_2x`, `nunif_model = 1`, kwargs `{in_channels = 3, out_channels = 3}`, `154` tensors, and `15,178,368` bytes of tensor payload.
- Confirmed early sample tensors and keys from Rust-side inspection, including `unet.patch.0.weight` with shape `48x3x3x3`, `unet.down1.conv.weight` with shape `192x96x2x2`, and mixed dtypes such as `I64` for `unet.swin1.block.0.attn.relative_position_index`.
- Added the first Burn-side waifu2x load probe to `image model show` and `image model prepare`: a minimal `waifu2x.patch-stem` module that instantiates Burn `Conv2d` layers from checkpoint tensor shapes and loads the earliest `state_dict` weights through `PytorchStore` remapping.
- Verified the patch stem loads cleanly from Rust with filter `^unet\.patch\.(0|2)\.(weight|bias)$`, applying `patch0.weight`, `patch0.bias`, `patch2.weight`, and `patch2.bias` with no missing tensors, no unused tensors, and no load errors.
- Confirmed the second patch convolution is `unet.patch.2.weight` with shape `96x48x3x3`, which gives the first concrete Burn-side module boundary for widening into the rest of the encoder path.
- Widened the same Burn probe through `unet.down1.conv`, using filter `^(unet\.patch\.(0|2)|unet\.down1\.conv)\.(weight|bias)$` and a minimal Burn module containing `patch0`, `patch2`, and `down1_conv`.
- Verified `unet.down1.conv.weight` with shape `192x96x2x2` and `unet.down1.conv.bias` also load cleanly from Rust, giving six applied tensors total with no missing tensors, no unused tensors, and no load errors.
- Extended the Burn probe into the first Swin block using the checkpoint's actual key layout for `unet.swin1.block.0.*`: attention `qkv`/`proj`, MLP `0`/`3`, `relative_position_bias_table`, and `relative_position_index`.
- Verified the first Swin block also loads cleanly from Rust with no missing tensors, no unused tensors, and no load errors, including `attn.qkv.weight` shape `288x96`, `relative_position_bias_table` shape `121x6`, and the `I64` `relative_position_index` buffer shape `1296`.
- Confirmed an important checkpoint-layout detail for the Burn port: this block does not carry block-local `norm1`/`norm2` tensors under `unet.swin1.block.0.*`, and the second MLP linear lives at `mlp.3`, not `mlp.2`.
- Widened the same Burn probe across the whole `swin1` stage by loading both `unet.swin1.block.0.*` and `unet.swin1.block.1.*` into two identical Burn block probes.
- Verified the complete `swin1` stage prefix loads cleanly from Rust with no missing tensors, no unused tensors, and no load errors, and confirmed there are no additional stage-level tensors under the `unet.swin1.*` prefix beyond those two block-local payload sets.
- Confirmed `block.1` mirrors `block.0` exactly for the current import boundary: `attn.qkv`, `attn.proj`, `mlp.0`, `mlp.3`, `relative_position_bias_table`, and `relative_position_index`, with the same observed tensor shapes for `qkv`, bias table, and index buffer.
- Extended the same Burn probe through `unet.down2.conv` and the full `unet.swin2.*` stage, keeping the same Rust-side block representation but allowing the higher channel count to prove or disprove the repeated-stage assumption.
- Verified `unet.down2.conv.weight` with shape `192x192x2x2` also loads cleanly from Rust, and confirmed the whole `swin2` stage prefix loads with no missing tensors, no unused tensors, and no load errors.
- Confirmed the higher-channel Swin stage keeps the same structural pattern as `swin1`: two blocks, no extra stage-level tensors under the `unet.swin2.*` prefix, the same `relative_position_bias_table` and `relative_position_index` shapes, and a widened `attn.qkv.weight` shape of `576x192`.
- Extended the probe one stage further into `unet.swin3.*`, first by loading `block.0` as the cheapest discriminating hop and then by widening to the full six-block stage once that first block matched the existing structure.
- Verified the complete `swin3` stage prefix loads cleanly from Rust with no missing tensors, no unused tensors, and no load errors, and confirmed there are still no extra stage-level tensors under the `unet.swin3.*` prefix beyond the six block-local payload sets.
- Confirmed `swin3` keeps the same per-block tensor layout as the earlier Swin stages while increasing depth instead of width: blocks `0` through `5` all expose `attn.qkv`, `attn.proj`, `mlp.0`, `mlp.3`, `relative_position_bias_table`, and `relative_position_index`, with the same observed `qkv` shape `576x192` and the same relative-position tensor shapes as `swin2`.
- Extended the Burn probe through the remaining checkpoint path after `swin3`: the full `swin4` stage, the full reduced-width `swin5` stage, and the decoder/output projection layers `up1.proj`, `up2.proj`, and `to_image.proj`.
- Verified the entire waifu2x `state_dict` now loads cleanly through the Rust-side Burn probe with no missing tensors, no unused tensors, and no load errors on the `image model show` surface, establishing that the current Burn skeleton can represent the full checkpoint payload shape.
- Confirmed the late-model structure stays regular all the way through the output heads: `swin4` remains a two-block `576x192` stage, `swin5` narrows back to a two-block `288x96` stage, and the remaining non-Swin tensors load as plain linear projections with observed weight shapes `up1 = 384x192`, `up2 = 768x192`, and `to_image = 12x96`.
- Taught `image model prepare` to materialize a real `model.bpk` from that same full-checkpoint mapping instead of stopping at metadata/source preparation, using the validated waifu2x Burn skeleton as the first export-capable model shape.
- Verified `cargo run -- image model prepare waifu2x-art-2x` now writes the Burnpack file and flips the model state from `MetadataOnly` to `Prepared`, with `burnpack_exists = true` and the full checkpoint still loading cleanly into the Burn skeleton before save.
- Wired `image upscale` to auto-prepare the default managed image model source artifacts before reaching the current Burn inference placeholder.
- Wired `image upscale` to consume the prepared Burnpack on the real command path, proving the managed waifu2x model now reloads cleanly from `model.bpk` before inference.
- Added the first Rust-side image preprocessing slice on that same command path: the input image now opens through the Rust runtime, converts to nunif-style normalized CHW RGB data plus optional alpha, and reports whether alpha is effectively blank before the remaining inference stub.
- Replaced the flat load-only skeleton with the first reusable Rust-side `swin_unet_2x` runtime forward path, including staged patch/downsample/upsample wiring, shifted-window Swin attention, MLP blocks, and output projection.
- Verified a focused shape contract in Rust with `cargo test waifu2x_forward_preserves_the_known_2x_shape_contract --lib`, proving the current Burn model wiring maps a `1x3x64x64` input to the expected `1x3x96x96` output.
- Replaced the temporary whole-image helper with nunif-style tiled RGB inference: replicate padding, valid tile-size fallback, batched tile execution, seam blending, output stitching, and final crop back to the requested image extent.
- Verified the tiled helper directly with `cargo test waifu2x_tiled_inference_handles_sizes_the_untiled_path_rejected --lib`, proving a `100x100` input now upscales cleanly to `200x200` even though the old untiled path rejected that size.
- Wired the real `image upscale` CLI path to the tiled Burn runtime and report fields for `tile_size`, `batch_size`, and `runtime_mode = "tiled-rgb-only"`.
- Verified that real tiled CLI path end-to-end with `cargo run -- image upscale target/test-artifacts/image-upscale/tiny-tiled-100.png --device cpu`, producing `tiny-tiled-100.upscaled-2x.png` with reported and observed output size `200x200`.
- Reintroduced nunif-style alpha handling on top of the tiled runtime: preserve input alpha presence through preprocessing, apply alpha-border RGB padding for non-blank alpha, upscale alpha through the same scale model by expanding to RGB and averaging back to one channel, and write RGBA PNG output when alpha is present.
- Verified the alpha path with `cargo test waifu2x_alpha_border_padding_fills_transparent_neighbors_from_opaque_pixels --lib` and `cargo run -- image upscale target/test-artifacts/image-upscale/tiny-alpha-64.png --device cpu`, producing `tiny-alpha-64.upscaled-2x.png` with reported `runtime_mode = "tiled-rgba"`, output size `128x128`, and observed on-disk alpha range `7..255`.
- Added real CUDA-backed image upscale execution instead of the earlier CPU-only advisory path, reusing Burn CUDA device selection and generic model loading patterns already proven in Whisper.
- Verified the CUDA path with `cargo run -- image upscale target/test-artifacts/image-upscale/tiny-alpha-64.png --device cuda --log-filter trace`, confirming `cubecl_cuda` activity and successful tiled RGBA output.
- Added format-aware still-image writing for PNG, JPEG, and WebP, preserving alpha for PNG/WebP and flattening over white for JPEG.
- Added focused self-test and CLI-surface coverage updates so managed model preparation, output format behavior, and image CLI paths match the now-real runtime behavior.
- Added scale-4 support first, then generalized scale handling to powers of two by treating the current managed 2x model as a repeated-pass primitive: 2x = one pass, 4x = two passes, 8x = three passes.
- Verified the generalized power-of-two path with `cargo test image_upscale_ --lib`, `cargo run -- image upscale target/test-artifacts/image-upscale/tiny-alpha-64.png --device cuda --log-filter trace --scale 8`, and a real local `--scale 4` CUDA run on `i adore you.png`.
- Expanded the managed image model inventory beyond the original single art 2x checkpoint so Teamy can describe and prepare upstream `swin_unet` art, photo, and art_scan variants with explicit style, method, native scale, and runtime status metadata.
- Added runnable art denoise+2x support and routed CLI `--style` plus `--noise-level` selection through the shared model inventory instead of hard-coding one checkpoint for every request.
- Generalized the Burn waifu2x runtime to handle both 2x and 4x checkpoint layouts, including native art 4x and native art denoise+4x checkpoints, and added the derived-2x-from-4x runtime path for photo and art_scan.
- Fixed the native 4x tiled-runtime metadata mismatch by giving 4x models the upstream `offset = 32` and `blend_size = 16`, which removed the real `--scale 4` CUDA failure and improved seam behavior.
- Changed the default art CLI route to a denoise-aware low-noise preset (`noise_level = 0`) and added an explicit `--disable-denoise` escape hatch so the scale-only art models remain reachable.
- Updated CLI surface coverage and feature-sensitive terminal replay coverage so `cargo test --test cli_surface`, `cargo test --test terminal_replay_cli`, and `./check-all.ps1` pass after the denoise/default-quality changes.

Current focus:

- Update this plan so it reflects the real current runtime and captures the next quality-oriented slice: explicit quality/runtime knobs, especially TTA and preset-driven defaults, without regressing the now-correct baseline image pipeline.

Remaining work:

- Add denoise-only runtime support for the inventory-only `noise{0,1,2,3}` checkpoints when that path is worth exposing to users.
- Add preset-driven quality/runtime controls so default behavior stays maximum quality while fast-path users can opt into cheaper settings explicitly.
- Add TTA as an optional still-image quality knob, ideally through preset-aware defaults plus an explicit override flag.
- Tighten Python-reference parity coverage so new model families and denoise paths do not drift from upstream semantics.
- Re-run `.\check-all.ps1` after the latest scale generalization and after each major pipeline expansion.
- Build the GUI layer only after the CLI proves the remaining image pipeline slices.

Immediate next step:

- Define the preset and TTA product surface in spec terms first: default `quality` preset, explicit `fast` preset, preset-seeded defaults for optional knobs, and an explicit `--tta` override that can still force the high-quality path when users need it.

## Settled Decisions

- MVP is image-only. No video support.
- MVP accepts exactly one input image path. No globbing, recursive directories, batch folders, or file-list manifests yet.
- Main target assets are stickers, paper graphics, and scrapbook-style art assets.
- Default style is `art`.
- Default scale is `2`.
- Additional powers-of-two upscale factors are allowed today by repeated application of the 2x model. This is a pragmatic runtime capability, not yet a promise that Teamy supports upstream-native scale checkpoints for every family/domain.
- All CLI parameters except the input image path are optional and have defaults.
- Output format is automatic by default.
- If an output path is supplied and `--output-format auto`, infer the output format from that output path extension.
- If no output path is supplied and `--output-format` is explicit, generated output paths must use the matching extension.
- If an explicit `--output-format` disagrees with a user-supplied output path extension, bail.
- Preserve alpha as MVP behavior, following nunif as closely as possible.
- Tiling is part of the MVP.
- Tiling defaults should inherit from nunif. For current nunif waifu2x, default tile size is `256` and default batch size is `4`.
- Still-image denoise support is in scope for the next expansion of this plan.
- Default user-facing behavior should prioritize maximum quality rather than minimum latency.
- Presets should seed the defaults for optional image-quality/runtime flags instead of replacing the explicit flags.
- The default preset should be `quality`.
- A `fast` preset should exist for users who want lower latency and are willing to trade away quality-oriented options.
- TTA should be implemented as an image-only quality knob that presets can enable or disable by default, while still remaining explicitly overridable from the CLI.
- Video filters and video-only options remain out of scope.
- CUDA is the expected runtime path.
- If `--device cpu` is not specified and CUDA is unavailable, bail with a warning/error that explains the `--device cpu` fallback flag.
- CPU support should exist for tiny tests and explicit fallback, but it does not need to be pleasant for large real assets.
- A separate self-test command should host the Python comparison/reference workflow. Do not put Python comparison behind `image upscale` flags.
- Final behavior must be entirely Rust-based. Python is for development verification only.
- If `image upscale` runs and the default image model is not prepared, it should warn and prepare the model automatically instead of bailing.
- An explicit prepare command should also exist.
- Managed image models should live under `{cache_home}/models/image/...`.
- Runtime should not depend on the checked-out nunif repository or the downloaded nunif-windows folder.

## MVP CLI Contract

Intended command shape:

```powershell
teamy-studio image upscale <image-path> [output-path] `
  --preset quality `
  --style art `
  --scale 2 `
  --tile-size 256 `
  --batch-size 4 `
  --device cuda `
  --output-format auto
```

Required argument:

- `<image-path>`: single input image file.

Optional arguments and defaults:

- `[output-path]`: generated from the input path when omitted.
- `--preset quality`: default preset. Presets set the starting values for optional quality/runtime knobs.
- `--style art`: MVP default; future values may include `photo`, `scan`, or `art-scan` once supported.
- `--scale 2`: default. Additional powers-of-two scales currently reuse the same 2x model in repeated passes.
- `--tile-size 256`: inherited from nunif default model behavior.
- `--batch-size 4`: inherited from nunif default model behavior.
- `--device cuda`: expected fast path. `--device cpu` is explicit fallback.
- `--output-format auto`: infer from output path when present, otherwise default generated output should be PNG.

Future quality/runtime knobs to surface through the same command:

- `--tta`: explicit override for test-time augmentation. The default value should come from the active preset.
- other quality/runtime flags should follow the same rule: the preset chooses the default, and the explicit flag wins when supplied.

Output path behavior:

- If no output path is supplied and `--output-format auto`, generate a PNG path beside the input, for example `asset.upscaled-2x.png`.
- If no output path is supplied and `--output-format png`, generate `asset.upscaled-2x.png`.
- If no output path is supplied and a future supported format is requested, generate the matching extension.
- If output path is supplied and `--output-format auto`, infer from the supplied extension.
- If output path is supplied and explicit `--output-format` conflicts with its extension, bail.
- If the requested output format cannot preserve required alpha, bail until an explicit flatten/background option is designed.

MVP exclusions:

- No video.
- No directories.
- No globbing.
- No recursion.
- No GUI.
- No video processing.

## Architecture Direction

### Command And Module Shape

Follow Teamy Studio CLI conventions:

- Add an `image` command group under `src/cli/image/`.
- Each subcommand gets its own directory module.
- Each implementation lives in a discoverable `{}_{}_{}_cli.rs`-style file and is re-exported by `mod.rs`.

Likely CLI modules:

- `src/cli/image/image_cli.rs`
- `src/cli/image/upscale/image_upscale_cli.rs`
- `src/cli/image/model/image_model_cli.rs`
- `src/cli/image/model/prepare/image_model_prepare_cli.rs`
- `src/cli/image/model/list/image_model_list_cli.rs`
- `src/cli/image/model/show/image_model_show_cli.rs`
- `src/cli/self_test/image_upscale_reference/self_test_image_upscale_reference_cli.rs`

### Model Cache Layout

Use Teamy Studio cache state, under a domain-specific image model namespace:

```text
{cache_home}/models/image/waifu2x-art-2x/
```

Likely managed artifact layout:

```text
{cache_home}/models/image/waifu2x-art-2x/
  model.bpk
  model.json or dims.json
  model-metadata.json
```

The exact metadata shape should encode:

- model family
- style/domain
- scale
- architecture name
- source model URL/version
- source checkpoint path inside the archive
- model offset
- blend size
- default tile size
- default batch size
- expected input channels
- expected output channels
- alpha behavior support

### Model Family Strategy

The desired product target is the upstream nunif still-image pipeline, staged so Teamy does not over-promise support before the Rust/Burn port is real.

Short-term implementation strata:

1. Keep the current `swin_unet/art/scale2x` path healthy as the reference-quality baseline.
2. Add denoise-only and denoise+scale model inventory and selection for the same family/domain before broadening to more families.
3. Add more upstream domains and native checkpoint scales once the selection/model-cache surface is stable.

Relevant upstream still-image surfaces to capture over time:

- `swin_unet/art` with `noise{0,1,2,3}`, `noise{0,1,2,3}_scale2x`, and `noise{0,1,2,3}_scale4x`
- `swin_unet/photo` with the same denoise/scale variants
- `swin_unet/art_scan` with the same denoise/scale variants
- lower-quality but faster families such as `upconv_7/art`, `upconv_7/photo`, and `cunet/art` only after the higher-priority `swin_unet` still-image surface is mapped

The implementation should be staged:

1. Keep the current `swin_unet/art` 2x path as the first proven Burn runtime slice.
2. Expand within `swin_unet` first so denoise/noise-level routing and native scale checkpoint support land on the product-target family before lower-priority families are added.
3. Only then decide whether Teamy should also expose faster/lower-quality families such as `upconv_7` or `cunet`.

Important context from nunif:

- `upconv_7/art` and `upconv_7/photo` are fastest but lowest quality.
- `swin_unet/art` is the current default art model family.
- Upstream still-image denoise surfaces are expressed as checkpoint names such as `noise0.pth`, `noise1_scale2x.pth`, and `noise3_scale4x.pth` rather than as a single model with a scalar noise parameter.
- `swin_unet` has stricter tile-size requirements and more complex layers.
- `I2IBaseModel` defines defaults and tiling metadata: scale, offset, optional blend size, default tile size `256`, and default batch size `4`.

Teamy should model the upstream image pipeline in terms of real managed model artifacts and explicit runtime routing, not pretend one checkpoint can natively cover arbitrary denoise/scale combinations that upstream actually represents as separate weights.

Use the Python verifier to avoid guessing at layer semantics while porting.

### Rust Runtime Boundary

Runtime inference must be Rust/Burn.

The runtime path should:

- load a Teamy-managed Burn model artifact
- decode the input image in Rust
- normalize RGB into the expected tensor layout
- preserve and process alpha using nunif-equivalent behavior
- run tiled Burn inference
- optionally run TTA transforms as a batched inference expansion when the resolved quality settings request it
- stitch tiles using the model offset and blend behavior
- encode the output image in Rust

### Quality Preset Strategy

Still-image quality/runtime options should be modeled in two layers:

1. A high-level preset such as `quality` or `fast` chooses the default values for optional knobs.
2. Explicit flags such as `--tta` override the preset-derived defaults for a specific invocation.

Current product direction:

- `quality` should be the default preset.
- `quality` should aim for the highest practical still-image quality Teamy can provide, even if it is slower.
- `fast` should be opt-in and should disable or relax expensive quality-oriented knobs where that makes sense.
- Presets should not hide capability. Users should always be able to override an individual knob explicitly.

For TTA specifically:

- treat TTA as an image-only quality knob, not a video feature
- implement it so geometric transforms can be expanded into one inference batch per tile when memory allows
- keep the explicit CLI override separate from the preset name so future knobs can follow the same pattern

### Python Verification Boundary

Python is allowed only for development verification.

Add a Teamy-owned `uv`-managed reference project or script for comparing against nunif/PyTorch behavior. It should be separate from the normal runtime and from `image upscale`.

Preferred shape:

- A self-test command invokes or coordinates the reference path.
- The self-test can create/use a `uv` environment when necessary.
- The verifier can emit intermediate tensors for deterministic tiny fixtures.
- The normal user-facing `image upscale` command never shells out to Python.

Reference comparison should validate:

- image load normalization: RGB/RGBA to NCHW float in `0..1`
- alpha extraction and blank-alpha detection
- alpha border padding behavior
- first convolution output
- activation output
- each major model block output
- final RGB before clamp
- final alpha path
- final PNG dimensions and pixel content for tiny fixtures

Use CPU reference comparisons for deterministic layer checks where practical. CUDA comparison can be a secondary smoke path.

### Alpha Handling

Preserve nunif behavior as MVP acceptance criteria.

Observed nunif behavior:

- The image loader preserves alpha when requested.
- If alpha exists and is not fully opaque, nunif pads RGB with `AlphaBorderPadding` before model inference.
- For scale methods, alpha is upscaled through the scale model when the model is available.
- If no scale model is available for alpha, nunif falls back to bilinear alpha upscaling.
- If alpha is fully opaque, nunif uses nearest-neighbor alpha scaling.

Sticker transparency correctness is not a follow-up. The MVP should include a small transparent sticker-like fixture that catches edge fringe/matte mistakes.

### Tiling

Tiling is part of MVP.

The implementation should port the nunif tiled render behavior closely enough that tile output matches reference fixtures. Do not stitch model outputs naively. Respect:

- model scale
- model offset
- blend size when present
- validated tile size
- default tile size
- default batch size

Initial CLI defaults should use nunif defaults: tile size `256`, batch size `4`.

Future work can add VRAM-aware automatic tile sizing.

## Tracey Specification Strategy

This is a new user-facing behavior area, so add a dedicated product spec by default.

Recommended new spec:

```text
docs/spec/product/image-upscale.md
```

Recommended Tracey registration:

- Add `teamy-studio-image-upscale` to `.config/tracey/config.styx`.
- Include `docs/spec/product/image-upscale.md`.
- Include Rust implementation under `src/**/*.rs` and tests under `tests/**/*.rs`.

Suggested initial requirements:

- `image[cli.image-command]`: CLI exposes `image` command group.
- `image[cli.upscale-command]`: `image upscale` accepts one image input and optional output path.
- `image[cli.upscale-defaults]`: defaults are style `art`, scale `2`, tile size `256`, batch size `4`, device `cuda`, output format `auto`.
- `image[cli.output-path-generation]`: missing output path is generated from input path and selected output format.
- `image[cli.output-format-inference]`: output format is inferred from user-supplied output path when `auto`.
- `image[cli.output-format-conflict]`: explicit output format conflicting with supplied output extension fails.
- `image[cli.alpha-preserved]`: alpha is preserved for formats that support alpha.
- `image[cli.cuda-required-by-default]`: CUDA is required unless `--device cpu` is explicit.
- `image[cli.auto-prepare-default-model]`: missing default model is prepared with a warning before upscaling.
- `image[cli.model-prepare]`: explicit image model prepare command exists.
- `image[model.cache-layout]`: image models are managed under `{cache_home}/models/image/...`.
- `image[runtime.rust-only]`: normal upscale runtime uses Rust/Burn, not Python.
- `image[runtime.tiled-inference]`: upscale uses tiled inference with model offset/blend semantics.
- `image[runtime.nunif-alpha-behavior]`: non-opaque alpha uses nunif-compatible border handling and alpha upscaling.
- `image[self-test.reference-command]`: separate self-test command validates Rust behavior against the Python/nunif reference harness.

Baseline Tracey workflow:

```powershell
tracey query status
tracey query uncovered
tracey query unmapped
tracey query unmapped --path src/cli/image
tracey query validate --deny warnings
```

After implementation coverage is under control:

```powershell
tracey query untested
```

Teamy Studio validation should use:

```powershell
.\check-all.ps1
```

Do not use alternate `CARGO_TARGET_DIR` values.

## Phased Task Breakdown

### Phase 1: Spec And CLI Skeleton

Objective: Establish the observable behavior contract and command surface before model work.

Tasks:

- Add `docs/spec/product/image-upscale.md` with the initial requirements above.
- Register a new Tracey spec in `.config/tracey/config.styx`.
- Add `src/cli/image/` modules following repo conventions.
- Add `ImageArgs`, `ImageCommand`, `ImageUpscaleArgs`, and model subcommand skeletons.
- Add `ImageDevice` and `ImageOutputFormat` types with parse-safe defaults.
- Add command dispatch from `src/cli/mod.rs`.
- Add CLI surface tests for help/roundtrip if existing test patterns make that straightforward.

Definition of done:

- `teamy-studio image --help`, `teamy-studio image upscale --help`, and `teamy-studio image model --help` parse.
- Tracey validates the new requirements as covered by skeleton implementation references where applicable.
- `.\check-all.ps1` passes or any unrelated pre-existing failure is documented.

### Phase 2: Output Path, Format, And Image IO

Objective: Implement deterministic command-level file behavior before neural inference.

Tasks:

- Add output-path generation for omitted output path.
- Implement output format inference from output extension.
- Implement explicit output-format conflict detection and bail behavior.
- Decode input images in Rust.
- Encode PNG output in Rust.
- Preserve alpha through a placeholder non-model path only for early scaffolding if needed, but do not call MVP complete until nunif alpha behavior lands.
- Add focused tests for output path generation and conflict bailing.

Definition of done:

- Path and format behavior works without needing a prepared model.
- Conflict behavior is tested.
- Generated output names match the selected format.

### Phase 3: Image Model Metadata And Cache Management

Objective: Create the Rust-managed image model registry/cache path.

Tasks:

- Add image model metadata types separate from Whisper metadata.
- Add `{cache_home}/models/image/...` helpers.
- Add known default model entry for the MVP art 2x model.
- Implement `image model list`.
- Implement `image model show`.
- Implement `image model prepare` skeleton and idempotence behavior.
- Implement `image upscale` auto-prepare warning path when the default model is missing.

Definition of done:

- The CLI can list/show known image model locations.
- Missing model paths are reported clearly.
- `image upscale` attempts prepare instead of immediately bailing when the default model is absent.

### Phase 4: Rust Model Artifact Preparation

Objective: Produce Teamy-managed Burn model artifacts without requiring Python in final behavior.

Tasks:

- Decide the first concrete checkpoint/architecture to port based on implementation risk and quality target.
- Download or ingest the official nunif model archive in Rust.
- Extract the selected checkpoint/archive contents in Rust.
- Convert weights into `model.bpk` or another Teamy-supported Burn artifact layout in Rust.
- Store metadata needed for runtime tiling and alpha behavior.
- Make prepare idempotent and overwrite-aware.
- Add clear errors for unsupported source archive or metadata mismatch.

Definition of done:

- `teamy-studio image model prepare` creates a complete managed image model directory using Rust code.
- The prepared directory can be inspected without Python.
- Re-running prepare is idempotent unless overwrite is requested.

### Phase 5: Python Reference Harness And Self-Test Command

Objective: Build the development-only comparison harness needed to port model logic confidently.

Tasks:

- Add a Teamy-owned `uv` reference project or script for waifu2x/nunif comparisons.
- Add `self-test image-upscale-reference` command.
- Create deterministic tiny image fixtures, including transparent sticker-like alpha edges.
- Make Python emit intermediate tensors for selected model/layer checkpoints.
- Make Rust load those tensors or ask Python to write comparison artifacts.
- Compare Rust tensor outputs against Python reference outputs with documented tolerances.

Definition of done:

- Self-test can create/use a `uv` environment if needed.
- Self-test is separate from `image upscale`.
- A tiny transparent fixture catches alpha/fringe regressions.
- Layer comparison failures identify the first mismatching stage.

### Phase 6: Burn Model Implementation

Objective: Port the selected waifu2x model architecture to Burn.

Tasks:

- Implement the selected model architecture generic over Burn backend where practical.
- Load converted weights from the managed artifact.
- Match tensor layout and shape semantics against nunif/PyTorch.
- Implement clamp/output behavior.
- Add CPU-compatible execution for self-tests and explicit `--device cpu`.
- Add CUDA execution for default runtime path.

Definition of done:

- Rust model outputs match the Python reference on tiny deterministic fixtures within agreed tolerances.
- The model can run on CUDA for normal upscale.
- Explicit CPU mode works for small fixture inputs.

### Phase 7: Nunif-Compatible Tiling And Alpha

Objective: Preserve nunif quality behavior for real sticker/paper assets.

Tasks:

- Port tiled render semantics: tile sizing, valid tile size adjustment, batching, offset crop, blend behavior, and stitching.
- Port or reproduce `AlphaBorderPadding` behavior.
- Implement blank-alpha detection.
- Implement non-blank alpha path with model-based alpha upscale when available and bilinear fallback when not.
- Implement fully opaque alpha nearest-neighbor behavior.
- Add fixture comparisons for transparent and opaque alpha cases.

Definition of done:

- Tiled Rust output matches non-tiled/reference behavior for small and medium fixtures.
- Transparent sticker-like fixture has no obvious edge matte/fringe regression.
- Alpha behavior is covered by self-test and/or Rust tests.

### Phase 8: Upstream Still-Image Model Inventory And Selection

Objective: Move beyond the single hard-coded art 2x model and represent the upstream still-image pipeline explicitly.

Tasks:

- Extend the managed image model inventory to include upstream still-image families, domains, denoise levels, and native scale checkpoints that Teamy intends to support.
- Decide the Teamy CLI surface for model selection: explicit model ids, higher-level `--style`/`--noise-level` routing, or a staged hybrid.
- Update managed metadata so each prepared artifact records family/domain/denoise-level/native-scale information.
- Make `image model list` and `image model show` useful for multiple prepared still-image models, not just the current default.
- Update the Python reference harness so it can report against non-default models and denoise variants.

Definition of done:

- Teamy can describe and prepare more than one upstream still-image model variant.
- The selection/routing surface is explicit enough that future denoise support does not require another metadata redesign.

### Phase 9: Denoise And Denoise+Scale Runtime

Objective: Capture the denoise part of the upstream still-image pipeline in Rust/Burn.

Tasks:

- Port the first denoise-only model path for the chosen high-priority family/domain.
- Port the first native denoise+scale checkpoint path instead of relying only on repeated 2x passes.
- Add a Teamy CLI surface for denoise selection that maps cleanly onto upstream checkpoint reality.
- Extend parity/self-test coverage for denoise-only and denoise+scale transparent/opaque fixtures.
- Verify alpha behavior stays correct on denoise paths as well as upscale paths.

Definition of done:

- Teamy can run at least one denoise-only and one denoise+scale still-image path in Rust/Burn.
- The denoise surface is covered by focused reference comparisons and CLI tests.

### Phase 10: End-To-End `image upscale`

Objective: Make the proof command usable on real single-image scrapbook assets.

Tasks:

- Wire model resolution, auto-prepare, image decode, tiling, Burn inference, alpha handling, and still-image output together.
- Implement CUDA availability check and default bail behavior.
- Implement explicit `--device cpu` fallback.
- Add preset resolution for still-image runtime defaults, with `quality` as the default preset and `fast` as an explicit opt-in.
- Add explicit CLI overrides for quality/runtime knobs such as TTA so presets seed defaults rather than hiding functionality.
- Implement TTA as an optional image-only quality pass, with batching across transformed variants when memory allows.
- Ensure progress and diagnostics go through tracing/stderr, not mixed into generated image output.
- Run against at least one local real asset and inspect output manually.

Definition of done:

- `teamy-studio image upscale input.png` writes a valid generated output path for the chosen format and scale.
- `teamy-studio image upscale input.png output.png` writes the requested path.
- Missing prepared model warns, prepares, then continues.
- CUDA missing without `--device cpu` bails with a helpful message.
- Explicit `--device cpu` works on a small input.
- Preset-derived defaults and explicit per-flag overrides resolve predictably.
- TTA can be enabled intentionally and improves quality on representative still-image fixtures without breaking alpha or tiling semantics.
- The exposed still-image options map to real upstream-capable model paths rather than placeholders.

### Phase 11: Tests, Tracey Coverage, And Hardening

Objective: Make the feature safe to keep evolving.

Tasks:

- Add parser/roundtrip tests for the new CLI args.
- Add output path/format tests.
- Add model metadata/cache tests.
- Add self-test fixture coverage.
- Add Tracey implementation references throughout touched code.
- Run Tracey baseline commands and address new unmapped/uncovered work.
- Run `.\check-all.ps1`.

Definition of done:

- New behavior is covered in Tracey.
- New CLI and path semantics have tests.
- `.\check-all.ps1` passes or unrelated pre-existing failures are documented.

### Phase 12: GUI Layer

Objective: Add GUI only after the CLI proves real functionality.

Tasks:

- Design an image-upscale GUI surface for selecting input, output, style, scale, tile size, batch size, and device.
- Reuse the same Rust service path as CLI.
- Show model preparation state and friendly prepare progress.
- Show output path and completion status.
- Preserve the same format conflict and alpha rules.

Definition of done:

- GUI calls the same core Rust upscaling pipeline as CLI.
- GUI can upscale a single image using the prepared/default model.
- GUI exposes no behavior that contradicts CLI/spec rules.

## Recommended Implementation Order

1. Add spec and Tracey registration.
2. Scaffold `image` CLI and self-test command surfaces.
3. Implement output path and format resolution.
4. Add image model metadata/cache helpers.
5. Build explicit and auto-prepare control flow with placeholder prepare internals.
6. Build Python `uv` reference self-test harness.
7. Port the first model architecture to Burn and compare layers.
8. Implement Rust model artifact preparation.
9. Port nunif-compatible tiling and alpha behavior.
10. Expand the managed model inventory to cover the upstream still-image variants Teamy actually wants to support.
11. Add denoise and denoise+scale runtime paths.
12. Wire the broader still-image `image upscale` surface end to end.
13. Add preset-driven quality/runtime controls, including TTA.
14. Harden tests, Tracey coverage, and `./check-all.ps1`.
15. Build GUI on top of the proven service path.

## Acceptance Criteria For MVP

- A single command can process one still image asset through the supported upstream-derived image pipeline: `teamy-studio image upscale <image-path> [output-path]`.
- Style defaults to `art` and scale defaults to `2`.
- Output path and output format follow the settled automatic rules.
- Explicit output format conflict with a user-supplied output extension fails.
- Alpha is preserved using nunif-compatible behavior.
- Tiled inference is used with nunif default tile size `256` and batch size `4` unless overridden.
- CUDA is the default path; missing CUDA without `--device cpu` produces a clear bail message.
- Missing default image model is prepared automatically with a warning before upscaling.
- Explicit image model prepare/list/show commands exist.
- Default behavior favors maximum practical still-image quality rather than minimum latency.
- Presets set defaults for optional quality/runtime knobs, and explicit flags can override those defaults.
- Normal runtime and prepare behavior do not use Python.
- Python reference comparison is available only through a separate self-test command.
- Tracey requirements are present and mapped for the new behavior.
- The plan explicitly separates still-image pipeline work from out-of-scope video work.

## Open Decisions

- Which concrete model architecture/checkpoint should be the first Burn port: a simpler `upconv_7/art` proof or the product-target `swin_unet/art` path first?
- What exact Rust-only checkpoint conversion route is best for nunif `.pth` files?
- What tolerance thresholds should be used for layer-level reference comparisons?
- Which Teamy CLI surface is best for the broader still-image pipeline: explicit model ids, `--style` plus `--noise-level`, or a layered hybrid?
- When Teamy exposes denoise levels, should it mirror upstream `-1/0/1/2/3` semantics directly or rename the no-denoise case for CLI clarity?
- Which exact preset-controlled knobs should ship in the first `fast`/`quality` slice besides TTA?
- Should `quality` enable TTA immediately by default once it exists, or should the first rollout keep `quality` as the semantic default while leaving `--tta` opt-in until more fixture comparisons are gathered?
- Should future batch support use shell glob expansion, explicit Teamy globbing, directory traversal, manifest files, or all of these?

## Out Of MVP

- Video processing.
- Recursive directories.
- Globbing and batch manifests.
- Video-only filters and video noise/grain surfaces.
- Photo/scan/art-scan styles unless needed to unblock model conversion.
- WebP/JPEG output.
- Alpha flattening/background controls.
- VRAM-aware automatic tile sizing.
- GUI layer.

## Resume Notes

When resuming this plan:

1. Read `AGENTS.md` first.
2. Use `.\check-all.ps1` for validation, not ad hoc `cargo check`.
3. Do not use alternate `CARGO_TARGET_DIR` values.
4. Start with Phase 1 unless newer work has updated this status section.
5. Keep this file current after every meaningful implementation session.
6. Keep Tracey spec, implementation references, and tests moving together.