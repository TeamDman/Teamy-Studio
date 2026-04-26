# WhisperX-Inspired Transcription Optimization Plan

## Goal

Make Teamy Studio's Rust/Burn Whisper transcription path fast enough to handle many short samples and long media files without losing observability or transcript correctness.

The target is not to copy WhisperX line for line. The target is to identify the optimizations that make WhisperX/faster-whisper feel fast, adopt the ones that fit Teamy's Rust-owned architecture, and keep a resumable trail so future work can continue without reconstructing the whole investigation from chat history.

## Current Status

- Done so far:
  - Added a Burn Whisper transcription CLI path: `audio transcribe <path>`.
  - Added managed model cache commands: `audio model list`, `audio model prepare`, and `audio model show`.
  - Fixed the OpenAI checkpoint conversion bug by normalizing floating tensors to contiguous fp32 before Burn import.
  - Added `--compare-python` for OpenAI Whisper CUDA reference logits and transcript comparison.
  - Added reusable Burn model loading through `LoadedWhisperGreedyDecoder`, so demo batches do not reload the model for every sample.
  - Added `audio transcribe --demo <count>` for multi-sample throughput smoke tests.
  - Fixed long-input truncation by splitting validated audio into 30-second Whisper windows instead of sending the whole file through a frontend that trims to one window.
  - Moved demo and long-input progress metrics to tracing/stderr so stdout remains transcript-only.
  - Committed the completed diagnostic/model-list work as `f3c6f54 audio: improve whisper model diagnostics`.
  - Committed long-input chunking as `ef10da6 audio: chunk long whisper inputs`.
  - Last full validation after chunking passed with `./check-all.ps1`: main test run `323 passed; 0 failed; 5 ignored`, audio Tracey coverage `53 of 53`.
  - Created this resumable optimization plan under `docs/notes/whisperx-optimization-plan.md`.
  - Extended `docs/spec/product/audio-input.md` with requirements for long-input chunking, tracing-based progress diagnostics, and chunk-level stage timing.
  - Implemented the first telemetry slice: `GreedyDecodeSummary` now carries total, encoder, and decoder-loop elapsed milliseconds, and `audio transcribe` logs per-chunk frontend/decode/encoder/decoder timings, generated token count, tokens per second, and audio seconds per second through tracing.
  - Smoke-tested `cargo run -- audio transcribe --demo --max-decode-tokens 4`; the run emitted stage timing diagnostics and kept transcript text on stdout.
  - Validated the telemetry slice with `./check-all.ps1`: main test run `323 passed; 0 failed; 5 ignored`, audio Tracey coverage `56 of 56`.
  - Added bounded parallel decode workers for independent demo samples and long-input chunks through `audio transcribe --decode-workers <count>` with conservative automatic worker caps by model size.
  - Smoke-tested `cargo run -- audio transcribe --demo 4 --max-decode-tokens 4`; the run started four demo workers, decoded samples concurrently, and preserved transcript output order.
  - Smoke-tested a generated 65-second WAV with `cargo run -- audio transcribe target/tmp/teamy-transcribe-65s.wav --max-decode-tokens 2`; the run split the input into three chunks, processed chunks concurrently, and preserved final chunk order.
  - Re-ran full `cargo run -- audio transcribe --demo 20` on the current tree; it completed with four workers, so the earlier failing terminal state was from before the worker/refactor fix or an already-stale run.
  - Added `audio transcribe --timing-jsonl <path>` so demo batches and long inputs can write ordered per-chunk timing records without scraping tracing output or contaminating transcript stdout.
  - Replaced the fixed automatic worker cap with VRAM-aware worker selection. The current heuristic queries free GPU memory with `nvidia-smi`, estimates per-worker memory from the selected model's parameter count when known, reserves 85% of free VRAM for Teamy decode workers, and then clamps by available work. Host parallelism is logged but does not cap the automatic worker count.
  - Smoke-tested `cargo run -- audio transcribe --demo 20 --max-decode-tokens 2 --timing-jsonl target/tmp/demo20-vram-workers-parameter-estimate.jsonl`; tiny.en selected 20 workers for 20 samples from 21.18 GiB free VRAM and a 404.77 MiB estimated worker size.
  - Smoke-tested the 43-minute Clay WebM with `--max-decode-tokens 1 --timing-jsonl target/tmp/clay-vram-target-workers-smoke.jsonl`; tiny.en selected 45 workers for 87 chunks, processed every chunk, and wrote 87 ordered timing records. Observed VRAM reached about 11.5 GiB used with about 12.1 GiB remaining.
  - Fixed long-input progress so chunk metrics are emitted as work completes instead of only after the whole parallel decode finishes. Smoke-tested `cargo run -- audio transcribe "C:\Users\TeamD\AppData\Local\TeamDman\Teamy-Studio\cache\audio\prepared\How Clay's UI Layout Algorithm Works [by9lQvpvMIc].16khz-mono.wav" --max-decode-tokens 1 --timing-jsonl target\tmp\clay-live-progress-smoke.jsonl`; the run wrote 87 ordered timing records and streamed ETA, remaining bytes, remaining audio seconds, remaining chunks, and VRAM diagnostics during the run.
  - Added the first one-model batched long-input path. `LoadedWhisperGreedyDecoder::decode_batch` builds batched feature and token tensors, and long inputs now send bounded batches of chunks through one loaded Burn Whisper model instead of loading one model per chunk worker.
  - Smoke-tested `cargo run -- audio transcribe "C:\Users\TeamD\AppData\Local\TeamDman\Teamy-Studio\cache\audio\prepared\How Clay's UI Layout Algorithm Works [by9lQvpvMIc].16khz-mono.wav" --max-decode-tokens 1 --timing-jsonl target\tmp\clay-batched-smoke.jsonl`; tiny.en selected a batch size of 45 for 87 chunks, processed all chunks, and wrote 87 ordered timing records.
  - Added in-batch progress diagnostics so large full-token batches report `transcription_batch_metrics` at batch start and during greedy token decoding instead of going quiet until the whole batch returns. Smoke-tested `cargo run -- audio transcribe "C:\Users\TeamD\AppData\Local\TeamDman\Teamy-Studio\cache\audio\prepared\How Clay's UI Layout Algorithm Works [by9lQvpvMIc].16khz-mono.wav" --decode-workers 4 --max-decode-tokens 6 --timing-jsonl target\tmp\clay-batch-progress-smoke.jsonl`; the run emitted batch start, token step 1/6, token step 5/6, token step 6/6, per-chunk metrics, and 87 ordered timing records.
  - Validated the in-batch progress slice with `./check-all.ps1`: main test run `323 passed; 0 failed; 5 ignored`, audio Tracey coverage `60 of 60`.
  - Reworked long-input terminal behavior so stdout streams transcript chunks in input order as soon as preceding chunks are available, while progress uses one `transcription_progress` structure that can show decode window, token step, decoder layer, active items, throughput, remaining bytes, remaining audio seconds, remaining items, and VRAM. Detailed per-chunk stage timing remains available through debug logs and `--timing-jsonl` instead of flooding normal terminal output.
  - Compared the current Rust chunking strategy with WhisperX: WhisperX runs VAD, merges speech regions up to `chunk_size`, batches VAD-derived speech segments, decodes with faster-whisper beam/fallback/no-speech/compression options, and then aligns returned segments. Teamy's current Rust path still uses fixed disjoint 30-second windows, no timestamp tokens, no VAD, no overlap/merge policy, and greedy full-prefix decoding, so chunk boundary oddities and repetition are expected until the segmentation/decoding policy is upgraded.
  - Added the first speech-aware chunking slice. Long inputs still respect the 30-second Whisper frontend window, but chunk boundaries now prefer the quietest 50 ms frame in the last two seconds before the hard window limit. Synthetic tests cover short inputs, quiet-boundary snapping, and the invariant that no chunk exceeds the Whisper window. Smoke-tested the Clay prepared WAV with `--decode-workers 4 --max-decode-tokens 3 --timing-jsonl target\tmp\clay-speech-aware-smoke.jsonl`; the run wrote 90 ordered timing records, with early boundaries at 28.27s, 57.90s, 86.08s, and 114.40s rather than exact 30-second cuts, and redirected stdout remained transcript-only.
  - Added an estimated decode-work progress axis for batched long-input transcription. Progress now reports batch position, total work units, remaining work units, percent complete, work units per second, clock ETA, countdown, and VRAM based on `chunks * decode_limit * decoder_layers`, while byte/audio/item throughput fields are omitted from transcription progress because most visible work happens inside batched model execution. Batched decode progress is phase-tagged so logs can distinguish encoder start/finish from decoder layer/token progress instead of looking idle until a whole batch completes. Batched completion logging is collapsed to one progress update per batch so completed chunks do not flood several near-identical ETA blocks after the decoder returns.
  - Moved multi-sample `audio transcribe --demo <count>` away from per-worker model copies and onto the one-model batched decoder path. Demo clips are prepared/validated first, split into decode units, then submitted through bounded `decode_batch_with_progress` calls while preserving ordered transcript and timing output.
  - Added an incremental cached decoder path for batched Burn Whisper decode. The batch decoder pre-fills the prompt once, stores self-attention and cross-attention key/value state per decoder layer, then feeds only the newest token for each following greedy step.
  - Smoke-tested `cargo run -- audio transcribe --demo 4 --max-decode-tokens 8 --timing-jsonl target\tmp\demo4-cached-decode-smoke.jsonl`; the run produced four ordered transcript lines and four timing records without runtime errors.
  - Compared the cached batched demo output with the existing full-prefix single-file decode path on the same four prepared WAVs; all four emitted transcript lines matched exactly.
- Current focus:
  - Validate the cached batched decoder with full `./check-all.ps1`, then move from quiet-boundary snapping toward a WhisperX-like segmentation policy: VAD or stronger energy segmentation, speech-region merge up to a model window, ordered segment timestamps, and reference comparisons against WhisperX/faster-whisper.
- Remaining work:
  - Compare Rust/Burn stage timings against Python OpenAI Whisper and WhisperX/faster-whisper on the same samples.
  - Measure cached batched decoder performance against the earlier full-prefix batched decode on longer decode limits and representative long media.
  - Tune true batched chunk inference for independent 30-second windows; the first implementation exists, but batch sizing, feature preparation, and progress timing still need measured improvement.
  - Investigate fp16 or mixed precision inference in Burn/CubeCL for CUDA.
  - Investigate replacing or augmenting the Burn path with faster-whisper/CTranslate2 for the Python daemon path.
  - Add VAD or silence skipping so long media does not spend full decode effort on empty chunks.
  - Add accuracy/regression fixtures for long media transcript quality, including repetition/non-sequitur detection against reference transcripts where available.
- Next step:
  - Add a benchmark/reference harness that records the new stage timings for Rust/Burn and comparable timings for Python OpenAI Whisper and WhisperX/faster-whisper.

## Constraints And Assumptions

- Teamy Studio uses Tracey; observable behavior changes belong in `docs/spec/product/audio-input.md` unless a new behavior area becomes distinct enough for its own spec.
- The current CLI subcommand structure should stay in the existing `audio transcribe` module; no new subcommand is needed for timing instrumentation.
- `./check-all.ps1` is the required validation command.
- stdout from transcription commands should stay transcript-only so shell redirection can capture clean text.
- progress, probe, and throughput diagnostics should use tracing, which is already configured to write logs to stderr.
- Current Rust/Burn inference can run independent demo samples through one loaded batched decoder. Long-input chunks now use one loaded decoder and batched feature/token tensors, and the batched decoder reuses decoder key/value cache state between greedy token steps. It still needs batch-size tuning and broader timing comparison.
- The older worker approach remains a useful baseline for demo samples and timing comparison, but long-input optimization should move toward one scheduler that accepts many prepared 16 kHz mono slices, chunks them internally, batches work against one model, and returns ordered rough chunk timestamps.
- Whisper chunks are currently bounded to the 30-second frontend contract and use a cheap quiet-boundary snap near the hard limit, but they are not yet true VAD-derived speech segments.
- The first optimization target is `tiny.en`, but the design must not hard-code assumptions that prevent larger or multilingual models later.
- The current greedy decoder does not implement timestamp recovery, word-level alignment, beam search, or VAD. Batched inference and decoder KV caching exist for long-input chunks, but they are first slices rather than a tuned final scheduler.

## Product Requirements

- `audio transcribe <path>` must process every chunk of a long media file rather than silently trimming to the first 30 seconds.
- `audio transcribe <path> > transcript.txt` must write only transcript text to stdout.
- Transcription diagnostics must include enough information to understand throughput on both many short clips and long media files.
- Benchmark diagnostics must be available in a machine-readable sidecar form so throughput comparisons do not depend on human tracing log text.
- Demo batching must keep the model loaded while processing multiple samples.
- Long media transcription must expose file size, duration, chunk count, chunk progress, remaining work, and GPU memory when available.
- Long media transcription should accept many 16 kHz mono slices at the scheduling boundary, divide them into Whisper windows internally, and return ordered text segments with rough input/chunk start and end seconds.
- Long media transcription should avoid treating fixed 30-second boundaries as semantic segment boundaries; the next segmentation slice should use speech activity and chunk merging so word/phrase boundaries are less likely to be cut arbitrarily.
- Future optimization work must preserve correctness checks against the Python/OpenAI Whisper reference path.

## Architectural Direction

The path to WhisperX-like performance is a sequence of measured improvements:

1. Make the current Rust/Burn pipeline observable enough to find the real bottleneck.
2. Keep the model loaded across all chunks and samples.
3. Avoid recomputing decoder state for the full token prefix on every generated token.
4. Batch independent 30-second windows when the backend can actually exploit the batch.
5. Reduce unnecessary work by skipping silence or low-speech chunks before decode.
6. Use lower precision or an optimized backend where it does not break parity.
7. Preserve a Python daemon route for WhisperX/faster-whisper features that are not worth rebuilding immediately in Burn.

Whisper is transformer-based, not LSTM-based. Independent audio windows can be processed in parallel or batched once they have been segmented. The decoder's autoregressive tokens within one chunk remain sequential unless the decoding algorithm changes, but decoder KV caching can make each sequential step much cheaper.

## Tracey Specification Strategy

This is a narrow extension of the existing audio transcription behavior, so the plan should extend `docs/spec/product/audio-input.md` instead of creating a dedicated Tracey spec.

Baseline from the latest validation before this plan:

```text
teamy-studio-audio-input/rust: 53 of 53 requirements are covered. 32 of 53 have a verification reference.
teamy-studio-cli/rust: 44 of 44 requirements are covered. 26 of 44 have a verification reference.
```

Tracey workflow for this plan:

```powershell
tracey query status
tracey query uncovered
tracey query unmapped --path src/cli/audio/transcribe/audio_transcribe_cli.rs
tracey query validate --deny warnings
tracey query untested
```

Spec work to include with implementation:

- add requirements that long inputs are chunked rather than trimmed;
- add requirements that progress diagnostics go through tracing/stderr while transcript text remains on stdout;
- add requirements for stage timing telemetry once the timing fields are stable;
- add requirements for bounded decode workers that preserve transcript output order;
- add requirements for VRAM-aware automatic worker selection;
- add requirements for ordered timing JSONL output when benchmark sidecars are introduced;
- map touched implementation with `audio[...]` references as each behavior lands.

## Phased Task Breakdown

### Phase 1: Timing And Bottleneck Telemetry

Objective: Make every transcription chunk explain where time went.

Tasks:

- Add per-chunk timing fields for frontend feature extraction, total decode, encoder forward, decoder loop, generated tokens, and tokens per second.
- Emit these timings through tracing at info level for long-input chunks and debug level for lower-level decoder internals.
- Keep stdout transcript-only.
- Add or update Tracey requirements for long-input chunking and tracing diagnostics.

Definition of done:

- Running a long input reports chunk count and per-chunk stage timings on stderr.
- Redirecting stdout captures transcript text only.
- `./check-all.ps1` passes.

### Phase 2: Reference Benchmark Harness

Objective: Compare Teamy's Rust/Burn path against Python OpenAI Whisper and WhisperX/faster-whisper with the same inputs.

Tasks:

- Add a benchmark-style command or diagnostic mode for a fixed VCTK sample set and a long-media slice.
- Capture model load time, feature time, encode time, decode time, tokens per second, audio seconds per second, and VRAM.
- Use `--timing-jsonl <path>` as the first Rust/Burn benchmark sidecar before deciding whether a dedicated benchmark subcommand is needed.
- Run OpenAI Whisper and WhisperX/faster-whisper references through `uv` where available.
- Save or print structured JSONL/trace output that can be compared across runs.

Definition of done:

- One command can produce comparable timing summaries for Rust/Burn and Python reference backends.
- The output identifies whether decoder token loop, frontend, model load, or audio prep is the primary bottleneck.

### Phase 3: Decoder KV Cache Or Incremental Decode

Objective: Avoid re-running the decoder over the full growing token prefix for every generated token.

Tasks:

- Read Burn attention APIs and current `WhisperTextDecoder` structure.
- Design a cache representation for self-attention keys/values per decoder layer.
- Add an incremental decoder path that consumes only the newest token after the prompt prefill.
- Validate first-step and multi-token parity against the existing full-prefix decoder.

Definition of done:

- Cached decode produces the same token ids as full-prefix greedy decode on fixed samples.
- Per-token decoder time drops measurably in telemetry.
- Existing full-prefix path remains available as a fallback until cached decode is trusted.

### Phase 4: Batched Independent Chunks

Objective: Replace multi-worker model-copy parallelism with one loaded model that processes multiple 30-second windows together when the backend can exploit batch parallelism.

Tasks:

- Treat the current VRAM-aware worker path as a baseline to beat, not the final architecture.
- Change frontend/request shapes to represent a batch of log-mel tensors.
- Add batched encoder forward for multiple chunks.
- Decide whether decoder batching is viable with variable-length token generation.
- Start with batch size controls and conservative defaults.

Definition of done:

- Multi-chunk long inputs can encode multiple chunks in one backend call.
- Telemetry reports batch size and effective audio seconds per second.
- Accuracy remains comparable to single-chunk sequential decode.

### Phase 5: Silence Skipping And VAD Direction

Objective: Stop spending full decode effort on chunks without speech.

Tasks:

- Add cheap RMS/energy summaries per chunk.
- Compare simple energy gates with Whisper no-speech behavior.
- Decide whether to adopt a real VAD dependency or keep a simple local gate.
- Preserve timestamps/chunk offsets so skipped regions do not confuse transcript ordering.

Definition of done:

- Long quiet media avoids unnecessary decode work.
- Progress diagnostics clearly report skipped chunks and why they were skipped.
- Speech-heavy files are not harmed by false skipping in the tested sample set.

### Phase 6: Precision And Backend Strategy

Objective: Decide how far Teamy's Burn path should go versus delegating fast inference to the Python daemon.

Tasks:

- Investigate Burn/CubeCL fp16 support for the current model path.
- Measure fp32 versus fp16 or mixed precision when available.
- Compare against faster-whisper/CTranslate2 for the same model and files.
- Decide whether Teamy's product default should be Burn, Python faster-whisper, or a selectable backend.

Definition of done:

- The project has measured data for Burn fp32, any available Burn fp16 path, OpenAI Whisper Python, and faster-whisper/WhisperX.
- The default backend choice is documented and reflected in CLI/daemon help.

## Recommended Implementation Order

1. Finish Phase 1 timing telemetry.
2. Add a small benchmark/reference harness before changing algorithms.
3. Implement decoder KV caching if telemetry confirms decoder token loop dominates.
4. Add batched encoder/chunk processing once single-chunk timing is understood.
5. Add silence skipping after chunk and timing behavior are stable.
6. Revisit backend defaults after measuring faster-whisper and Burn precision options.

## Open Decisions

- Should long-media transcription eventually print chunk separators or timestamps, or should stdout stay plain joined transcript text by default?
- Should the Rust/Burn path remain the default if faster-whisper is dramatically faster, or should Teamy default to Python daemon inference when configured?
- Should `audio transcribe --demo <count>` support parallel workers, or should parallelism be reserved for backend-level batching?
- Should VAD be an explicit opt-in mode until false-skip risk is measured?
- How much timestamp/alignment behavior do we want before adopting WhisperX word alignment?

## First Concrete Slice

Phase 1 timing telemetry has started and the first vertical slice is implemented:

- `GreedyDecodeSummary` carries total, encoder, and decoder-loop timings.
- `audio_transcribe_cli.rs` emits frontend, decode, encoder, decoder, generated-token, tokens-per-second, and audio-seconds-per-second diagnostics per chunk.
- Tracey requirements for long-input chunking, tracing diagnostics, and stage timing are added and mapped.
- `./check-all.ps1` passed after the implementation.

The next concrete slice is Phase 2: build a small reference benchmark harness so the new timing fields can be compared against Python OpenAI Whisper and WhisperX/faster-whisper on the same VCTK and long-media samples.