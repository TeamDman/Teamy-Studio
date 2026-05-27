# LLM Inference Reference Repos

Bookkeeping for local reference material used while improving Teamy-Studio's local LLM runtime.

These repos were cloned as read-only comparison material on 2026-05-27 with submodules disabled. Do not run install scripts, package-manager syncs, notebooks, or build commands from these repos until their manifests and setup scripts have been reviewed.

## Local Checkouts

| Reference | Upstream | Local checkout | Why it is relevant |
| --- | --- | --- | --- |
| Shard | https://github.com/krish1905/shard | `G:\Programming\Repos\shard` | Direct reference from the bookmarked Krish Garg post. Drop-in `transformers.Cache` subclass for asymmetric KV-cache compression: PCA/int4 keys, Hadamard plus vector-quantized values, compressed attention, and TurboQuant-style decode streaming. |
| TurboQuant reference | https://github.com/OmarHory/turboquant | `G:\Programming\Repos\turboquant` | Compact Python reference for TurboQuant-style KV-cache compression, including MSE quantization, QJL, packed indices, cache integration, quantized attention, and Triton kernels. |
| TurboQuant landscape | https://github.com/OnlyTerp/turboquant | `G:\Programming\Repos\turboquant-landscape` | Practical integration and comparison notes for TurboQuant variants across vLLM, SGLang, llama.cpp, MLX, and related KV-cache approaches. |
| nano-vLLM | https://github.com/GeeeekExplorer/nano-vllm | `G:\Programming\Repos\nano-vllm` | Already-local lightweight vLLM-style reference for scheduler, block manager, model runner, attention layout, and Qwen model flow. |
| vLLM | https://github.com/vllm-project/vllm | `G:\Programming\Repos\vllm` | Production serving-engine reference for paged attention, KV-cache dtype options, FP8 KV cache, TurboQuant integration points, and benchmark framing. |
| NVIDIA KVPress | https://github.com/NVIDIA/kvpress | `G:\Programming\Repos\kvpress` | Research framework for KV-cache compression and pruning strategies. Useful for comparing quantization against cache eviction/press methods. |
| KIVI | https://github.com/jy-yuan/KIVI | `G:\Programming\Repos\KIVI` | Prior-art baseline for tuning-free asymmetric 2-bit KV-cache quantization, commonly compared against newer TurboQuant-style methods. |

## Initial Optimization Questions

- How does Teamy's current Burn decode state store K/V tensors, and what is the exact per-token memory traffic during decode?
- Is current generation limited by memory capacity, memory bandwidth, kernel launch overhead, or tensor layout churn?
- Can a compressed-cache prototype be isolated behind an experimental runtime flag while preserving the FP16/BF16 cache path as the reference?
- Which validation harness should gate correctness first: bit/token match on short deterministic decode, NIAH-style retrieval, or LongBench-style quality deltas?
- For Qwen-family models, should K and V use different bit budgets or layouts, following the Shard and TurboQuant guidance that K/V sensitivity can be asymmetric?

## Safety Notes

- Clones were created with `--no-recurse-submodules`.
- Treat these repos as source references until reviewed.
- Avoid `pip install -e .`, `pip install -r requirements.txt`, `uv sync`, `npm install`, `pre-commit install`, and submodule initialization until needed and inspected.
