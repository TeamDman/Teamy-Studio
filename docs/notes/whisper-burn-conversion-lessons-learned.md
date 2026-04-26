# Whisper To Burn Conversion Lessons Learned

This note captures what we learned while converting OpenAI Whisper checkpoints into Teamy Studio's Burn-based transcription model path. It is intentionally detailed because the failure mode was subtle: the model loaded, inference ran, tensors had plausible shapes, the frontend matched Python, and yet the decoded text collapsed into repeated nonsense such as `absorabsorabsorabsor`.

The short version is that the final bug was not in greedy decoding, not in the log-mel frontend, not in CUDA versus CPU execution, and not primarily in token suppression. The converted Burn model had corrupted linear weights because the source OpenAI Whisper checkpoint stores many weights as non-contiguous PyTorch tensor views. Burn's PyTorch import path did not preserve the logical strided layout for those tensors. The durable fix was to normalize the checkpoint before import by materializing every floating-point tensor as `float().contiguous()`, then importing that normalized checkpoint into BurnPack.

## Goal

The goal was to add a Teamy-owned Whisper transcription path that could run from the Rust CLI without depending on Python at inference time:

```text
teamy-studio audio transcribe <wav>
teamy-studio audio transcribe --demo
teamy-studio audio model list
teamy-studio audio model prepare tiny.en
teamy-studio audio model show tiny.en
```

The intended architecture was:

- use OpenAI Whisper checkpoints as the source of truth for weights and metadata;
- convert a known Whisper checkpoint into a local BurnPack model directory;
- keep model artifacts in Teamy-managed cache state;
- use Teamy-owned Rust code for audio loading, resampling, log-mel feature extraction, tokenization, decoding, and CLI presentation;
- use CUDA through Burn for inference when running the converted model;
- preserve a Python/OpenAI Whisper comparison path for diagnosis, but avoid requiring Python for the normal Rust transcription path.

The first practical target was `tiny.en`, because it is small enough for fast iteration but still exercises the same architectural pieces: convolutional audio encoder, attention blocks, MLPs, text decoder, tokenizer, special-token prompt policy, and greedy token generation.

## Starting Point

Before the Burn model path, Teamy Studio already had a Python-based transcription lane in progress. That lane could launch Python through `uv`, load OpenAI Whisper on CUDA, send audio through a shared-memory or one-shot path, and return transcript text. That proved the machine, CUDA install, Python package environment, and model downloads were viable.

The Burn work moved toward a Rust-native path:

- `src/frontend.rs` computes Whisper-compatible log-mel features.
- `src/whisper.rs` defines the Burn Whisper model and inference helpers.
- `src/model.rs` handles model cache, checkpoint inspection, conversion, BurnPack saving, and tokenizer installation.
- `src/cli/audio/transcribe/audio_transcribe_cli.rs` exposes `audio transcribe`, demo mode, resampling, debugging, and Python comparison output.

The conversion flow originally loaded OpenAI's `.pt` checkpoint with Burn's `PytorchStore`, remapped OpenAI parameter names to Teamy's Burn module names, saved the result as `model.bpk`, and wrote `dims.json` plus `tokenizer.json` beside it.

That looked reasonable because the model directory could be created, tensors were present, and inference could run end to end.

## Symptom: The Model Ran But Said Nonsense

The first obvious failure was that Rust transcription did not match Python. On real VCTK clips, OpenAI Whisper produced sensible transcripts while the Rust Burn path produced repeated garbage. The most memorable bad output was:

```text
absorabsorabsorabsor
```

That mattered because repeated-token collapse can come from many places:

- wrong audio frontend normalization;
- wrong mel filter bank;
- missing 30-second padding;
- wrong decoder prompt tokens;
- wrong tokenizer file;
- missing token suppression;
- greedy decoding bugs;
- CPU/CUDA numeric differences;
- model architecture mismatch;
- parameter-name remapping errors;
- parameter layout corruption.

The hard part was that the failure was not a crash. The system produced valid tensors with valid shapes. The output was semantically wrong, not structurally invalid.

## First Fix: Make Demo Mode Useful

Early demo mode always selected the same VCTK sample, which made repeated smoke testing less informative. That was useful for a fixed repro, but it did not tell us whether the model was broadly working or only memorizing one lucky path.

The demo clip finder was changed to collect all paired VCTK `wav48` and transcript files, sort them for deterministic inventory, and choose a time-seeded index. That gave quick variety through:

```text
teamy-studio audio transcribe --demo
```

This did not fix the divergence, but it improved the debugging loop. A transcription model should succeed on random ordinary clips, not just on the one clip we have stared at all afternoon.

Lesson: when debugging ML parity, keep one fixed repro for exact comparisons and one randomized smoke path for reality checks.

## First Suspicion: The Prompt Was Wrong

The first real mismatch we found was in the decoder prompt.

For multilingual Whisper models, the prompt usually starts with:

```text
<|startoftranscript|> <|en|> <|transcribe|> <|notimestamps|>
```

But `tiny.en` is an English-only model. OpenAI Whisper's tokenizer prompt for `tiny.en` does not include the `<|en|>` language token. Python used:

```text
[50257, 50358, 50362]
```

Rust was originally using the multilingual-looking prompt:

```text
[50257, <|en|>, 50358, 50362]
```

That was a real bug. The fix was to detect English-only models from the model dimensions and omit `<|en|>` when the vocabulary size indicates an English-only checkpoint.

This was necessary, but it was not sufficient. After matching Python's prompt, Rust still produced nonsense. The model still diverged before decoding had a chance to be a subtle policy issue.

Lesson: prompt parity is foundational, but matching prompt token ids only proves the decoder starts from the same text context. It does not prove the model weights or activations are correct.

## Added Python Reference Comparison

The next debugging improvement was a direct Python comparison flag:

```text
teamy-studio audio transcribe <wav> --compare-python
```

That path runs OpenAI Whisper through `uv` with CUDA PyTorch, then prints a structured JSON summary including:

- Python backend name;
- device, usually `cuda:0`;
- model name;
- whether the model is multilingual;
- prompt token ids;
- transcript text;
- encoder output shape;
- decoder logits shape;
- top first-step token ids;
- top first-step logit values;
- decoded text for those top tokens.

This gave us a stable oracle. Instead of comparing only final transcripts, we could compare the very first decoder step. That matters because final text can diverge for many downstream reasons, while first-step logits tell us whether the model state before generation is already wrong.

On the fixed VCTK clip, Python's first token was clearly sensible. After the final fix, Python's top id was `554` with a logit around `23.895`, and Rust's top id became the same `554` with a logit around `23.901`. Before the final fix, Rust's top ids and values were nowhere close.

Lesson: final transcript comparison is too coarse for model-porting work. First-step logits are the smallest high-value parity check for encoder plus decoder correctness.

## Checked CUDA And Backend Differences

There was a period where CPU versus CUDA differences were a plausible suspect. Some helper paths originally loaded BurnPack with the CPU backend, while the main inference path used CUDA. That made diagnostics harder to interpret because one command might compare Python CUDA against Rust CPU helper output or use a helper path that did not match production inference.

The helper paths were adjusted so BurnPack prompt-forward and encoder-shape summaries use the same CUDA inference backend as normal transcription. This made comparison fairer and closer to the path users actually run.

This did not fix the bad transcript either. The divergence was present when Rust was running on CUDA.

Lesson: eliminate backend skew in diagnostic helpers. Even when backend skew is not the root cause, it adds enough uncertainty to waste time.

## Checked The Audio Frontend

A very reasonable early hypothesis was that Rust's log-mel frontend was wrong. Whisper frontend bugs are common because there are many details:

- sample rate must be 16 kHz;
- audio must be mono `f32`;
- input must be padded or trimmed to 30 seconds;
- STFT windowing and hop length must match Whisper;
- mel filters must match the reference;
- log scaling and clamping must match the reference;
- tensor shape must be `[80, 3000]` for `tiny.en`.

The frontend already had focused tests against a Python reference sine wave. During the divergence hunt, real-clip activation comparisons confirmed the frontend and early convolution outputs matched Python closely enough that they were not the cause of the repeated-token collapse.

That was an important negative result. It let us stop repeatedly revisiting audio loading and feature generation.

Lesson: frontend correctness needs both synthetic tests and at least one real-clip parity check. A sine wave test catches many math mistakes, but real speech catches padding, duration, normalization, and data-shape mistakes in a more realistic path.

## Checked Tokenization And Decoding

The tokenizer and greedy decoder were also plausible suspects.

Potential issues included:

- using the wrong tokenizer file;
- missing special tokens;
- suppressing the wrong token ids;
- not stopping on end-of-text;
- allowing timestamp tokens when `without_timestamps` should be active;
- selecting max logits from the wrong sequence position;
- reading logits from the wrong flattened offset;
- producing repeated-token collapse because no repetition guard existed.

Some defensive improvements were useful. Debug mode now logs Rust's top logits at each greedy step. Repeated-token collapse detection remains useful as a safety check so obviously broken generation can stop instead of printing unbounded nonsense.

But decoding was not the root cause. Once the model conversion was fixed, the existing greedy path produced the correct transcript for the fixed sample and the random demo sample.

Lesson: if first-step logits are wrong, do not spend too long tuning generation policy. Greedy decoding can only pick from the distribution the model gives it.

## Layer-By-Layer Diagnosis

After prompt, frontend, tokenizer, and backend skew were reduced, we moved to activation parity.

The important observation was that divergence started inside the encoder stack:

- frontend matched;
- convolutional frontend matched;
- positional embedding and pre-block state were plausible;
- divergence appeared in `encoder.block_0`;
- within that block, layer norm output matched;
- attention or MLP projections then diverged.

That narrowed the search dramatically. If layer norm output matched but `q`, `k`, `v`, or MLP linear outputs diverged, the issue was probably not audio, padding, tokenizer, or high-level decode policy. It pointed at either:

- the Burn linear implementation;
- how we mapped OpenAI weights into Burn linear modules;
- whether the weight matrices needed transposition;
- whether the checkpoint reader imported the actual tensor values correctly.

Temporary runtime workarounds were tried during this phase, including compensating for suspected linear layout differences in attention projections. Those experiments were useful because they showed partial local improvements, but they did not fix the whole model. MLP projections still diverged, which made a one-off attention workaround the wrong abstraction.

Lesson: when both attention and MLP linears are suspicious, do not fix one call site. Look at the shared weight import path.

## The Misleading Transpose Hypothesis

One natural thought was that the weight matrices were transposed. Different frameworks store linear layers differently, and it is easy to get confused about whether a weight should be `[out, in]` or `[in, out]`.

This was tempting because a transposed linear can produce plausible shapes if the surrounding code also expects the opposite convention, or it can fail only after a remapping step changes names without changing layout.

But the evidence did not support a simple transpose bug:

- not all layers behaved like a uniform transpose mistake;
- partial runtime layout compensation helped some projections but not all;
- the divergence pattern pointed to source tensor value order, not just matrix orientation;
- OpenAI and Burn shapes were not enough to prove logical memory order.

The final root cause explained why the transpose hypothesis was close but incomplete. The issue was layout, but not a model-architecture transpose. It was PyTorch tensor stride and storage-order handling during checkpoint import.

Lesson: shape equality is not layout equality. A tensor can have the correct logical shape and still be read incorrectly if its strides are ignored.

## Root Cause: Non-Contiguous PyTorch Tensor Views

OpenAI Whisper checkpoints contain many floating-point tensors that are non-contiguous PyTorch views. Several linear weights are stored with strides that do not match ordinary row-major contiguous layout. For example, we observed weights with logical shape like:

```text
[1536, 384]
```

but strides like:

```text
(1, 1536)
```

and `is_contiguous=False`.

That means the logical tensor is valid in PyTorch, but the physical storage order is not the same as a plain contiguous `[out, in]` array. PyTorch operations honor the stride metadata. Burn's PyTorch import path, as used here, effectively imported the storage order without applying the tensor's logical strides. The resulting BurnPack had the right parameter names and shapes, but the wrong values in many linear matrices.

That exactly matched the symptoms:

- convolutions and some early operations could look fine;
- layer norm could match;
- linear projections could diverge sharply;
- both attention and MLP paths could be affected;
- the model could run without shape errors;
- generation could collapse into repeated nonsense.

This also explained why debugging at runtime felt slippery. The Rust model code was largely doing what it was told. The saved BurnPack weights were already corrupted.

Lesson: when importing `.pt` files from PyTorch, non-contiguous source tensors are a first-class correctness hazard. Always check `shape`, `stride`, `dtype`, and `is_contiguous`, not just shape and dtype.

## Why Earlier Attempts Did Not Work

### Prompt Fix Only

Skipping `<|en|>` for English-only models was correct and remains part of the fix. It did not solve the repeated output because corrupted linears made the model distribution wrong before generation policy mattered.

### Token Suppression And Decode Tweaks

Token suppression and repetition detection are useful guardrails, but they cannot recover a corrupted model. If the correct token is not near the top of the logits, generation policy can only hide the symptom or fail differently.

### CPU Versus CUDA Checks

Using CUDA consistently in BurnPack diagnostics improved confidence, but it did not change the wrong answers. The Python reference and Rust path were not diverging because of normal backend floating-point drift.

### Frontend Investigation

The frontend was worth checking because Whisper is sensitive to log-mel details. But activation comparisons showed the frontend and conv path were close to Python. The first major divergence happened later.

### Runtime Linear Workarounds

Temporary experiments around linear layout helped identify the neighborhood of the bug, but they were not the right fix. Compensating in selected runtime forwards would have been fragile and incomplete because the same import issue affected multiple linear layers across encoder and decoder blocks.

### Re-Preparing Without Normalization

Rebuilding BurnPack from the original checkpoint did not help until the source checkpoint was normalized. The corruption was deterministic. Repeating the same import reproduced the same bad weights.

Lesson: many failed attempts were still useful because they reduced the search space. The mistake would have been leaving a local workaround in place after the evidence pointed at conversion.

## Final Fix

The durable fix lives in model conversion.

Before importing with Burn's `PytorchStore`, Teamy Studio now creates a temporary normalized checkpoint beside the output model directory. The normalization script runs under `uv` with `torch`, loads the original OpenAI checkpoint on CPU, and replaces each floating-point tensor in `model_state_dict` with:

```python
value.detach().float().contiguous()
```

Then it saves the temporary checkpoint and imports that normalized file into Burn.

This does three things at once:

- materializes PyTorch views according to their logical strides;
- converts fp16 checkpoint weights to fp32 before import;
- gives Burn's importer a simple contiguous tensor layout.

After BurnPack is saved, the temporary normalized checkpoint is removed.

The conversion path also now fails if `PytorchStore` reports tensor errors after remapping. Before this, conversion focused on missing and unused keys, but tensor load errors deserve to be fatal. A model package that silently ignores import errors is worse than no model package.

Lesson: fix corrupted model data at the conversion boundary. Runtime code should not carry compensating hacks for bad persisted weights.

## Validation After The Fix

After implementing checkpoint normalization, `tiny.en` was re-prepared from the original fp16 OpenAI checkpoint through the new conversion path:

```text
cargo run -- audio model prepare tiny.en --overwrite
```

Then the fixed VCTK clip was transcribed with Python comparison and Rust debug logits:

```text
cargo run -- audio transcribe g:/Datasets/VCTK/VCTK-Corpus-smaller/wav48/p227/p227_271.wav --resample --overwrite --compare-python --debug --max-decode-tokens 16
```

The Python reference reported the expected transcript:

```text
In the meantime, the fans would settle for a Derby win.
```

The Rust Burn path produced the same transcript. The first-step logits also aligned closely. Python's top token was `554` with a value around `23.895`, and Rust's top token was also `554` with a value around `23.901`.

Random demo mode was also smoke tested:

```text
cargo run -- audio transcribe --demo --max-decode-tokens 16
```

One validated random sample was:

```text
Demo clip: p229_352.wav
Expected text: Everything was an effort.
Output text: Everything was an effort.
```

Finally, the repo validation passed:

```text
.\check-all.ps1
```

The validation run passed formatting, clippy, build, tests, and Tracey checks. The test summary was `323 passed; 0 failed; 5 ignored`, and Tracey audio coverage remained `53 of 53` covered.

Lesson: parity should be validated at multiple levels: first-step logits, fixed transcript, randomized demo transcript, and full repo checks.

## Practical Debugging Techniques That Paid Off

### Keep A Python Oracle In The CLI

The `--compare-python` flag is valuable even after the bug is fixed. It lets us compare against the upstream implementation from the same command users are running. This is much faster than switching between separate scripts and trying to remember whether paths, model names, prompts, and devices match.

### Compare First-Step Logits

First-step logits gave an early and precise signal. When they were wrong, we knew the problem existed before multi-step greedy decoding. When they matched, the transcript had a much better chance of matching too.

### Use Fixed And Random Samples

The fixed VCTK clip gave repeatability. Random demo clips gave confidence that the fix generalized beyond a single file.

### Diagnose By Activation Boundaries

Finding the first layer where Rust and Python diverged was the key move. It avoided endless speculation about later decode behavior and pointed us toward the shared linear import path.

### Treat Temporary Hacks As Probes

Runtime linear-layout experiments were useful as probes, but they were removed once the conversion bug was identified. Keeping them would have made the model implementation harder to reason about and likely would have broken other checkpoints.

## Risks And Follow-Up Work

The current normalization fix is pragmatic and correct for Teamy's conversion path, but it is not the ideal ecosystem-level fix. The deeper issue is that a PyTorch checkpoint reader should honor tensor strides. A future improvement would be to fix or wrap the Burn PyTorch import path so it correctly materializes non-contiguous tensors itself.

Follow-up items worth considering:

- add a conversion-time diagnostic that reports how many tensors were non-contiguous before normalization;
- add a small test fixture with a known non-contiguous tensor to prevent regressions in the import path;
- keep `--compare-python` available, but consider marking it as a diagnostic flag in help text because it requires `uv`, Python packages, and CUDA-capable PyTorch for the fastest path;
- expand validation from `tiny.en` to at least one multilingual or larger model;
- verify that timestamp suppression and multilingual prompt policy remain correct when moving beyond English-only checkpoints;
- consider upstreaming or filing a focused issue against the Burn PyTorch import behavior with a minimal non-contiguous tensor reproduction.

## Summary Of Lessons

- A model can have correct shapes and still be wrong because tensor layout was imported incorrectly.
- Non-contiguous PyTorch views are dangerous at framework boundaries.
- OpenAI Whisper's English-only models use a different prompt shape than multilingual models.
- First-step logits are a much better diagnostic than final text alone.
- Frontend parity, prompt parity, and backend parity are necessary but independent checks.
- Repeated-token collapse is often a symptom, not the disease.
- Runtime compensation for bad weights is the wrong place to fix conversion corruption.
- Conversion should fail loudly on tensor import errors.
- Good debugging tools, especially `--compare-python` and randomized `--demo`, are worth keeping after the immediate bug is gone.