# Image Upscale

This specification covers Teamy Studio image upscaling for single-image scrapbook, sticker, paper, and art assets.

## CLI Inventory

image[cli.image-command]
The CLI must expose an `image` command group.

image[cli.upscale-command]
The `image` command group must expose an `upscale` subcommand that accepts one input image path and an optional output path.

image[cli.upscale-defaults]
The `image upscale` command must default to style `art`, scale `2`, tile size `256`, batch size `4`, device `cuda`, and output format `auto`.

image[cli.model-selection]
The `image upscale` command must resolve its managed image model from the CLI request shape, including style and optional denoise noise level, instead of hard-coding one checkpoint for every request.

image[cli.art-denoise-2x]
For `style = art`, `image upscale --noise-level <0..3>` must resolve to the matching managed `noise_scale2x` model and run through the normal prepare/runtime path.

image[cli.art-default-low-denoise]
For `style = art`, when `--noise-level` is omitted, `image upscale` must default to the low-denoise managed art model (`noise_level = 0`) so the default preset matches nunif's web UI more closely.

image[cli.art-disable-denoise]
For `style = art`, `image upscale --disable-denoise` must opt out of the default low-denoise preset and resolve the scale-only managed art model instead.

image[cli.art-native-4x]
For `style = art`, when the requested upscale factor is a power of 4, `image upscale` must prefer the native managed `scale4x` or `noise_scale4x` model instead of always repeating 2x passes.

image[cli.photo-derived-2x]
For `style = photo` and `style = art_scan`, `image upscale` must prepare the upstream `scale4x` checkpoint and downscale each 4x tile back to the logical 2x output so the managed `*-2x` model ids remain runnable.

image[cli.output-path-generation]
When `image upscale` is invoked without an output path, Teamy Studio must generate one from the input path and selected output format.

image[cli.output-format-inference]
When `--output-format auto` is used with an explicit output path, Teamy Studio must infer the image output format from the output path extension.

image[cli.output-format-conflict]
When explicit `--output-format` conflicts with a user-supplied output path extension, `image upscale` must fail before model preparation or output writing.

image[cli.alpha-preserved]
The `image upscale` command must preserve alpha for formats that support alpha.

image[cli.cuda-required-by-default]
The `image upscale` command must require CUDA by default and explain `--device cpu` when CUDA is unavailable.

image[cli.auto-prepare-default-model]
When the resolved default art request model is missing, `image upscale` must warn and prepare it before upscaling instead of immediately failing.

image[cli.unsupported-model-selection]
When `image upscale` resolves to a known inventory-only model variant that Teamy cannot run yet, it must fail clearly before inference rather than pretending the request can run.

image[cli.model-command]
The `image` command group must expose a `model` subcommand group.

image[cli.model-prepare]
The `image model` command group must expose a `prepare` subcommand for explicit image model preparation.

image[cli.model-list]
The `image model` command group must expose a `list` subcommand for known and managed image models.

image[cli.model-show]
The `image model` command group must expose a `show` subcommand for known or explicit image model directories.

image[model.cache-layout]
Managed image models must live under `{cache_home}/models/image/...`.

image[runtime.rust-only]
Normal image upscale runtime and model preparation must use Rust and Burn rather than Python.

image[runtime.tiled-inference]
Image upscaling must use tiled inference with nunif-compatible model offset and blend semantics.

image[runtime.nunif-alpha-behavior]
Image upscaling must preserve nunif-compatible alpha behavior for non-opaque and opaque alpha channels.

image[self-test.reference-command]
The `self-test` command group must expose a separate image upscale reference command for Python/nunif comparison during development.