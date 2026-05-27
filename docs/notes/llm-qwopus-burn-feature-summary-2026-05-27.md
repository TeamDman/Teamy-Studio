# Qwopus Burn LLM Feature Summary 2026-05-27

This note is the current high-level record for Teamy Studio's local Burn-based LLM inference feature.

It is intended to answer four questions:

1. What feature was built?
2. How does it work today?
3. What decisions and debugging steps mattered during development?
4. What does someone need to know before extending or operating it?

This note complements, but does not replace, the more granular notes:

- [docs/notes/qwopus-qwen35-burn-handoff-2026-05-23.md](./qwopus-qwen35-burn-handoff-2026-05-23.md)
- [docs/notes/llm-inference-reference-repos-2026-05-27.md](./llm-inference-reference-repos-2026-05-27.md)
- [docs/notes/llm-inference-optimization-pass-2026-05-27.md](./llm-inference-optimization-pass-2026-05-27.md)

## Why This Exists

The feature began from a request to make the Jackrong Qwopus Qwen3.5 coder model runnable in pure Rust inside Teamy Studio, with the same general pattern previously used for Whisper and Waifu2x/RealESRGAN:

- inspect the upstream Python flow closely;
- reimplement the tensor path in Rust;
- compare against reference behavior;
- make the end-user experience not depend on Python at runtime.

The specific model family in scope is:

- source model: `Jackrong/Qwopus3.5-9B-Coder`
- quantized artifact source: `Jackrong/Qwopus3.5-9B-Coder-GGUF`
- Teamy managed model name: `qwopus-3.5-9b-coder-q4-k-m`

The work was explicitly redirected away from `candelabra` and onto Burn. That decision is now reflected in the codebase and should be treated as settled for this feature line.

## What Was Built

Teamy Studio now has a local LLM stack that can:

- manage a known Qwopus model entry;
- prepare a Teamy-managed model directory;
- export a Burn-native lazy-load text bundle from the upstream Python model;
- inspect and report model metadata, tokenizer config, and Burn support status;
- run local prompt inference through a pure Rust Burn runtime;
- compare Rust outputs against a Python reference path;
- run incremental decode diagnostics;
- enforce generation timeout limits from the CLI;
- default to CUDA when available;
- surface actionable missing-model recovery instructions.

The current CLI surface is organized under its own subcommand modules:

- [crates/teamy_studio_cli/src/cli/llm/mod.rs](../../crates/teamy_studio_cli/src/cli/llm/mod.rs)
- [crates/teamy_studio_cli/src/cli/llm/model/llm_model_cli.rs](../../crates/teamy_studio_cli/src/cli/llm/model/llm_model_cli.rs)
- [crates/teamy_studio_cli/src/cli/llm/prompt/llm_prompt_cli.rs](../../crates/teamy_studio_cli/src/cli/llm/prompt/llm_prompt_cli.rs)

The runtime implementation lives primarily in:

- [crates/teamy_studio_llm_stack/src/burn_text.rs](../../crates/teamy_studio_llm_stack/src/burn_text.rs)
- [crates/teamy_studio_llm_stack/src/runtime.rs](../../crates/teamy_studio_llm_stack/src/runtime.rs)
- [crates/teamy_studio_llm_stack/src/model.rs](../../crates/teamy_studio_llm_stack/src/model.rs)
- [crates/teamy_studio_llm_stack/src/source_config.rs](../../crates/teamy_studio_llm_stack/src/source_config.rs)
- [crates/teamy_studio_llm_stack/src/reference.rs](../../crates/teamy_studio_llm_stack/src/reference.rs)

The Python-side export and reference harness lives in:

- [python/llm-reference/teamy_llm_reference/__main__.py](../../python/llm-reference/teamy_llm_reference/__main__.py)

## Model Facts That Matter

This is not a 2B model. It is a 9B model.

The Teamy-managed metadata and Hugging Face config consistently identify it as a Qwen3.5-derived 9B text model with a hybrid decoder stack. The important architecture facts are:

- hidden size: `4096`
- intermediate size: `12288`
- hidden layers: `32`
- attention heads: `16`
- key/value heads: `4`
- head dim: `256`
- hybrid layer pattern: mostly `linear_attention`, with `full_attention` every 4th layer
- full-attention interval: `4`
- additional linear-attention head layout:
  - `linear_num_key_heads = 16`
  - `linear_num_value_heads = 32`
  - `linear_key_head_dim = 128`
  - `linear_value_head_dim = 128`

Those facts are no longer meant to be treated as hand-entered constants. The runtime/export path was tightened to derive relevant architecture fields from model JSON and manifest data instead of silently guessing.

## Core Design Decisions

### 1. Burn, not candelabra

This feature line is Burn-first. Earlier exploration around `candelabra` was abandoned. The current runtime and support reporting are Burn-oriented.

### 2. Pure Rust runtime, Python only for preparation and reference

The end-user runtime path is Rust-only. Python is used for:

- exporting the Burn text bundle;
- generating reference prompt/layer reports during debugging;
- inspecting upstream behavior while implementing parity.

### 3. Preservation-first implementation

The work followed a preservation-first style rather than a broad redesign:

- add new LLM modules and CLI surfaces;
- preserve existing repo patterns;
- avoid speculative cleanup while landing the runtime;
- keep debugging helpers when they paid for themselves.

### 4. No `serde` in the LLM stack

The LLM stack was moved to `facet` and `facet-json`. `serde` and `serde_json` were explicitly removed from this area. That matters because the feature requirement was to derive model behavior from JSON while using Teamy's preferred parsing stack.

### 5. Model files are the source of truth

The feature should not hardcode values that are available in model JSON or export manifests. During development, several assumptions were tightened into explicit derived values or validation checks, including:

- `rope_theta`
- `hidden_act`
- layer topology and counts
- tensor shapes and expected dimensions
- tokenizer-config-derived metadata presence

The one important remaining exception is prompt rendering. The runtime still uses a hardcoded Qwen-style single-turn prompt renderer instead of applying the tokenizer `chat_template` directly.

## Development Timeline

The major implementation checkpoints were:

1. `21fca19` `Add Burn-based Qwen3.5 LLM runtime`
2. `c603b18` `Cache Burn text weights across generation`
3. `dd6ba46` `Capture Burn decode state for hybrid generation`
4. `69ee24e` `Speed up Burn LLM generation on CUDA`
5. `8ea5c19` `Document LLM inference reference repos`
6. `13d244f` `Reduce LLM decode scratch allocations`
7. `524844f` `Store compact grouped KV decode cache`
8. `e0635a2` `Record LLM inference optimization pass`
9. `d91a923` `Improve LLM missing model recovery`

Those commits represent three phases:

- bring-up and correctness work;
- performance and decode-architecture work;
- operational polish and recovery messaging.

## Bring-Up and Correctness Work

The first pass landed the basic feature skeleton:

- a managed model entry for the Jackrong Qwopus model;
- Teamy model preparation and inspection flows;
- a Burn text export lane from Python;
- a Rust runtime that could consume the exported bundle;
- CLI support for prompt execution and self-test/reference work.

Correctness work then focused on:

- inspecting the Hugging Face `config.json` contract;
- mapping the hybrid Qwen3.5 text stack into Rust;
- comparing Rust outputs against Python reference behavior;
- identifying where incremental decode diverged from the full path.

One of the most important debugging findings was that divergence first appeared at the first `full_attention` layer, not in the early `linear_attention` layers. That narrowed the search materially.

Another important correction was the handling of full-attention `q_proj`. Upstream Qwen3.5 stores query and gate values interleaved per head. Treating that tensor as a flat `all q, then all gate` layout was wrong. Fixing that moved the Rust path into alignment with the Python reference for the first generated token.

The smoke expectation in [crates/teamy_studio_llm_stack/src/runtime.rs](../../crates/teamy_studio_llm_stack/src/runtime.rs) now reflects that current first token:

- prompt: `Hello again`
- first token id: `248068`
- decoded text: `<think>`

## Burn Export and Model Bundle Layout

The runtime does not execute the GGUF directly. Instead, Teamy prepares or consumes a model directory that includes:

- tokenizer files;
- Hugging Face configuration files;
- optional GGUF artifact;
- a Burn text export directory containing:
  - `burn-text-manifest.json`
  - packed tensor payload, typically `tensors.bin`

Relevant code:

- [crates/teamy_studio_llm_stack/src/model.rs](../../crates/teamy_studio_llm_stack/src/model.rs)
- [crates/teamy_studio_llm_stack/src/burn_text.rs](../../crates/teamy_studio_llm_stack/src/burn_text.rs)
- [python/llm-reference/teamy_llm_reference/__main__.py](../../python/llm-reference/teamy_llm_reference/__main__.py)

Operationally, this means:

- code merges do not include model artifacts;
- a working runtime depends on a prepared local model directory;
- repo-local benchmark bundles are ephemeral and are not part of source control by default;
- the prepared bundle can be much larger than the source quantized GGUF because the Burn export is dense floating-point data.

## Why VRAM Usage Looked So Bad

The model is 9B, and the Burn export path uses dense floating-point tensors rather than executing the original quantized GGUF representation directly. During development, the converted Burn payload was roughly 16.7 GiB on disk.

That means high VRAM use is expected once you include:

- model tensors
- decode caches
- intermediate activations
- temporary buffers
- backend allocator overhead

The right interpretation was not "the model is secretly tiny and something is broken." The correct interpretation was "the current Burn artifact path is dense and expensive."

## Performance Problems and What Fixed Them

Early multi-token generation performance was unacceptable. At different stages, long runs took multiple minutes for very few tokens. The main reasons were:

- replaying too much whole-sequence work during generation;
- reloading or re-decoding weights too often;
- host-device synchronization churn;
- CPU-side processing in places that belonged on device;
- excessive scratch allocation;
- oversized decode cache layout.

The improvements that mattered most were:

### Weight caching

The runtime stopped repeatedly reloading or re-decoding the same tensors across generation.

### Hybrid prefill + cached decode handoff

The runtime now supports generation modes and defaults to a hybrid strategy:

- prefill the prompt in the full path;
- capture per-layer decode state during that prefill;
- hand off to incremental decode without replaying the prompt token by token.

This is selected internally via:

- `TEAMY_STUDIO_LLM_BURN_GENERATION_MODE`

Relevant implementation:

- [crates/teamy_studio_llm_stack/src/burn_text.rs](../../crates/teamy_studio_llm_stack/src/burn_text.rs)

### CUDA `f16` backend

The Burn CUDA backend was moved from `Cuda<f32, i32>` to `Cuda<f16, i32>`.

### Direct device-bound tensor loading

Tensor loading was changed so that supported dtypes are decoded directly into Burn `TensorData` instead of always expanding through host `f32` first.

### On-device MLP

The MLP path was moved back onto the device instead of bouncing through CPU-side scalar/vector processing.

### Reduced scratch allocation

Single-token full-attention and recurrent linear-attention paths were tightened to reuse buffers rather than allocating unnecessary temporaries.

### Compact grouped KV decode cache

The full-attention decode state now stores grouped K/V data in compact form instead of eagerly expanding it to every query head. This was informed by local reference-repo study and is documented in:

- [docs/notes/llm-inference-reference-repos-2026-05-27.md](./llm-inference-reference-repos-2026-05-27.md)
- [docs/notes/llm-inference-optimization-pass-2026-05-27.md](./llm-inference-optimization-pass-2026-05-27.md)

## Benchmark History

The most important stable benchmark result achieved during development was:

```powershell
.\target\release\teamy-studio.exe llm prompt "Count from 1 to 200, comma separated." --model-dir python\llm-reference\bench-model --max-new-tokens 100 --generation-timeout 1m
```

Observed during the successful optimization pass:

- exit code `0`
- actual generated tokens: `100`
- wall-clock: about `31.48s`

That was the checkpoint proving the Burn path could generate 100 requested tokens in under 1 minute.

It is important to understand what that benchmark depended on:

- a release build;
- CUDA backend availability;
- a prepared repo-local Burn benchmark bundle;
- the optimized hybrid/decode-cache path;
- a prompt that would not terminate early due to EOS.

It is also important to understand what it did not prove:

- it did not mean every prompt in every checkout would succeed immediately;
- it did not mean the default app cache was populated;
- it did not remove the need for model preparation on a new machine or a cleaned cache.

## Current CLI Surface

### Model management

Useful commands:

```powershell
cargo run -- llm model list
cargo run -- llm model show
cargo run -- llm model prepare --burn-text-only --with-burn-text
```

Relevant file:

- [crates/teamy_studio_cli/src/cli/llm/model/llm_model_cli.rs](../../crates/teamy_studio_cli/src/cli/llm/model/llm_model_cli.rs)

Important options:

- `--with-burn-text`
- `--burn-text-dtype`
- `--burn-text-only`
- `--overwrite`

### Prompt execution

Useful command shape:

```powershell
cargo run -- llm prompt "hello there" --max-new-tokens 10 --generation-timeout 3m
```

Or with an explicit prepared model directory:

```powershell
cargo run -- llm prompt "hello there" --model-dir path\to\prepared-model-dir --max-new-tokens 10 --generation-timeout 3m
```

Important options:

- `--model`
- `--model-dir`
- `--system-prompt`
- `--max-new-tokens`
- `--generation-timeout`
- `--compare-python`
- `--compare-python-layers`
- `--compare-incremental`
- `--compare-incremental-hidden`
- `--compare-incremental-layers`

Relevant file:

- [crates/teamy_studio_cli/src/cli/llm/prompt/llm_prompt_cli.rs](../../crates/teamy_studio_cli/src/cli/llm/prompt/llm_prompt_cli.rs)

## Timeout Behavior

Generation timeout support was added because long-running or pathological decode loops needed a controlled exit path, especially when profiling.

Key facts:

- CLI accepts human-readable durations such as `90s`, `5m`, or `1h`;
- timeout is enforced inside the generation loop;
- timeout checks happen at token boundaries, not mid-token;
- timeout errors are treated as real generation failures and should not cause a fallback to CPU.

This means a nominal `5m` timeout can surface later than exactly 5 minutes if the token currently in flight takes a long time to finish.

## Current Operational Failure Mode: Missing Prepared Model

One common operator-facing failure is:

- running `llm prompt` with no explicit `--model-dir`;
- expecting a previously prepared model to exist in the default managed cache;
- discovering that the current checkout or app cache does not actually have the prepared bundle.

That is why the current error message now points the user toward:

- `teamy-studio llm model prepare --burn-text-only --with-burn-text`
- or `--model-dir <prepared-model-dir>`

The most likely reasons this happens are:

- the prepared bundle was created in a different worktree or cache location;
- the local app cache was cleaned;
- only source code was merged, not the local model artifacts;
- the earlier benchmark used a repo-local benchmark bundle rather than the default app cache.

Relevant implementation:

- [crates/teamy_studio_llm_stack/src/model.rs](../../crates/teamy_studio_llm_stack/src/model.rs)

## Reference Repos Used During Optimization

The optimization work consulted several local reference repos. The most immediately useful pattern came from `nano-vllm` and related grouped-KV ideas.

See:

- [docs/notes/llm-inference-reference-repos-2026-05-27.md](./llm-inference-reference-repos-2026-05-27.md)

These repos were used as design references only. The Teamy implementation stayed Burn-native and vendored its own logic rather than adding new inference dependencies.

## Remaining Limitations

The current feature is usable, but it is not done in the broader architectural sense.

Known limitations include:

- prompt rendering still uses `render_qwen_single_turn_prompt(...)` instead of directly applying tokenizer `chat_template`;
- the runtime is specific to the captured Qwen3.5 hybrid text architecture rather than a broad generic LLM runtime;
- model preparation is still a required local artifact step and is not automatic;
- dense Burn export remains expensive in disk usage and VRAM compared with directly running a quantized format;
- correctness and performance assumptions should still be validated against the Python reference path when making deep runtime changes.

Relevant prompt-rendering file:

- [crates/teamy_studio_llm_stack/src/runtime.rs](../../crates/teamy_studio_llm_stack/src/runtime.rs)

## Recommended Workflow For Future Changes

When changing this feature again:

1. Inspect current model report output before changing runtime assumptions.
2. Treat model JSON and export manifest values as authoritative.
3. Validate with:
   - `cargo clippy -p teamy_studio_llm_stack --features extended_observability,tracing_subscriber_tracy -- -D warnings`
   - `cargo test -p teamy_studio_llm_stack --features extended_observability,tracing_subscriber_tracy`
4. If touching decode logic, compare Rust against Python reference on a short deterministic prompt first.
5. If touching performance-sensitive paths, benchmark release builds only.
6. Distinguish clearly between:
   - managed app-cache model directories;
   - repo-local temporary benchmark bundles;
   - source-controlled code.

## Minimal Orientation Checklist

If someone is resuming this feature cold, they should read these in order:

1. This note.
2. [docs/notes/qwopus-qwen35-burn-handoff-2026-05-23.md](./qwopus-qwen35-burn-handoff-2026-05-23.md)
3. [docs/notes/llm-inference-optimization-pass-2026-05-27.md](./llm-inference-optimization-pass-2026-05-27.md)
4. [crates/teamy_studio_llm_stack/src/burn_text.rs](../../crates/teamy_studio_llm_stack/src/burn_text.rs)
5. [python/llm-reference/teamy_llm_reference/__main__.py](../../python/llm-reference/teamy_llm_reference/__main__.py)

## External Origins

The feature request was originally prompted by these model references:

- X post: `https://x.com/KyleHessling1/status/2055848923328364823`
- Hugging Face page: `https://huggingface.co/Jackrong/Qwopus3.5-9B-Coder-GGUF`

Those links are useful context for provenance, but the code should be understood from the Teamy files listed above rather than from the social-media thread.
