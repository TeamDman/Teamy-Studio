# Qwopus Qwen3.5 Burn Handoff 2026-05-23

## Purpose

This note captures the current state of the Burn-based Jackrong/Qwopus Qwen3.5 LLM work in Teamy Studio, with emphasis on:

- what is already implemented;
- what we were doing immediately before work was interrupted;
- the concrete diagnostics and timings we observed;
- the local model paths, URLs, commands, commits, and files that matter for resuming.

This is a preservation-first handoff, not a rewrite note.

## Relevant URLs

- X post that kicked this off:
  - `https://x.com/KyleHessling1/status/2055848923328364823`
- Saved local snapshot of that X post:
  - `file:///C:/Users/TeamD/OneDrive/Memes/Bulk%2099/Kyle%20Hessling%20on%20X%EF%BC%9A%20%EF%BC%82Hello%20again,%20everyone!%20_We've%20got%20another%20really%20fun%209b,%20this%20one%20specifically%20trained%20for%20tool%20calling%20and%20agentic%20coding%20workflows%20in%20@NousResearch%20Hermes%20agent%E2%80%A6.html`
- Hugging Face model page referenced from the post:
  - `https://huggingface.co/Jackrong/Qwopus3.5-9B-Coder-GGUF`
- Additional reading path mentioned by the user:
  - `G:\Programming\Repos\Agent-Scratchpad\teamy-studio-reference-reading-2026-05-20\main.typ`

## Important Reality Check

This is **not** a 2B model.

The local Teamy metadata for the model under `.teamy-cache` says:

- `display_name`: `Jackrong/Qwopus3.5-9B-Coder-GGUF (Q4_K_M)`
- `source_repo_id`: `Jackrong/Qwopus3.5-9B-Coder`
- `model_repo_id`: `Jackrong/Qwopus3.5-9B-Coder-GGUF`
- `parameter_count`: `9B`
- `size_estimate`: `5.63 GiB`

Source:

- [.teamy-cache/models/llm/qwopus-3.5-9b-coder-q4-k-m/model-metadata.json](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/.teamy-cache/models/llm/qwopus-3.5-9b-coder-q4-k-m/model-metadata.json)

The Hugging Face `config.json` also clearly describes the Qwen3.5 hybrid text architecture:

- `hidden_size`: `4096`
- `intermediate_size`: `12288`
- `num_hidden_layers`: `32`
- `num_attention_heads`: `16`
- `num_key_value_heads`: `4`
- `head_dim`: `256`
- `layer_types`: repeating `linear_attention` with `full_attention` every 4th layer
- `full_attention_interval`: `4`
- `linear_num_key_heads`: `16`
- `linear_num_value_heads`: `32`
- `linear_key_head_dim`: `128`
- `linear_value_head_dim`: `128`

Source:

- [.teamy-cache/models/llm/qwopus-3.5-9b-coder-q4-k-m/config.json](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/.teamy-cache/models/llm/qwopus-3.5-9b-coder-q4-k-m/config.json)

## Relevant Local Model Paths

- Model root:
  - [.teamy-cache/models/llm/qwopus-3.5-9b-coder-q4-k-m](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/.teamy-cache/models/llm/qwopus-3.5-9b-coder-q4-k-m)
- Burn conversion manifest:
  - [.teamy-cache/models/llm/qwopus-3.5-9b-coder-q4-k-m/burn-text/burn-text-manifest.json](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/.teamy-cache/models/llm/qwopus-3.5-9b-coder-q4-k-m/burn-text/burn-text-manifest.json)
- Burn packed tensor payload:
  - [.teamy-cache/models/llm/qwopus-3.5-9b-coder-q4-k-m/burn-text/tensors.bin](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/.teamy-cache/models/llm/qwopus-3.5-9b-coder-q4-k-m/burn-text/tensors.bin)
- Tokenizer:
  - [.teamy-cache/models/llm/qwopus-3.5-9b-coder-q4-k-m/tokenizer.json](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/.teamy-cache/models/llm/qwopus-3.5-9b-coder-q4-k-m/tokenizer.json)
- Tokenizer config:
  - [.teamy-cache/models/llm/qwopus-3.5-9b-coder-q4-k-m/tokenizer_config.json](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/.teamy-cache/models/llm/qwopus-3.5-9b-coder-q4-k-m/tokenizer_config.json)

Size observations from the local converted model:

- `burn-text/tensors.bin`: `17,907,606,528` bytes
- `burn-text` total on disk: `17,907,705,561` bytes

This is a big part of why VRAM pressure was so high even though the source GGUF is quantized.

## Relevant Source Files

Main implementation files:

- [crates/teamy_studio_llm_stack/src/burn_text.rs](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/crates/teamy_studio_llm_stack/src/burn_text.rs)
- [crates/teamy_studio_llm_stack/src/runtime.rs](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/crates/teamy_studio_llm_stack/src/runtime.rs)
- [crates/teamy_studio_llm_stack/src/model.rs](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/crates/teamy_studio_llm_stack/src/model.rs)
- [crates/teamy_studio_llm_stack/src/reference.rs](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/crates/teamy_studio_llm_stack/src/reference.rs)
- [python/llm-reference/teamy_llm_reference/__main__.py](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/python/llm-reference/teamy_llm_reference/__main__.py)
- [crates/teamy_studio_cli/src/cli/llm/prompt/llm_prompt_cli.rs](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/crates/teamy_studio_cli/src/cli/llm/prompt/llm_prompt_cli.rs)

## Relevant Commits

Latest checkpoints:

1. `21fca19` `Add Burn-based Qwen3.5 LLM runtime`
2. `c603b18` `Cache Burn text weights across generation`

Those are useful mental checkpoints:

- `21fca19` is the first substantial Burn runtime landing.
- `c603b18` is the weight-caching checkpoint before the newer incremental-diagnostics work that is currently uncommitted.

## Current Git State

At the time this note was written:

- modified:
  - [crates/teamy_studio_llm_stack/src/burn_text.rs](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/crates/teamy_studio_llm_stack/src/burn_text.rs)
  - [crates/teamy_studio_cli/src/cli/llm/prompt/llm_prompt_cli.rs](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/crates/teamy_studio_cli/src/cli/llm/prompt/llm_prompt_cli.rs)
- untracked runtime artifacts:
  - `.teamy-cache/`
  - `.teamy-home/`

Those cache/home directories should remain local artifacts, not committed repository state.

## What Was Working Before The Latest Deep-Debug Pass

Before the latest attention-layout investigation:

- Burn inference worked end-to-end on the Jackrong model.
- CUDA was the preferred default backend.
- Weight caching across generation significantly improved multi-token runtime.
- The stable warm output for:

```powershell
.\target\release\teamy-studio.exe llm prompt "Hello again" --max-new-tokens 10
```

was:

```text
 (urm.nt)
 tick'sont,
```

Earlier timings of note:

- warm `--max-new-tokens 1`: about `39.16s`
- warm `--max-new-tokens 10`: about `77.61s`

Before the weight-caching pass, the 10-token continuation had been roughly `333.82s`, so that caching pass was a real win.

## What Was Added In The Current Uncommitted Pass

The current working tree adds diagnostic tooling around the experimental incremental decode path:

### New CLI flags

In [crates/teamy_studio_cli/src/cli/llm/prompt/llm_prompt_cli.rs](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/crates/teamy_studio_cli/src/cli/llm/prompt/llm_prompt_cli.rs):

- `--compare-incremental`
- `--compare-incremental-hidden`
- `--compare-incremental-layers`

Important behavior:

- these diagnostics return early and do **not** run the ordinary prompt path afterward;
- this was changed because the earlier version reran too much work and made iteration painfully slow.

### New Burn diagnostic/report types and helpers

In [crates/teamy_studio_llm_stack/src/burn_text.rs](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/crates/teamy_studio_llm_stack/src/burn_text.rs):

- `BurnTextIncrementalComparisonReport`
- `BurnTextHiddenDifferenceSummary`
- `BurnTextIncrementalHiddenDiagnosticsReport`
- `BurnTextLayerHiddenDifference`
- `BurnTextIncrementalLayerDiagnosticsReport`

New public helpers:

- `compare_with_incremental_burn_text_runtime(...)`
- `diagnose_incremental_hidden_burn_text_runtime(...)`
- `diagnose_incremental_layers_burn_text_runtime(...)`

New runtime helpers/scaffolding:

- `forward_token_hidden_layer_outputs(...)`
- `full_last_hidden_for_prompt(...)`
- `full_layer_outputs_for_prompt(...)`
- `incremental_last_hidden_for_prompt(...)`
- `incremental_layer_outputs_for_prompt(...)`
- `compare_hidden_slices(...)`
- `first_mismatch_index(...)`

### New full-attention layout patch

The most recent code change before interruption was a tensor-layout patch in the full-attention path:

- `transpose_seq_head_layout(...)`

and its use around:

- full-attention query layout
- repeated key layout
- repeated value layout

Relevant region:

- [crates/teamy_studio_llm_stack/src/burn_text.rs](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/crates/teamy_studio_llm_stack/src/burn_text.rs)

This patch was motivated by the finding that divergence began at the first `full_attention` layer while earlier `linear_attention` layers matched exactly.

## Key Diagnostics We Observed

### 1. Fast incremental token comparison before the full-attention layout patch

Command:

```powershell
$env:TEAMY_STUDIO_CACHE_DIR='C:\Users\TeamD\.codex\worktrees\3458\Teamy-Studio\.teamy-cache'
$env:TEAMY_STUDIO_HOME_DIR='C:\Users\TeamD\.codex\worktrees\3458\Teamy-Studio\.teamy-home'
$env:TEAMY_STUDIO_LLM_BACKEND='cuda'
.\target\release\teamy-studio.exe llm prompt "Hello again" --model-dir .teamy-cache\models\llm\qwopus-3.5-9b-coder-q4-k-m --max-new-tokens 1 --compare-incremental
```

Observed output:

```text
Burn incremental comparison backend: cuda
Burn incremental token match: false
Burn incremental first mismatch index: Some(0)
Burn incremental stable token ids: [318]
Burn incremental experimental token ids: [198]
Burn incremental stable text: " ("
Burn incremental experimental text: "\n"
```

Meaning:

- the experimental incremental path diverged on the **very first generated token**.

### 2. Hidden-state diagnostic

Command:

```powershell
$env:TEAMY_STUDIO_CACHE_DIR='C:\Users\TeamD\.codex\worktrees\3458\Teamy-Studio\.teamy-cache'
$env:TEAMY_STUDIO_HOME_DIR='C:\Users\TeamD\.codex\worktrees\3458\Teamy-Studio\.teamy-home'
$env:TEAMY_STUDIO_LLM_BACKEND='cuda'
.\target\release\teamy-studio.exe llm prompt "Hello again" --model-dir .teamy-cache\models\llm\qwopus-3.5-9b-coder-q4-k-m --compare-incremental-hidden
```

Observed output:

```text
Burn incremental hidden diagnostics backend: cuda
Burn incremental first-prompt-token hidden diff: max_abs=0.017169952 mean_abs=0.0007743057
Burn incremental full-prompt hidden diff: max_abs=27.423296 mean_abs=1.9665203
```

Meaning:

- the first prompt token was very close;
- the full prompt hidden state was very far apart;
- the problem likely lived in **state accumulation across prompt tokens**, not in the first token’s local math.

### 3. Per-layer hidden diagnostic

Command:

```powershell
$env:TEAMY_STUDIO_CACHE_DIR='C:\Users\TeamD\.codex\worktrees\3458\Teamy-Studio\.teamy-cache'
$env:TEAMY_STUDIO_HOME_DIR='C:\Users\TeamD\.codex\worktrees\3458\Teamy-Studio\.teamy-home'
$env:TEAMY_STUDIO_LLM_BACKEND='cuda'
.\target\release\teamy-studio.exe llm prompt "Hello again" --model-dir .teamy-cache\models\llm\qwopus-3.5-9b-coder-q4-k-m --compare-incremental-layers
```

Observed output:

```text
Burn incremental first large diff layer index: Some(3)
Burn incremental layer 0 (linear_attention) hidden diff: max_abs=0 mean_abs=0
Burn incremental layer 1 (linear_attention) hidden diff: max_abs=0 mean_abs=0
Burn incremental layer 2 (linear_attention) hidden diff: max_abs=0 mean_abs=0
Burn incremental layer 3 (full_attention) hidden diff: max_abs=15.282642 mean_abs=0.17980942
...
Burn incremental layer 31 (full_attention) hidden diff: max_abs=380.946 mean_abs=6.1503515
```

Meaning:

- layers `0`, `1`, and `2` matched exactly;
- divergence began at the **first `full_attention` layer**;
- that strongly suggested an issue in the full-sequence full-attention tensor layout rather than the linear-attention recurrence.

### 4. What happened after the full-attention layout patch

After introducing `transpose_seq_head_layout(...)` and using it in the full-attention path:

Command:

```powershell
$env:TEAMY_STUDIO_CACHE_DIR='C:\Users\TeamD\.codex\worktrees\3458\Teamy-Studio\.teamy-cache'
$env:TEAMY_STUDIO_HOME_DIR='C:\Users\TeamD\.codex\worktrees\3458\Teamy-Studio\.teamy-home'
$env:TEAMY_STUDIO_LLM_BACKEND='cuda'
$env:TEAMY_STUDIO_LLM_TRACE='1'
.\target\release\teamy-studio.exe llm prompt "Hello again" --model-dir .teamy-cache\models\llm\qwopus-3.5-9b-coder-q4-k-m --max-new-tokens 1
```

Observed output:

```text
Rust Burn output:

[teamy-llm-trace] selected token id 198
```

Meaning:

- the previously “stable” full-sequence path now also lands on token `198`;
- this strongly suggests that the stable and incremental paths have moved into agreement on the full-attention behavior;
- however, we **did not finish validating whether `198` is actually correct with respect to Python/reference**.

This was the exact point implementation work was interrupted.

## Immediate Resume Question

The next question to answer is:

> Does token `198` match the Python / upstream reference, or are both Rust paths now consistently wrong in the same way?

This is the most important open question.

Do **not** assume that the latest full-attention patch is “the fix” just because it made stable and incremental agree more closely.

## Recommended Resume Sequence

When resuming implementation, use this order:

1. Verify no stale `teamy-studio.exe` is still holding VRAM.
2. Run the Python reference comparison for the same prompt and same token count.
3. Compare Python top token ids/logits against current Rust output after the latest full-attention patch.
4. If Python says `198`, then the old `318` snapshot was wrong and the next job is performance/memory cleanup.
5. If Python still says `318`, then the new full-attention reshape patch is overcorrecting or reshaping the wrong tensors.

## Suggested Commands To Resume

### Basic environment

```powershell
$env:TEAMY_STUDIO_CACHE_DIR='C:\Users\TeamD\.codex\worktrees\3458\Teamy-Studio\.teamy-cache'
$env:TEAMY_STUDIO_HOME_DIR='C:\Users\TeamD\.codex\worktrees\3458\Teamy-Studio\.teamy-home'
$env:TEAMY_STUDIO_LLM_BACKEND='cuda'
```

### Fast validation

```powershell
cargo clippy -p teamy_studio_llm_stack --features extended_observability,tracing_subscriber_tracy -- -D warnings
cargo test -p teamy_studio_llm_stack --features extended_observability,tracing_subscriber_tracy
```

### Rebuild release binary

```powershell
cargo build --release --bin teamy-studio
```

### Stable prompt run

```powershell
.\target\release\teamy-studio.exe llm prompt "Hello again" --model-dir .teamy-cache\models\llm\qwopus-3.5-9b-coder-q4-k-m --max-new-tokens 1
```

### Python comparison

```powershell
.\target\release\teamy-studio.exe llm prompt "Hello again" --model-dir .teamy-cache\models\llm\qwopus-3.5-9b-coder-q4-k-m --max-new-tokens 1 --compare-python --python-device cuda
```

### Incremental diagnostics

```powershell
.\target\release\teamy-studio.exe llm prompt "Hello again" --model-dir .teamy-cache\models\llm\qwopus-3.5-9b-coder-q4-k-m --max-new-tokens 1 --compare-incremental
```

```powershell
.\target\release\teamy-studio.exe llm prompt "Hello again" --model-dir .teamy-cache\models\llm\qwopus-3.5-9b-coder-q4-k-m --compare-incremental-hidden
```

```powershell
.\target\release\teamy-studio.exe llm prompt "Hello again" --model-dir .teamy-cache\models\llm\qwopus-3.5-9b-coder-q4-k-m --compare-incremental-layers
```

### Tracy / profiling context

Relevant captures already in the repo:

- [tracy/manual-2026-05-23-llm-burn.tracy](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/tracy/manual-2026-05-23-llm-burn.tracy)
- [tracy/manual-2026-05-23-llm-burn-postdecode.tracy](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/tracy/manual-2026-05-23-llm-burn-postdecode.tracy)

If an `.exe` lock blocks rebuilding, the existing repo guidance says to use:

```powershell
.\stop.ps1
```

## GPU Memory Situation

At one point Task Manager showed the RTX 4090 at `24/24 GB`, and `nvidia-smi` showed roughly:

```text
24055 MiB / 24564 MiB
```

Important contributing factors:

- this is the `9B` model, not a `2B` model;
- the Burn runtime is currently working from the converted `burn-text` payload, not directly from the quantized GGUF;
- the converted `tensors.bin` is about `16.7 GiB`;
- additional VRAM is consumed by device tensors, temporary allocations, activations, and allocator/cache overhead;
- aborted or long-running `teamy-studio.exe` processes can linger and hold the allocation.

At one point a live process holding GPU memory was:

- `teamy-studio.exe` from `target\release`

The user later closed the live instance before requesting this note.

## Performance Notes

Iteration got much slower when the comparison path did too much at once.

Bad pattern:

- one command performing hidden diagnostics, stable generation, incremental generation, and then the normal prompt generation again.

Improvement already made:

- the diagnostic CLI flags now early-return after printing their own report.

Even after that, release diagnostic runs are still expensive on this model.

## Most Important Hypotheses To Preserve

1. The earliest clean evidence pointed to the first `full_attention` layer, not the linear-attention recurrence.
2. The incremental decode path looked locally plausible for the first prompt token but drifted badly over the full prompt.
3. The latest reshape patch appears to have pulled the stable path toward the incremental path.
4. The next truth check must be against Python reference, not against the older Rust snapshot alone.

## Files To Treat Carefully

- [crates/teamy_studio_llm_stack/src/burn_text.rs](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/crates/teamy_studio_llm_stack/src/burn_text.rs)
- [crates/teamy_studio_cli/src/cli/llm/prompt/llm_prompt_cli.rs](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/crates/teamy_studio_cli/src/cli/llm/prompt/llm_prompt_cli.rs)
- [python/llm-reference/teamy_llm_reference/__main__.py](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/python/llm-reference/teamy_llm_reference/__main__.py)

Avoid speculative cleanup here. The current note is meant to preserve the debugging trail.

## Addendum 2026-05-24

This follow-up pass tightened two areas:

- model-derived runtime behavior:
  - `serde`/`serde_json` are gone from `teamy_studio_llm_stack`; the LLM stack now uses `facet` + `facet-json`;
  - Burn manifest/runtime now derives and requires `hidden_act` and `rope_theta` from model config/export instead of silently hardcoding them;
  - Burn runtime validates tensor shapes against manifest-derived dimensions up front, so several previously implicit assumptions are now explicit contract checks.
- graceful long-run timeout handling:
  - `llm prompt` now accepts `--generation-timeout` with `humantime` parsing, for example `1s`, `90s`, or `5m`;
  - the timeout is checked at token boundaries inside the Burn generation loops;
  - a timeout is now treated as a real error, not as a signal to fall back from CUDA to CPU.

### New Relevant Files / Surfaces

- [crates/teamy_studio_llm_stack/src/source_config.rs](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/crates/teamy_studio_llm_stack/src/source_config.rs)
- [crates/teamy_studio_llm_stack/src/runtime.rs](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/crates/teamy_studio_llm_stack/src/runtime.rs)
- [crates/teamy_studio_llm_stack/src/burn_text.rs](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/crates/teamy_studio_llm_stack/src/burn_text.rs)
- [crates/teamy_studio_cli/src/cli/llm/prompt/llm_prompt_cli.rs](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/crates/teamy_studio_cli/src/cli/llm/prompt/llm_prompt_cli.rs)
- [python/llm-reference/teamy_llm_reference/__main__.py](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/python/llm-reference/teamy_llm_reference/__main__.py)

### Fresh Benchmark Model Path

Because the original managed `burn-text` directory was Windows-locked and still had the older manifest schema, a fresh local benchmark bundle was created here:

- [python/llm-reference/bench-model](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/python/llm-reference/bench-model)
- [python/llm-reference/bench-model/burn-text/burn-text-manifest.json](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/python/llm-reference/bench-model/burn-text/burn-text-manifest.json)

This bundle was exported from the local cached Hugging Face snapshot:

- `G:\Programming\Caches\huggingface\hub\models--Jackrong--Qwopus3.5-9B-Coder\snapshots\7ee683ab9ff928d315410a1e0ae1b4cbde854b70`

### Fresh Measured Timings

Release 1-token baseline on the fresh benchmark bundle:

```powershell
.\target\release\teamy-studio.exe llm prompt "Hello again" --model-dir python\llm-reference\bench-model --max-new-tokens 1
```

Observed:

- exit code: `0`
- wall-clock: about `46.995s`
- generated text: `<think>`

### 100-Token Timeout Run

Command intent:

```powershell
.\target\release\teamy-studio.exe llm prompt "Hello again" --model-dir python\llm-reference\bench-model --max-new-tokens 100 --generation-timeout 5m
```

The most reliable observation came from a traced background run, because direct wrapper timing kept hitting the outer tool timeout before surfacing the process output.

Observed result:

- the process exited on its own;
- `selected token id` appeared `3` times in the trace;
- timeout surfaced exactly as an LLM runtime error, not as a CPU fallback;
- final error message:

```text
Burn text generation timed out after 5m 58s 531ms 603us 600ns while producing 3 of 100 requested tokens
```

Important nuance:

- the timeout is checked at token boundaries, not in the middle of a single token’s forward pass;
- because of that, a requested `5m` timeout can surface later than `5m` wall-clock if the token in flight takes a long time to finish;
- in this observed run, the timeout was enforced after the third generated token completed, which is why the reported elapsed time was just under `6m`.

### Timeout Bug That Was Fixed

Before the fix in this pass:

- CUDA timeout errors were being treated as generic CUDA failures;
- the runtime would then fall back to CPU, which made timeout probes look like hangs.

Now:

- timeout errors stay fatal and bubble up immediately;
- CUDA only falls back to CPU for actual backend-availability failures.

### Residual Hardcoded Assumption Still Worth Revisiting

The biggest remaining obvious model-specific assumption is still prompt rendering:

- [crates/teamy_studio_llm_stack/src/runtime.rs](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/crates/teamy_studio_llm_stack/src/runtime.rs) still hardcodes a Qwen-style single-turn prompt shape;
- the tokenizer config does contain a `chat_template`, and the runtime does report whether it is present;
- however, this pass did **not** replace the hardcoded renderer with a true tokenizer-config-driven template application.

That is the next major “derive from model files instead of assumptions” candidate if work continues in this direction.

## 2026-05-24 Burn Decode-State Capture Addendum

### Reference Repo

Local reference repo inspected during continuation optimization:

- `G:\Programming\Repos\nano-vllm`

Notable ideas worth carrying forward:

- separate prefill and decode execution shapes;
- persist decode state instead of replaying the prompt token by token;
- treat cache construction as a first-class runtime concern rather than an incidental side effect;
- keep future Teamy implementation Burn-native and vendored, rather than introducing new inference dependencies.

### What Changed In Teamy

The Burn runtime in [crates/teamy_studio_llm_stack/src/burn_text.rs](/C:/Users/TeamD/.codex/worktrees/3458/Teamy-Studio/crates/teamy_studio_llm_stack/src/burn_text.rs) now has:

- explicit generation modes: `full`, `incremental`, and `hybrid`;
- a default `hybrid` mode selected by `TEAMY_STUDIO_LLM_BURN_GENERATION_MODE` when not overridden;
- timeout checks during prompt-token processing for the incremental path;
- batched prefill decode-state capture via `forward_hidden_states_with_decode_state(...)`;
- per-layer state population during batched prefill for both:
  - full-attention repeated key/value caches;
  - linear-attention recurrent state and convolution history;
- a raw 1D tensor-values cache to avoid recloning small norm/bias vectors repeatedly.

The important architectural shift is:

- old hybrid attempt: generate token 1 with the full path, then replay the whole prompt through the single-token incremental path to build cache state;
- current hybrid path: generate token 1 with the full path, capture decode state during that same batched prefill, then hand off directly to incremental continuation.

### Fresh Observations

Short continuation run:

```powershell
.\target\release\teamy-studio.exe llm prompt "Hello again" --model-dir python\llm-reference\bench-model --max-new-tokens 3 --generation-timeout 5m
```

Observed:

- exit code: `0`
- wall-clock: about `47.4s`
- generated text:

```text
<think>
The
```

Trace confirms the new handoff shape:

- `hybrid generating token 1 with prompt length 10`
- `hybrid decoder stack complete; scanning lm_head`
- `hybrid selected token id 248068`
- `hybrid handing off first generated token to cached decode state`
- `hybrid generating token 2 after 11 processed tokens`
- `hybrid selected token id 198`
- `hybrid generating token 3 after 12 processed tokens`
- `hybrid selected token id 760`

Longer continuation is still too slow:

```powershell
.\target\release\teamy-studio.exe llm prompt "Hello again" --model-dir python\llm-reference\bench-model --max-new-tokens 10 --generation-timeout 5m
```

Observed result from the latest run:

- timeout after about `5m 3s`;
- error reported `while producing 1 of 10 requested tokens`;
- trace had only reached the first generated token in that specific run.

### Current Hypothesis

The biggest remaining issue is no longer prompt replay. That part was meaningfully improved.

The remaining bottleneck is the single-token incremental path itself:

- it still performs many tiny GPU matmuls;
- it still does frequent device-to-host roundtrips via `tensor_to_vec_f32(...)`;
- it still mixes Burn tensor math with large amounts of scalar/vector host-side postprocessing.

That means the next serious optimization target is:

- reducing or eliminating host synchronization inside `forward_token_hidden(...)` and its per-layer helpers,
- especially the single-token full-attention and linear-attention branches.

## 2026-05-24 Performance Breakthrough Addendum

### Changes That Moved The Needle

The following changes produced the first successful sub-minute 100-token benchmark:

- CUDA Burn backend changed from `Cuda<f32, i32>` to `Cuda<f16, i32>`;
- `tensor_to_vec_f32(...)` now explicitly converts Burn tensor data before extracting `Vec<f32>`, which keeps the half-precision CUDA path valid instead of tripping a type mismatch and falling back to CPU;
- device-bound 1D/2D tensor loads now decode directly from `float16` / `bfloat16` / `float32` bytes into Burn `TensorData`, instead of first expanding everything to host `f32` and then converting again on device;
- the MLP path was moved back onto device for both:
  - `forward_mlp(...)`
  - `forward_mlp_single(...)`
- the runtime still uses the hybrid prefill/decode-state-capture path from the previous addendum.

### Important Benchmark Result

Clean release benchmark command:

```powershell
.\target\release\teamy-studio.exe llm prompt "Count from 1 to 200, comma separated." --model-dir python\llm-reference\bench-model --max-new-tokens 100 --generation-timeout 1m
```

Observed:

- exit code: `0`
- actual generated tokens: `100`
- wall-clock: about `31.48s`

Validation method:

- a traced run with `TEAMY_STUDIO_LLM_TRACE=1` showed:
  - `GENERATING_LINES=100`
  - `SELECTED_LINES=100`
- the clean wall-clock measurement used the same prompt without extra trace overhead.

### Supporting Measurements

- `Hello again` with `--max-new-tokens 10` completed in about `22.2s`
- `Hello again` with `--max-new-tokens 1` had previously dropped to about `27.1s` during this same optimization wave

### Caution About Benchmark Interpretation

The earlier `Hello again` benchmark prompt can terminate early due to EOS before reaching 100 tokens, so it is not a reliable throughput benchmark by itself.

The counting prompt above is the benchmark that confirmed the actual target:

- 100 requested tokens
- completed under 1 minute
- on the current Burn-based Rust path
