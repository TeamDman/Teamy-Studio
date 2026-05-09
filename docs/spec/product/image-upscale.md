# Image Upscale

This specification covers Teamy Studio image upscaling for single-image scrapbook, sticker, paper, and art assets.

## CLI Inventory

image[cli.image-command]
The CLI must expose an `image` command group.

image[cli.upscale-command]
The `image` command group must expose an `upscale` subcommand that accepts one input image path and an optional output path.

image[cli.upscale-defaults]
The `image upscale` command must default to style `art`, scale `2`, tile size `256`, batch size `4`, device `cuda`, and output format `auto`.

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
When the default image model is missing, `image upscale` must warn and prepare it before upscaling instead of immediately failing.

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