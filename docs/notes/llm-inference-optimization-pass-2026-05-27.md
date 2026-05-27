# LLM Inference Optimization Pass 2026-05-27

This note records the first optimization pass after cloning the local inference reference repos listed in `docs/notes/llm-inference-reference-repos-2026-05-27.md`.

## Context

Recent Teamy-Studio commits added a Burn-based Qwen3.5/Qwopus local LLM runtime and then improved CUDA generation enough to complete a 100-token counting prompt in under a minute on a prepared Burn benchmark bundle. In this checkout, that prepared bundle is not present, so this pass focused on structural decode-path improvements that can be validated without the full model artifact.

The most relevant reference repo finding was from Shard: for grouped-query attention, K/V cache storage should preserve the smaller KV-head shape rather than eagerly expanding it to every query head. Shard goes much further with compressed K/V formats, but keeping grouped KV compact is the low-risk first step for Teamy's current runtime.

## Changes Landed

- `8ea5c19` documents the local LLM inference reference repos.
- `13d244f` reduces per-token decode scratch allocations:
  - full-attention single-step softmax now normalizes scores in place instead of allocating a second weights vector;
  - recurrent gated-delta single-step reuses the `delta` scratch buffer instead of allocating a separate `kv_mem` buffer.
- `524844f` stores compact grouped K/V decode cache:
  - full-attention decode state now stores only `num_key_value_heads` K/V entries;
  - each query head maps back to its grouped KV head during attention score and value lookup;
  - prefill decode-state capture stores the compact K/V vectors while retaining the existing expanded tensor path for the batched prefill output.

## Validation

Focused validation after the runtime changes:

```powershell
cargo clippy -p teamy_studio_llm_stack --features extended_observability,tracing_subscriber_tracy -- -D warnings
cargo test -p teamy_studio_llm_stack --features extended_observability,tracing_subscriber_tracy
cargo build --release --bin teamy-studio
```

All three passed.

The exact CUDA benchmark from the prior handoff could not be rerun because `python/llm-reference/bench-model` is absent in this checkout and the normal Teamy app cache has no managed LLM model installed.

## Local Benchmark State

- The release CLI now exists at `target/release/teamy-studio.exe`.
- `teamy-studio.exe llm model list` reports the known Qwopus model as missing from the normal app cache.
- A local Hugging Face source snapshot for `Jackrong/Qwopus3.5-9B-Coder` exists, but this pass did not recreate the temporary Burn benchmark bundle because the managed tokenizer/config side of the model bundle is not currently present in this checkout.

## Next Steps

1. Recreate a temporary Burn benchmark model directory, preferably under `target/tmp/llm-bench-model` or another ignored local artifact path.
2. Run the known stable throughput prompt:

```powershell
$env:TEAMY_STUDIO_LLM_BACKEND='cuda'
Measure-Command { .\target\release\teamy-studio.exe llm prompt "Count from 1 to 200, comma separated." --model-dir target\tmp\llm-bench-model --max-new-tokens 100 --generation-timeout 1m | Out-Host } | Select-Object TotalSeconds
```

3. Compare current timing against the earlier documented roughly 31.48s release result.
4. If timing regresses, use the commits above as rollback points and compare `TEAMY_STUDIO_LLM_TRACE=1` token progress.
5. If timing holds or improves, the next larger optimization target is reducing host synchronization inside `forward_linear_attention_single`, especially the projection-to-CPU transitions around linear attention.
