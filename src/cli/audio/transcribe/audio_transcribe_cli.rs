use arbitrary::Arbitrary;
use eyre::{Context, bail};
use facet::Facet;
use figue as args;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::{Write, stdout};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::cli::output::CliOutput;

/// Transcribe a WAV file with the Rust Burn Whisper backend.
// audio[impl cli.transcribe-command]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "CLI flags map directly to switches"
)]
#[facet(rename_all = "kebab-case")]
pub struct AudioTranscribeArgs {
    /// Audio file to transcribe.
    #[facet(args::positional)]
    pub input: Option<String>,

    /// Find a local VCTK clip and run it as a transcription demo.
    // audio[impl cli.transcribe-demo]
    #[facet(args::named, default)]
    pub demo: bool,

    /// Managed model name under `{cache_home}/models/<model>`.
    #[facet(args::named, default = crate::model::DEFAULT_TRANSCRIPTION_MODEL_NAME.to_owned())]
    pub model: String,

    /// Print a Python `OpenAI` Whisper CUDA reference transcript and first-step logits.
    #[facet(args::named, default)]
    pub compare_python: bool,

    /// Converted Whisper model directory containing tokenizer.json, model.bpk, and dims.json.
    ///
    /// If omitted, Teamy Studio uses the first path in the app-home model registry.
    #[facet(args::named)]
    pub model_dir: Option<String>,

    /// Explicitly create a 16 kHz mono PCM WAV artifact first.
    #[facet(args::named, default)]
    pub resample: bool,

    /// Override the output path used when `--resample` is enabled.
    #[facet(args::named)]
    pub prepared_output: Option<String>,

    /// Maximum number of decoder tokens to greedily generate.
    #[facet(args::named)]
    pub max_decode_tokens: Option<usize>,

    /// Replace the prepared output if it already exists.
    #[facet(args::named, default)]
    pub overwrite: bool,

    /// Number of parallel Burn decoder workers for demo samples, or long-input chunk batch size.
    // audio[impl cli.transcribe-decode-workers]
    #[facet(args::named)]
    pub decode_workers: Option<usize>,

    /// Write ordered per-chunk timing diagnostics as JSON Lines to this file.
    // audio[impl cli.transcribe-timing-jsonl]
    #[facet(args::named)]
    pub timing_jsonl: Option<String>,
}

impl AudioTranscribeArgs {
    /// # Errors
    ///
    /// This function will return an error if audio validation, model loading, feature extraction,
    /// or Burn Whisper decoding fails.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        let _span = tracing::info_span!(
            "audio_transcribe_command",
            demo = self.demo,
            model = %self.model,
        )
        .entered();
        let command_started_at = Instant::now();
        tracing::debug!(demo = self.demo, model = %self.model, ?self.model_dir, "Starting audio transcribe command");
        let max_decode_tokens = self
            .max_decode_tokens
            .unwrap_or(crate::whisper::DEFAULT_MAX_DECODE_TOKENS);
        if self.demo {
            let demo_count = parse_demo_sample_count(self.input.as_deref())?;
            return self.invoke_demo_batch(
                app_home,
                cache_home,
                demo_count,
                max_decode_tokens,
                command_started_at,
            );
        }

        let input_path = PathBuf::from(self.input.as_deref().ok_or_else(|| {
            eyre::eyre!("audio transcribe requires an INPUT path unless --demo is set")
        })?);
        tracing::debug!(path = %input_path.display(), "Inspecting transcription input audio");
        let metadata = crate::audio::inspect_audio(&input_path)?;

        if self.compare_python {
            print_python_reference_comparison(&input_path, &self.model)?;
        }

        let explicit_model_dir = self.model_dir.as_deref().map(PathBuf::from);
        let model_dir = tracing::info_span!("resolve_transcription_model").in_scope(|| {
            crate::model::resolve_transcription_model_dir(
                app_home,
                cache_home,
                Some(&self.model),
                explicit_model_dir.as_deref(),
            )
        })?;
        tracing::debug!(model_dir = %model_dir.display(), "Resolved Burn Whisper model directory");
        let model_load_started_at = Instant::now();
        let model = tracing::info_span!("inspect_transcription_model").in_scope(|| {
            crate::model::inspect_model_dir(&model_dir).wrap_err_with(|| {
                format!(
                    "failed to load Burn Whisper model from {}. Run `teamy-studio audio model prepare {}` first, or pass --model-dir <dir>.",
                    model_dir.display(),
                    self.model,
                )
            })
        })?;
        tracing::debug!(elapsed_ms = model_load_started_at.elapsed().as_millis(), layout = ?model.layout, "Inspected Burn Whisper model directory");
        let decode_workers = self.effective_decode_workers(model.dims.as_ref(), usize::MAX)?;
        let decoder = tracing::info_span!("load_transcription_decoder").in_scope(|| {
            crate::whisper::LoadedWhisperGreedyDecoder::load(model.clone(), max_decode_tokens)
        })?;
        let result = transcribe_one_input(
            &input_path,
            metadata,
            TranscribeOneContext {
                cache_home,
                args: &self,
                decoder: &decoder,
                is_demo: false,
                max_decode_tokens,
                decode_workers,
            },
        )?;
        tracing::debug!(
            elapsed_ms = command_started_at.elapsed().as_millis(),
            "Finished audio transcribe command"
        );

        self.write_timing_jsonl(&result.timing_records)?;
        if !result.streamed_stdout {
            println!("{}", result.text);
        }
        Ok(CliOutput::none())
    }

    fn invoke_demo_batch(
        &self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
        demo_count: usize,
        max_decode_tokens: usize,
        command_started_at: Instant,
    ) -> eyre::Result<CliOutput> {
        let _span = tracing::info_span!(
            "audio_transcribe_demo_batch",
            demo_count,
            model = %self.model,
        )
        .entered();
        if demo_count > 1 && self.prepared_output.is_some() {
            bail!("--prepared-output cannot be used with --demo counts greater than 1");
        }
        tracing::debug!(demo_count, "Searching for VCTK demo clips");
        let clips = find_vctk_demo_clips(demo_count)?;
        let items = clips
            .into_iter()
            .map(DemoBatchItem::inspect)
            .collect::<eyre::Result<Vec<_>>>()?;

        let explicit_model_dir = self.model_dir.as_deref().map(PathBuf::from);
        let model_dir = tracing::info_span!("resolve_transcription_model").in_scope(|| {
            crate::model::resolve_transcription_model_dir(
                app_home,
                cache_home,
                Some(&self.model),
                explicit_model_dir.as_deref(),
            )
        })?;
        tracing::debug!(model_dir = %model_dir.display(), "Resolved Burn Whisper model directory for demo batch");
        let model = tracing::info_span!("inspect_transcription_model").in_scope(|| {
            crate::model::inspect_model_dir(&model_dir).wrap_err_with(|| {
                format!(
                    "failed to load Burn Whisper model from {}. Run `teamy-studio audio model prepare {}` first, or pass --model-dir <dir>.",
                    model_dir.display(),
                    self.model,
                )
            })
        })?;
        let decode_workers = self.effective_decode_workers(model.dims.as_ref(), items.len())?;
        let decoder = tracing::info_span!("load_transcription_decoder").in_scope(|| {
            crate::whisper::LoadedWhisperGreedyDecoder::load(model.clone(), max_decode_tokens)
        })?;
        if demo_count > 1 {
            tracing::info!(
                samples = items.len(),
                "Starting audio transcription demo batch"
            );
        }

        let results = if items.len() > 1 {
            transcribe_demo_items_batched(
                items,
                cache_home,
                self,
                &decoder,
                decode_workers,
                max_decode_tokens,
            )?
        } else {
            let mut results = Vec::new();
            for item in items {
                tracing::info!(path = %item.clip.wav_path.display(), expected_text = %item.clip.expected_text, "Starting demo transcription sample");
                if self.compare_python {
                    print_python_reference_comparison(&item.clip.wav_path, &self.model)?;
                }
                let result = transcribe_one_input(
                    &item.clip.wav_path,
                    item.metadata,
                    TranscribeOneContext {
                        cache_home,
                        args: self,
                        decoder: &decoder,
                        is_demo: true,
                        max_decode_tokens,
                        decode_workers: 1,
                    },
                )?;
                results.push((item.clip, result));
            }
            results
        };

        let mut timing_records = Vec::new();
        for (_clip, result) in results {
            println!("{}", result.text);
            timing_records.extend(result.timing_records.iter().cloned());
        }

        tracing::debug!(
            elapsed_ms = command_started_at.elapsed().as_millis(),
            "Finished audio transcribe demo batch"
        );
        self.write_timing_jsonl(&timing_records)?;
        Ok(CliOutput::none())
    }

    fn write_timing_jsonl(&self, records: &[TranscriptionTimingRecord]) -> eyre::Result<()> {
        let Some(path) = self.timing_jsonl.as_deref() else {
            return Ok(());
        };
        write_timing_jsonl(Path::new(path), records)
    }

    fn effective_decode_workers(
        &self,
        dims: Option<&crate::whisper::WhisperDims>,
        work_items: usize,
    ) -> eyre::Result<usize> {
        let requested = self.decode_workers.unwrap_or_else(|| {
            // audio[impl cli.transcribe-decode-workers]
            let host_parallelism =
                std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
            let vram_cap = automatic_worker_cap_from_vram(&self.model, dims).unwrap_or(1);
            let workers = vram_cap.min(work_items.max(1));
            tracing::info!(
                workers,
                host_parallelism,
                vram_cap,
                model = %self.model,
                "Selected automatic Burn Whisper decode parallelism"
            );
            workers
        });
        if requested == 0 {
            bail!("--decode-workers must be at least 1");
        }
        Ok(requested.min(work_items.max(1)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct VctkDemoClip {
    wav_path: PathBuf,
    expected_text: String,
}

#[derive(Clone, Debug, PartialEq)]
struct DemoBatchItem {
    clip: VctkDemoClip,
    metadata: crate::audio::AudioMetadata,
    bytes: u64,
    audio_seconds: f64,
}

struct DemoPreparedInput {
    clip: VctkDemoClip,
    audio: crate::transcription::ValidatedAudio,
    bytes: u64,
    audio_seconds: f64,
    chunk_boundaries: Vec<(usize, usize)>,
}

#[derive(Clone, Copy)]
struct DemoDecodeUnit<'a> {
    input_index: usize,
    global_index: usize,
    global_total: usize,
    chunk: AudioChunk<'a>,
}

impl DemoBatchItem {
    fn inspect(clip: VctkDemoClip) -> eyre::Result<Self> {
        let metadata = crate::audio::inspect_audio(&clip.wav_path)?;
        let bytes = std::fs::metadata(&clip.wav_path)
            .wrap_err_with(|| format!("failed to stat {}", clip.wav_path.display()))?
            .len();
        let audio_seconds = metadata.duration_seconds.unwrap_or(0.0);
        Ok(Self {
            clip,
            metadata,
            bytes,
            audio_seconds,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct DemoBatchTotals {
    items: usize,
    bytes: u64,
    audio_seconds: f64,
}

#[derive(Clone, Debug, PartialEq)]
struct TranscribedInput {
    text: String,
    bytes: u64,
    audio_seconds: f64,
    words: usize,
    timing_records: Vec<TranscriptionTimingRecord>,
    streamed_stdout: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct TranscriptionChunkResult {
    text: String,
    audio_seconds: f64,
    words: usize,
    timing_record: TranscriptionTimingRecord,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct TranscriptionTimingRecord {
    schema: &'static str,
    backend: &'static str,
    input_path: String,
    chunk_index: usize,
    chunk_total: usize,
    start_seconds: f64,
    end_seconds: f64,
    audio_seconds: f64,
    frontend_elapsed_ms: u128,
    decode_elapsed_ms: u128,
    decoder_total_elapsed_ms: u128,
    encoder_elapsed_ms: u128,
    decoder_elapsed_ms: u128,
    generated_tokens: usize,
    tokens_per_second: f64,
    audio_seconds_per_second: f64,
    stop_reason: String,
    words: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct AudioChunk<'a> {
    index: usize,
    total: usize,
    total_samples: usize,
    start_sample: usize,
    end_sample: usize,
    samples: &'a [f32],
}

struct ChunkProgressContext<'a> {
    input_path: &'a Path,
    bytes: u64,
    progress: &'a mut DemoBatchProgress,
    transcript_streamer: Option<&'a mut OrderedTranscriptStreamer>,
}

struct OrderedTranscriptStreamer {
    next_chunk_index: usize,
    pending: BTreeMap<usize, String>,
    wrote_anything: bool,
}

impl OrderedTranscriptStreamer {
    fn new() -> Self {
        Self {
            next_chunk_index: 1,
            pending: BTreeMap::new(),
            wrote_anything: false,
        }
    }

    fn push(&mut self, chunk_index: usize, text: &str) -> eyre::Result<()> {
        // audio[impl cli.transcribe-streaming-stdout]
        self.pending.insert(chunk_index, text.trim().to_owned());
        self.flush_ready()
    }

    fn finish(&mut self) -> eyre::Result<()> {
        if self.wrote_anything {
            println!();
            stdout()
                .flush()
                .wrap_err("failed to flush transcript stdout")?;
        }
        Ok(())
    }

    fn flush_ready(&mut self) -> eyre::Result<()> {
        let mut output = stdout().lock();
        while let Some(text) = self.pending.remove(&self.next_chunk_index) {
            if !text.is_empty() {
                if self.wrote_anything {
                    write!(output, " ").wrap_err("failed to write transcript separator")?;
                }
                write!(output, "{text}").wrap_err("failed to write transcript chunk")?;
                self.wrote_anything = true;
            }
            self.next_chunk_index = self.next_chunk_index.saturating_add(1);
        }
        output.flush().wrap_err("failed to flush transcript stdout")
    }
}

#[derive(Clone, Debug)]
struct DemoBatchProgress {
    totals: DemoBatchTotals,
    started_at: Instant,
    processed_items: usize,
    processed_bytes: u64,
    processed_audio_seconds: f64,
    processed_words: usize,
    estimated_decode_work_units_per_item: Option<u64>,
    estimated_decode_work_total_units: Option<u64>,
    estimated_decode_work_completed_units: u64,
}

impl DemoBatchProgress {
    fn new(totals: DemoBatchTotals) -> Self {
        Self {
            totals,
            started_at: Instant::now(),
            processed_items: 0,
            processed_bytes: 0,
            processed_audio_seconds: 0.0,
            processed_words: 0,
            estimated_decode_work_units_per_item: None,
            estimated_decode_work_total_units: None,
            estimated_decode_work_completed_units: 0,
        }
    }

    fn configure_estimated_decode_work(&mut self, items: usize, units_per_item: u64) {
        if items == 0 || units_per_item == 0 {
            return;
        }
        self.estimated_decode_work_units_per_item = Some(units_per_item);
        self.estimated_decode_work_total_units =
            Some(units_per_item.saturating_mul(usize_to_u64_saturating(items)));
    }

    fn estimated_decode_work_units_per_item(&self) -> Option<u64> {
        self.estimated_decode_work_units_per_item
    }

    fn record_estimated_decode_work(&mut self, completed_units: u64) {
        let completed_units = self
            .estimated_decode_work_total_units
            .map_or(completed_units, |total| completed_units.min(total));
        self.estimated_decode_work_completed_units = self
            .estimated_decode_work_completed_units
            .max(completed_units);
    }

    fn record_values(&mut self, bytes: u64, audio_seconds: f64, words: usize) {
        self.processed_items = self.processed_items.saturating_add(1);
        self.processed_bytes = self.processed_bytes.saturating_add(bytes);
        self.processed_audio_seconds += audio_seconds;
        self.processed_words = self.processed_words.saturating_add(words);
        if let Some(units_per_item) = self.estimated_decode_work_units_per_item {
            self.record_estimated_decode_work(
                units_per_item.saturating_mul(usize_to_u64_saturating(self.processed_items)),
            );
        }
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "demo progress metrics are human-scale floating point rates"
    )]
    fn render(
        &self,
        header: &str,
        current_index: usize,
        current_label: &str,
        gpu: Option<GpuMemorySnapshot>,
        detail_lines: &[String],
    ) -> String {
        let elapsed = self.started_at.elapsed().as_secs_f64().max(f64::EPSILON);
        let (vram_used, vram_remaining) = gpu.map_or_else(
            || ("unknown".to_owned(), "unknown".to_owned()),
            |snapshot| {
                (
                    human_bytes(snapshot.used_bytes),
                    human_bytes(snapshot.free_bytes),
                )
            },
        );

        if let Some(total_units) = self.estimated_decode_work_total_units {
            // audio[impl cli.transcribe-estimated-decode-work-progress]
            let completed_units = self.estimated_decode_work_completed_units.min(total_units);
            let remaining_units = total_units.saturating_sub(completed_units);
            let units_per_second = completed_units as f64 / elapsed;
            let percent = if total_units == 0 {
                100.0
            } else {
                completed_units as f64 * 100.0 / total_units as f64
            };
            let mut lines = vec![format!("{header}:")];
            if detail_lines.is_empty() {
                lines.push(format!("  item: {current_index}/{}", self.totals.items));
            } else {
                lines.extend(detail_lines.iter().map(|detail| format!("  {detail}")));
            }
            lines.extend([
                format!("  current: {current_label}"),
                format!("  work units total: {total_units}"),
                format!("  work units remaining: {remaining_units}"),
                format!("  work percent complete: {percent:.2}%"),
                format!("  work units per second: {units_per_second:.0}"),
                format!(
                    "  work eta: {}",
                    format_clock_eta(remaining_units as f64, units_per_second)
                ),
                format!(
                    "  work countdown: {}",
                    format_eta(remaining_units as f64, units_per_second)
                ),
                format!("  vram_used: {vram_used}"),
                format!("  vram_remaining: {vram_remaining}"),
            ]);
            return lines.join("\n");
        }

        let bytes_per_second = self.processed_bytes as f64 / elapsed;
        let audio_seconds_per_second = self.processed_audio_seconds / elapsed;
        let items_per_second = self.processed_items as f64 / elapsed;
        let words_per_second = self.processed_words as f64 / elapsed;
        let bytes_remaining = self.totals.bytes.saturating_sub(self.processed_bytes);
        let items_remaining = self.totals.items.saturating_sub(self.processed_items);
        let audio_seconds_remaining =
            (self.totals.audio_seconds - self.processed_audio_seconds).max(0.0);

        let mut lines = vec![
            format!("{header}:"),
            format!("  item: {current_index}/{}", self.totals.items),
            format!("  current: {current_label}"),
            format!(
                "  bytes_per_second: {}/s",
                human_bytes_f64(bytes_per_second)
            ),
            format!("  audio_seconds_per_second: {audio_seconds_per_second:.3}"),
            format!("  items_per_second: {items_per_second:.3}"),
            format!("  words_per_second: {words_per_second:.3}"),
            format!(
                "  eta_by_bytes: {}",
                format_eta(bytes_remaining as f64, bytes_per_second)
            ),
            format!(
                "  eta_by_audio_seconds: {}",
                format_eta(audio_seconds_remaining, audio_seconds_per_second)
            ),
            format!(
                "  eta_by_items: {}",
                format_eta(items_remaining as f64, items_per_second)
            ),
            format!("  bytes_remaining: {}", human_bytes(bytes_remaining)),
            format!("  audio_seconds_remaining: {audio_seconds_remaining:.3}"),
            format!("  items_remaining: {items_remaining}"),
        ];
        lines.extend([
            format!("  vram_used: {vram_used}"),
            format!("  vram_remaining: {vram_remaining}"),
        ]);
        lines.join("\n")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GpuMemorySnapshot {
    used_bytes: u64,
    free_bytes: u64,
}

fn parse_demo_sample_count(input: Option<&str>) -> eyre::Result<usize> {
    let Some(raw) = input else {
        return Ok(1);
    };
    let count = raw.parse::<usize>().wrap_err_with(|| {
        format!(
            "when --demo is set, positional INPUT must be a positive sample count, found {raw:?}"
        )
    })?;
    if count == 0 {
        bail!("--demo sample count must be at least 1");
    }
    Ok(count)
}

#[derive(Clone, Copy)]
struct TranscribeOneContext<'a> {
    cache_home: &'a crate::paths::CacheHome,
    args: &'a AudioTranscribeArgs,
    decoder: &'a crate::whisper::LoadedWhisperGreedyDecoder,
    is_demo: bool,
    max_decode_tokens: usize,
    decode_workers: usize,
}

#[expect(
    clippy::too_many_lines,
    reason = "input transcription coordinates audio loading, chunking, progress, streaming, and timing"
)]
fn transcribe_one_input(
    input_path: &Path,
    metadata: crate::audio::AudioMetadata,
    context: TranscribeOneContext<'_>,
) -> eyre::Result<TranscribedInput> {
    let _span = tracing::info_span!(
        "transcribe_audio_input",
        is_demo = context.is_demo,
        decode_workers = context.decode_workers,
    )
    .entered();
    let bytes = std::fs::metadata(input_path)
        .wrap_err_with(|| format!("failed to stat {}", input_path.display()))?
        .len();
    let original_duration_seconds = metadata.duration_seconds;
    let effective_audio = tracing::info_span!("load_transcription_audio")
        .in_scope(|| load_effective_transcription_audio(input_path, metadata, &context))?;

    let audio_seconds =
        original_duration_seconds.unwrap_or_else(|| effective_audio.duration_seconds());
    // audio[impl cli.transcribe-long-input-chunks]
    let chunks = tracing::info_span!("split_transcription_audio")
        .in_scope(|| audio_chunks(&effective_audio.samples));
    // audio[impl cli.transcribe-progress-tracing]
    tracing::info!(
        path = %input_path.display(),
        bytes = %human_bytes(bytes),
        duration = %format_audio_duration(audio_seconds),
        chunks = chunks.len(),
        sample_rate_hz = ?effective_audio.metadata.sample_rate_hz,
        channels = ?effective_audio.metadata.channels,
        codec = %effective_audio.metadata.display_codec(),
        container = %effective_audio.metadata.display_container(),
        "Prepared audio transcription input"
    );

    let mut progress = DemoBatchProgress::new(DemoBatchTotals {
        items: chunks.len(),
        bytes,
        audio_seconds,
    });
    progress.configure_estimated_decode_work(
        chunks.len(),
        estimated_decode_work_units_per_item(context.decoder),
    );
    let mut transcript_streamer = (!context.is_demo).then(OrderedTranscriptStreamer::new);
    let chunk_count = chunks.len();
    let chunk_results = if context.decode_workers > 1 && chunk_count > 1 {
        tracing::info_span!("transcribe_audio_chunks_batched", chunks = chunk_count).in_scope(
            || {
                transcribe_audio_chunks_batched(
                    &effective_audio,
                    &chunks,
                    context.decoder,
                    context.decode_workers,
                    ChunkProgressContext {
                        input_path,
                        bytes,
                        progress: &mut progress,
                        transcript_streamer: transcript_streamer.as_mut(),
                    },
                )
            },
        )?
    } else {
        let mut chunk_results = Vec::with_capacity(chunks.len());
        for chunk in &chunks {
            #[cfg(feature = "tracy")]
            let _span = tracing::debug_span!("transcribe_audio_chunk").entered();
            let chunk_result = transcribe_audio_chunk(
                &effective_audio,
                *chunk,
                context.decoder,
                context.max_decode_tokens,
            )?;
            record_completed_chunk(
                input_path,
                *chunk,
                &chunk_result,
                &mut progress,
                bytes,
                transcript_streamer.as_mut(),
                true,
            )?;
            chunk_results.push(chunk_result);
        }
        chunk_results
    };
    if let Some(streamer) = transcript_streamer.as_mut() {
        streamer.finish()?;
    }

    let text = chunk_results
        .iter()
        .map(|chunk| chunk.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let words = chunk_results.iter().map(|chunk| chunk.words).sum();
    let timing_records = chunk_results
        .iter()
        .map(|chunk| chunk.timing_record.clone())
        .collect();
    Ok(TranscribedInput {
        text,
        bytes,
        audio_seconds,
        words,
        timing_records,
        streamed_stdout: transcript_streamer.is_some(),
    })
}

fn record_completed_chunk(
    input_path: &Path,
    chunk: AudioChunk<'_>,
    result: &TranscriptionChunkResult,
    progress: &mut DemoBatchProgress,
    bytes: u64,
    transcript_streamer: Option<&mut OrderedTranscriptStreamer>,
    log_progress: bool,
) -> eyre::Result<()> {
    if let Some(streamer) = transcript_streamer {
        streamer.push(chunk.index, &result.text)?;
    }
    progress.record_values(
        chunk_audio_bytes(bytes, chunk),
        result.audio_seconds,
        result.words,
    );
    if log_progress {
        log_overall_progress(
            input_path,
            progress,
            &format!(
                "completed chunk {}/{} ({:.3}s..{:.3}s)",
                chunk.index,
                chunk.total,
                sample_index_seconds(chunk.start_sample),
                sample_index_seconds(chunk.end_sample)
            ),
            &[],
        );
    }
    Ok(())
}

fn load_effective_transcription_audio(
    input_path: &Path,
    metadata: crate::audio::AudioMetadata,
    context: &TranscribeOneContext<'_>,
) -> eyre::Result<crate::transcription::ValidatedAudio> {
    let issues = crate::audio::validate_for_transcription(&metadata);
    if issues.is_empty() {
        return tracing::info_span!("load_validated_transcription_audio")
            .in_scope(|| crate::transcription::load_validated_audio(input_path, metadata));
    }

    let prepared_output = context.args.prepared_output.as_deref().map_or_else(
        || default_prepared_output_path(context.cache_home, input_path),
        PathBuf::from,
    );

    let prepared = if prepared_output.exists() && !context.args.overwrite {
        tracing::info_span!("reuse_prepared_transcription_audio").in_scope(|| {
            eyre::Ok(crate::audio::PreparedAudio {
                metadata: crate::audio::inspect_audio(&prepared_output)?,
                path: prepared_output,
            })
        })?
    } else {
        tracing::info!(input = %input_path.display(), output = %prepared_output.display(), "Preparing audio for transcription");
        tracing::info_span!("prepare_transcription_audio").in_scope(|| {
            crate::audio::prepare_audio(input_path, &prepared_output, context.args.overwrite)
        })?
    };
    tracing::info_span!("load_validated_transcription_audio")
        .in_scope(|| crate::transcription::load_validated_audio(&prepared.path, prepared.metadata))
}

fn chunk_audio_bytes(total_bytes: u64, chunk: AudioChunk<'_>) -> u64 {
    let total_audio_seconds = sample_index_seconds(chunk.total_samples);
    estimate_chunk_bytes(
        total_bytes,
        total_audio_seconds,
        sample_index_seconds(chunk.samples.len()),
    )
}

fn estimated_decode_work_units_per_item(
    decoder: &crate::whisper::LoadedWhisperGreedyDecoder,
) -> u64 {
    usize_to_u64_saturating(decoder.decode_limit())
        .saturating_mul(usize_to_u64_saturating(decoder.decoder_layer_count()))
}

fn usize_to_u64_saturating(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn transcribe_audio_chunks_batched(
    input: &crate::transcription::ValidatedAudio,
    chunks: &[AudioChunk<'_>],
    decoder: &crate::whisper::LoadedWhisperGreedyDecoder,
    batch_size: usize,
    progress_context: ChunkProgressContext<'_>,
) -> eyre::Result<Vec<TranscriptionChunkResult>> {
    // audio[impl cli.transcribe-decode-workers]
    // audio[impl cli.transcribe-batched-chunks]
    let ChunkProgressContext {
        input_path,
        bytes,
        progress,
        mut transcript_streamer,
    } = progress_context;
    let batch_size = batch_size.min(chunks.len().max(1));
    tracing::info!(batch_size, chunks = chunks.len(), path = %input.path.display(), "Starting batched audio chunk transcription");
    let mut results = Vec::with_capacity(chunks.len());
    let batch_total = chunks.len().div_ceil(batch_size);
    for (batch_index, batch) in chunks.chunks(batch_size).enumerate() {
        #[cfg(feature = "tracy")]
        let _span = tracing::debug_span!("transcribe_audio_chunk_batch").entered();
        let batch_number = batch_index + 1;
        log_overall_progress(
            input_path,
            progress,
            &format!(
                "preparing chunks {}..{}",
                batch.first().map_or(0, |chunk| chunk.index),
                batch.last().map_or(0, |chunk| chunk.index)
            ),
            &[batch_progress_line(batch, batch_number, batch_total)],
        );
        let prepared_features = {
            #[cfg(feature = "tracy")]
            let _span = tracing::debug_span!("prepare_whisper_batch_features").entered();
            prepare_whisper_batch_features(batch.iter().map(|chunk| chunk.samples))?
        };
        let features = prepared_features
            .iter()
            .map(|prepared| prepared.features.clone())
            .collect::<Vec<_>>();

        let decode_started_at = Instant::now();
        let summaries = {
            #[cfg(feature = "tracy")]
            let _span = tracing::debug_span!("decode_whisper_chunk_batch").entered();
            decoder.decode_batch_with_progress(&features, |decode_progress| {
                if should_log_batch_token_progress(decode_progress) {
                    log_batch_decode_progress(
                        input_path,
                        batch,
                        progress,
                        batch_number,
                        batch_total,
                        decode_progress,
                    );
                }
            })?
        };
        let decode_elapsed_ms = decode_started_at.elapsed().as_millis();
        for ((chunk, summary), prepared_features) in
            batch.iter().copied().zip(summaries).zip(prepared_features)
        {
            let result = transcription_chunk_result_from_summary(
                input,
                chunk,
                prepared_features.elapsed_ms,
                decode_elapsed_ms,
                summary,
            );
            record_completed_chunk(
                input_path,
                chunk,
                &result,
                progress,
                bytes,
                transcript_streamer.as_deref_mut(),
                false,
            )?;
            results.push(result);
        }
        log_overall_progress(
            input_path,
            progress,
            &format!(
                "completed chunks {}..{}",
                batch.first().map_or(0, |chunk| chunk.index),
                batch.last().map_or(0, |chunk| chunk.index)
            ),
            &[batch_progress_line(batch, batch_number, batch_total)],
        );
    }
    Ok(results)
}

fn should_log_batch_token_progress(progress: crate::whisper::BatchedGreedyDecodeProgress) -> bool {
    let token_should_report = progress.token_index == 1
        || progress.token_index == progress.decode_limit
        || progress.token_index.is_multiple_of(5)
        || progress.active_items == 0;
    if !token_should_report {
        return false;
    }

    match (progress.decoder_layer_index, progress.decoder_layer_total) {
        (Some(layer_index), Some(layer_total)) => {
            if layer_total <= 4 {
                layer_index == 1 || layer_index == layer_total
            } else {
                layer_index == 1
                    || layer_index == layer_total
                    || layer_index == layer_total / 4
                    || layer_index == layer_total / 2
                    || layer_index == (layer_total * 3) / 4
            }
        }
        _ => progress.active_items == 0 || progress.token_index == progress.decode_limit,
    }
}

fn log_overall_progress(
    input_path: &Path,
    progress: &DemoBatchProgress,
    current_label: &str,
    detail_lines: &[String],
) {
    // audio[impl cli.transcribe-unified-progress]
    tracing::info!(
        path = %input_path.display(),
        "{}",
        progress.render(
            "estimated progress report",
            progress.processed_items,
            current_label,
            query_gpu_memory(),
            detail_lines,
        ),
    );
}

#[expect(
    clippy::cast_precision_loss,
    reason = "batch progress diagnostics are approximate human-scale values"
)]
fn log_batch_decode_progress(
    input_path: &Path,
    batch: &[AudioChunk<'_>],
    progress: &mut DemoBatchProgress,
    batch_number: usize,
    batch_total: usize,
    decode_progress: crate::whisper::BatchedGreedyDecodeProgress,
) {
    let (first_chunk, last_chunk) = batch_chunk_bounds(batch);
    if let Some(completed_units) = estimated_decode_work_completed_for_batch(
        batch,
        progress.estimated_decode_work_units_per_item(),
        decode_progress,
    ) {
        progress.record_estimated_decode_work(completed_units);
    }
    let token_steps_per_second = per_second(
        decode_progress.token_index as f64,
        decode_progress.elapsed_ms,
    );
    let layer = match (
        decode_progress.decoder_layer_index,
        decode_progress.decoder_layer_total,
    ) {
        (Some(index), Some(total)) => format!("decoder_layer: {index}/{total}"),
        _ => "decoder_layer: complete".to_owned(),
    };
    log_overall_progress(
        input_path,
        progress,
        &format!("decoding chunks {first_chunk}..{last_chunk}"),
        &[
            batch_progress_line(batch, batch_number, batch_total),
            format!("decode_phase: {:?}", decode_progress.phase),
            format!(
                "token_step: {}/{}",
                decode_progress.token_index, decode_progress.decode_limit
            ),
            layer,
            format!(
                "active_items: {}/{}",
                decode_progress.active_items, decode_progress.batch_size
            ),
            format!("token_steps_per_second: {token_steps_per_second:.3}"),
        ],
    );
}

fn estimated_decode_work_completed_for_batch(
    batch: &[AudioChunk<'_>],
    units_per_item: Option<u64>,
    decode_progress: crate::whisper::BatchedGreedyDecodeProgress,
) -> Option<u64> {
    let first_chunk = batch.first()?;
    let units_per_item = units_per_item?;
    let completed_before_batch =
        usize_to_u64_saturating(first_chunk.index.saturating_sub(1)).saturating_mul(units_per_item);
    let completed_in_batch =
        estimated_decode_work_completed_in_batch(batch.len(), units_per_item, decode_progress);
    Some(completed_before_batch.saturating_add(completed_in_batch))
}

fn estimated_decode_work_completed_in_batch(
    batch_len: usize,
    units_per_item: u64,
    decode_progress: crate::whisper::BatchedGreedyDecodeProgress,
) -> u64 {
    let batch_len = usize_to_u64_saturating(batch_len);
    match decode_progress.phase {
        crate::whisper::BatchedGreedyDecodePhase::EncoderStart
        | crate::whisper::BatchedGreedyDecodePhase::EncoderComplete => 0,
        crate::whisper::BatchedGreedyDecodePhase::TokenComplete => {
            if decode_progress.decode_limit == 0 {
                return units_per_item.saturating_mul(batch_len);
            }
            let completed_tokens = usize_to_u64_saturating(decode_progress.token_index);
            let decode_limit = usize_to_u64_saturating(decode_progress.decode_limit);
            units_per_item
                .saturating_mul(completed_tokens)
                .checked_div(decode_limit)
                .unwrap_or(units_per_item)
                .saturating_mul(batch_len)
        }
        crate::whisper::BatchedGreedyDecodePhase::DecoderLayer => {
            let Some(layer_index) = decode_progress.decoder_layer_index else {
                return 0;
            };
            let Some(layer_total) = decode_progress.decoder_layer_total else {
                return 0;
            };
            let completed_layers = usize_to_u64_saturating(
                decode_progress
                    .token_index
                    .saturating_sub(1)
                    .saturating_mul(layer_total)
                    .saturating_add(layer_index),
            );
            completed_layers
                .min(units_per_item)
                .saturating_mul(batch_len)
        }
    }
}

fn batch_chunk_bounds(batch: &[AudioChunk<'_>]) -> (usize, usize) {
    let first = batch.first().map_or(0, |chunk| chunk.index);
    let last = batch.last().map_or(first, |chunk| chunk.index);
    (first, last)
}

fn batch_progress_line(
    batch: &[AudioChunk<'_>],
    batch_number: usize,
    batch_total: usize,
) -> String {
    let completed_items = batch.last().map_or(0, |chunk| chunk.index);
    let total_items = batch.last().map_or(0, |chunk| chunk.total);
    format!("batch: {batch_number} of {batch_total} ({completed_items} of {total_items} items)")
}

fn demo_batch_progress_line(
    batch: &[DemoDecodeUnit<'_>],
    batch_number: usize,
    batch_total: usize,
) -> String {
    let completed_items = batch.last().map_or(0, |unit| unit.global_index);
    let total_items = batch.last().map_or(0, |unit| unit.global_total);
    format!("batch: {batch_number} of {batch_total} ({completed_items} of {total_items} items)")
}

struct PreparedWhisperFeature {
    features: crate::frontend::WhisperLogMelSpectrogram,
    elapsed_ms: u128,
}

fn prepare_whisper_batch_features<'a, I>(samples: I) -> eyre::Result<Vec<PreparedWhisperFeature>>
where
    I: IntoIterator<Item = &'a [f32]>,
{
    let samples = samples.into_iter().collect::<Vec<_>>();
    let mut prepared = (0..samples.len()).map(|_| None).collect::<Vec<_>>();

    std::thread::scope(|scope| -> eyre::Result<()> {
        let handles = samples
            .iter()
            .copied()
            .enumerate()
            .map(|(index, samples)| {
                scope.spawn(move || {
                    let started_at = Instant::now();
                    let features = crate::frontend::whisper_log_mel_spectrogram(samples);
                    PreparedWhisperFeature {
                        features,
                        elapsed_ms: started_at.elapsed().as_millis(),
                    }
                    .with_index(index)
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            let (index, feature) = handle.join().map_err(|panic| {
                eyre::eyre!("Whisper feature worker panicked: {}", panic_message(&panic))
            })?;
            prepared[index] = Some(feature);
        }
        Ok(())
    })?;

    prepared
        .into_iter()
        .map(|feature| feature.ok_or_else(|| eyre::eyre!("Whisper feature worker did not return")))
        .collect()
}

trait WithFeatureIndex {
    fn with_index(self, index: usize) -> (usize, Self)
    where
        Self: Sized;
}

impl WithFeatureIndex for PreparedWhisperFeature {
    fn with_index(self, index: usize) -> (usize, Self) {
        (index, self)
    }
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        return (*message).to_owned();
    }
    if let Some(message) = panic.downcast_ref::<String>() {
        return message.clone();
    }
    "unknown panic payload".to_owned()
}

#[expect(
    clippy::cast_precision_loss,
    reason = "batch progress diagnostics are approximate human-scale values"
)]
fn log_demo_batch_decode_progress(
    input_path: &Path,
    batch: &[DemoDecodeUnit<'_>],
    progress: &DemoBatchProgress,
    batch_number: usize,
    batch_total: usize,
    decode_progress: crate::whisper::BatchedGreedyDecodeProgress,
) {
    let first_item = batch.first().map_or(0, |unit| unit.global_index);
    let last_item = batch.last().map_or(first_item, |unit| unit.global_index);
    let token_steps_per_second = per_second(
        decode_progress.token_index as f64,
        decode_progress.elapsed_ms,
    );
    let layer = match (
        decode_progress.decoder_layer_index,
        decode_progress.decoder_layer_total,
    ) {
        (Some(index), Some(total)) => format!("decoder_layer: {index}/{total}"),
        _ => "decoder_layer: complete".to_owned(),
    };
    log_overall_progress(
        input_path,
        progress,
        &format!("decoding demo items {first_item}..{last_item}"),
        &[
            demo_batch_progress_line(batch, batch_number, batch_total),
            format!("decode_phase: {:?}", decode_progress.phase),
            format!(
                "token_step: {}/{}",
                decode_progress.token_index, decode_progress.decode_limit
            ),
            layer,
            format!(
                "active_items: {}/{}",
                decode_progress.active_items, decode_progress.batch_size
            ),
            format!("token_steps_per_second: {token_steps_per_second:.3}"),
        ],
    );
}

#[expect(
    clippy::too_many_lines,
    reason = "demo batching coordinates preparation, decode progress, and ordered result assembly"
)]
fn transcribe_demo_items_batched(
    items: Vec<DemoBatchItem>,
    cache_home: &crate::paths::CacheHome,
    args: &AudioTranscribeArgs,
    decoder: &crate::whisper::LoadedWhisperGreedyDecoder,
    decode_workers: usize,
    max_decode_tokens: usize,
) -> eyre::Result<Vec<(VctkDemoClip, TranscribedInput)>> {
    // audio[impl cli.transcribe-decode-workers]
    // audio[impl cli.transcribe-demo-batched]
    tracing::info!(
        batch_size = decode_workers,
        items = items.len(),
        "Starting batched demo transcription"
    );

    let mut prepared_inputs = Vec::with_capacity(items.len());
    for item in items {
        tracing::info!(path = %item.clip.wav_path.display(), expected_text = %item.clip.expected_text, "Preparing demo transcription sample");
        if args.compare_python {
            print_python_reference_comparison(&item.clip.wav_path, &args.model)?;
        }

        let original_duration_seconds = item.metadata.duration_seconds;
        let audio = load_effective_transcription_audio(
            &item.clip.wav_path,
            item.metadata,
            &TranscribeOneContext {
                cache_home,
                args,
                decoder,
                is_demo: true,
                max_decode_tokens,
                decode_workers: 1,
            },
        )?;
        let audio_seconds = original_duration_seconds.unwrap_or_else(|| audio.duration_seconds());
        let chunk_boundaries = speech_aware_chunk_boundaries(&audio.samples);
        prepared_inputs.push(DemoPreparedInput {
            clip: item.clip,
            audio,
            bytes: item.bytes,
            audio_seconds,
            chunk_boundaries,
        });
    }

    let total_decode_units = prepared_inputs
        .iter()
        .map(|input| input.chunk_boundaries.len().max(1))
        .sum::<usize>();
    let mut decode_units = Vec::with_capacity(total_decode_units);
    for (input_index, input) in prepared_inputs.iter().enumerate() {
        let total = input.chunk_boundaries.len().max(1);
        for (offset, (start_sample, end_sample)) in
            input.chunk_boundaries.iter().copied().enumerate()
        {
            let global_index = decode_units.len() + 1;
            decode_units.push(DemoDecodeUnit {
                input_index,
                global_index,
                global_total: total_decode_units,
                chunk: AudioChunk {
                    index: offset + 1,
                    total,
                    total_samples: input.audio.samples.len(),
                    start_sample,
                    end_sample,
                    samples: &input.audio.samples[start_sample..end_sample],
                },
            });
        }
    }

    let totals = DemoBatchTotals {
        items: total_decode_units,
        bytes: prepared_inputs.iter().map(|input| input.bytes).sum(),
        audio_seconds: prepared_inputs
            .iter()
            .map(|input| input.audio_seconds)
            .sum(),
    };
    let mut progress = DemoBatchProgress::new(totals);
    progress.configure_estimated_decode_work(
        total_decode_units,
        estimated_decode_work_units_per_item(decoder),
    );
    let mut chunk_results = (0..prepared_inputs.len())
        .map(|_| Vec::new())
        .collect::<Vec<Vec<TranscriptionChunkResult>>>();

    let batch_size = decode_workers.min(decode_units.len().max(1));
    let batch_total = decode_units.len().div_ceil(batch_size);
    for (batch_index, batch) in decode_units.chunks(batch_size).enumerate() {
        let batch_number = batch_index + 1;
        log_overall_progress(
            &prepared_inputs[batch[0].input_index].audio.path,
            &progress,
            "preparing demo batch",
            &[demo_batch_progress_line(batch, batch_number, batch_total)],
        );
        let prepared_features =
            prepare_whisper_batch_features(batch.iter().map(|unit| unit.chunk.samples))?;
        let features = prepared_features
            .iter()
            .map(|prepared| prepared.features.clone())
            .collect::<Vec<_>>();

        let decode_started_at = Instant::now();
        let summaries = decoder.decode_batch_with_progress(&features, |decode_progress| {
            if should_log_batch_token_progress(decode_progress) {
                if let Some(units_per_item) = progress.estimated_decode_work_units_per_item() {
                    let completed_before_batch =
                        usize_to_u64_saturating(batch[0].global_index.saturating_sub(1))
                            .saturating_mul(units_per_item);
                    let completed_in_batch = estimated_decode_work_completed_in_batch(
                        batch.len(),
                        units_per_item,
                        decode_progress,
                    );
                    progress.record_estimated_decode_work(
                        completed_before_batch.saturating_add(completed_in_batch),
                    );
                }
                log_demo_batch_decode_progress(
                    &prepared_inputs[batch[0].input_index].audio.path,
                    batch,
                    &progress,
                    batch_number,
                    batch_total,
                    decode_progress,
                );
            }
        })?;
        let decode_elapsed_ms = decode_started_at.elapsed().as_millis();
        for ((unit, summary), prepared_features) in
            batch.iter().copied().zip(summaries).zip(prepared_features)
        {
            let input = &prepared_inputs[unit.input_index];
            let result = transcription_chunk_result_from_summary(
                &input.audio,
                unit.chunk,
                prepared_features.elapsed_ms,
                decode_elapsed_ms,
                summary,
            );
            progress.record_values(
                chunk_audio_bytes(input.bytes, unit.chunk),
                result.audio_seconds,
                result.words,
            );
            chunk_results[unit.input_index].push(result);
        }
        log_overall_progress(
            &prepared_inputs[batch[0].input_index].audio.path,
            &progress,
            "completed demo batch",
            &[demo_batch_progress_line(batch, batch_number, batch_total)],
        );
    }

    Ok(prepared_inputs
        .into_iter()
        .zip(chunk_results)
        .map(|(input, chunk_results)| {
            let text = chunk_results
                .iter()
                .map(|chunk| chunk.text.trim())
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            let words = chunk_results.iter().map(|chunk| chunk.words).sum();
            let timing_records = chunk_results
                .iter()
                .map(|chunk| chunk.timing_record.clone())
                .collect();
            (
                input.clip,
                TranscribedInput {
                    text,
                    bytes: input.bytes,
                    audio_seconds: input.audio_seconds,
                    words,
                    timing_records,
                    streamed_stdout: false,
                },
            )
        })
        .collect())
}

fn automatic_worker_cap_from_vram(
    model_name: &str,
    dims: Option<&crate::whisper::WhisperDims>,
) -> Option<usize> {
    // audio[impl cli.transcribe-vram-aware-workers]
    let gpu = query_gpu_memory()?;
    let per_worker_bytes = estimate_decode_worker_vram_bytes(model_name, dims);
    if per_worker_bytes == 0 {
        return None;
    }

    let reservable_bytes = gpu.free_bytes.saturating_mul(85) / 100;
    let workers = (reservable_bytes / per_worker_bytes).max(1);
    tracing::info!(
        vram_free = %human_bytes(gpu.free_bytes),
        vram_reservable = %human_bytes(reservable_bytes),
        estimated_worker_vram = %human_bytes(per_worker_bytes),
        workers,
        model = %model_name,
        "Estimated Burn Whisper decode parallelism from available VRAM"
    );
    usize::try_from(workers).ok()
}

fn estimate_decode_worker_vram_bytes(
    model_name: &str,
    dims: Option<&crate::whisper::WhisperDims>,
) -> u64 {
    known_model_worker_vram_bytes(model_name)
        .or_else(|| dims.map(estimate_worker_vram_bytes_from_dims))
        .unwrap_or_else(gibibytes)
}

fn known_model_worker_vram_bytes(model_name: &str) -> Option<u64> {
    crate::model::KNOWN_WHISPER_MODELS
        .iter()
        .find(|model| model.name.eq_ignore_ascii_case(model_name))
        .and_then(|model| parse_parameter_count(model.parameter_count))
        .map(estimate_worker_vram_bytes_from_parameters)
}

fn parse_parameter_count(parameter_count: &str) -> Option<u64> {
    let normalized = parameter_count.trim();
    let numeric = normalized
        .trim_end_matches('M')
        .trim_end_matches('m')
        .trim_end_matches('B')
        .trim_end_matches('b')
        .trim();
    let multiplier = if normalized.ends_with('B') || normalized.ends_with('b') {
        1_000_000_000
    } else {
        1_000_000
    };
    numeric.parse::<u64>().ok()?.checked_mul(multiplier)
}

fn estimate_worker_vram_bytes_from_parameters(parameters: u64) -> u64 {
    let parameter_bytes = parameters.saturating_mul(4);
    parameter_bytes
        .saturating_add(256 * mebibytes())
        .max(384 * mebibytes())
}

fn estimate_worker_vram_bytes_from_dims(dims: &crate::whisper::WhisperDims) -> u64 {
    match dims.audio.n_audio_state {
        0..=512 => gibibytes(),
        513..=768 => 2 * gibibytes(),
        769..=1024 => 5 * gibibytes(),
        _ => 10 * gibibytes(),
    }
}

fn mebibytes() -> u64 {
    1024 * 1024
}

fn gibibytes() -> u64 {
    1024 * 1024 * 1024
}

fn transcribe_audio_chunk(
    input: &crate::transcription::ValidatedAudio,
    chunk: AudioChunk<'_>,
    decoder: &crate::whisper::LoadedWhisperGreedyDecoder,
    max_decode_tokens: usize,
) -> eyre::Result<TranscriptionChunkResult> {
    tracing::debug!(
        max_decode_tokens,
        chunk_index = chunk.index,
        chunk_total = chunk.total,
        "Building Burn Whisper transcription chunk request"
    );
    let request_started_at = Instant::now();
    let features = crate::frontend::whisper_log_mel_spectrogram(chunk.samples);
    let frontend_elapsed_ms = request_started_at.elapsed().as_millis();
    tracing::debug!(
        elapsed_ms = frontend_elapsed_ms,
        mel_bins = features.n_mels,
        frames = features.n_frames,
        chunk_index = chunk.index,
        chunk_total = chunk.total,
        "Built Burn Whisper transcription chunk request"
    );
    let decode_started_at = Instant::now();
    tracing::debug!(
        chunk_index = chunk.index,
        chunk_total = chunk.total,
        "Starting Burn Whisper chunk decode"
    );
    let summary = decoder.decode(&features)?;
    let decode_elapsed_ms = decode_started_at.elapsed().as_millis();
    Ok(transcription_chunk_result_from_summary(
        input,
        chunk,
        frontend_elapsed_ms,
        decode_elapsed_ms,
        summary,
    ))
}

fn transcription_chunk_result_from_summary(
    input: &crate::transcription::ValidatedAudio,
    chunk: AudioChunk<'_>,
    frontend_elapsed_ms: u128,
    decode_elapsed_ms: u128,
    summary: crate::whisper::GreedyDecodeSummary,
) -> TranscriptionChunkResult {
    let generated_tokens = summary.generated_token_ids.len();
    let chunk_audio_seconds = sample_index_seconds(chunk.samples.len());
    let tokens_per_second = tokens_per_second(generated_tokens, summary.decoder_elapsed_ms);
    let audio_seconds_per_second = per_second(chunk_audio_seconds, decode_elapsed_ms);
    let words = summary.text.split_whitespace().count();
    tracing::debug!(elapsed_ms = decode_elapsed_ms, text = %summary.text, stop_reason = ?summary.stop_reason, generated_tokens, chunk_index = chunk.index, chunk_total = chunk.total, path = %input.path.display(), "Finished Burn Whisper chunk decode");
    // audio[impl cli.transcribe-stage-timing]
    tracing::debug!(
        path = %input.path.display(),
        chunk_index = chunk.index,
        chunk_total = chunk.total,
        chunk_audio_seconds,
        frontend_elapsed_ms,
        decode_elapsed_ms,
        decoder_total_elapsed_ms = summary.total_elapsed_ms,
        encoder_elapsed_ms = summary.encoder_elapsed_ms,
        decoder_elapsed_ms = summary.decoder_elapsed_ms,
        generated_tokens,
        tokens_per_second,
        audio_seconds_per_second,
        stop_reason = ?summary.stop_reason,
        "Transcribed audio chunk"
    );
    let audio_seconds = chunk_audio_seconds;
    let timing_record = TranscriptionTimingRecord {
        schema: "teamy-studio.audio.transcription-timing.v1",
        backend: "burn-cuda-greedy",
        input_path: input.path.display().to_string(),
        chunk_index: chunk.index,
        chunk_total: chunk.total,
        start_seconds: sample_index_seconds(chunk.start_sample),
        end_seconds: sample_index_seconds(chunk.end_sample),
        audio_seconds,
        frontend_elapsed_ms,
        decode_elapsed_ms,
        decoder_total_elapsed_ms: summary.total_elapsed_ms,
        encoder_elapsed_ms: summary.encoder_elapsed_ms,
        decoder_elapsed_ms: summary.decoder_elapsed_ms,
        generated_tokens,
        tokens_per_second,
        audio_seconds_per_second,
        stop_reason: format!("{:?}", summary.stop_reason),
        words,
    };
    TranscriptionChunkResult {
        text: summary.text,
        audio_seconds,
        words,
        timing_record,
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "stage timing diagnostics are approximate human-scale throughput values"
)]
fn tokens_per_second(tokens: usize, elapsed_ms: u128) -> f64 {
    per_second(tokens as f64, elapsed_ms)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "stage timing diagnostics are approximate human-scale throughput values"
)]
fn per_second(value: f64, elapsed_ms: u128) -> f64 {
    if elapsed_ms == 0 {
        return 0.0;
    }
    value / (elapsed_ms as f64 / 1000.0)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "sample offsets are rendered as approximate seconds for progress diagnostics"
)]
fn sample_index_seconds(sample_index: usize) -> f64 {
    sample_index as f64 / f64::from(crate::audio::TRANSCRIPTION_SAMPLE_RATE)
}

const SPEECH_BOUNDARY_SEARCH_SECONDS: usize = 2;
const SPEECH_BOUNDARY_FRAME_MS: usize = 50;
const MIN_SPEECH_AWARE_CHUNK_SECONDS: usize = 5;

fn audio_chunks(samples: &[f32]) -> Vec<AudioChunk<'_>> {
    // audio[impl cli.transcribe-speech-aware-chunks]
    let boundaries = speech_aware_chunk_boundaries(samples);
    let total = boundaries.len().max(1);
    boundaries
        .into_iter()
        .enumerate()
        .map(|(offset, (start_sample, end_sample))| AudioChunk {
            index: offset + 1,
            total,
            total_samples: samples.len(),
            start_sample,
            end_sample,
            samples: &samples[start_sample..end_sample],
        })
        .collect()
}

fn speech_aware_chunk_boundaries(samples: &[f32]) -> Vec<(usize, usize)> {
    if samples.is_empty() {
        return vec![(0, 0)];
    }

    let max_chunk_samples = crate::frontend::N_SAMPLES;
    let min_chunk_samples = MIN_SPEECH_AWARE_CHUNK_SECONDS
        .saturating_mul(crate::audio::TRANSCRIPTION_SAMPLE_RATE as usize);
    let mut boundaries = Vec::new();
    let mut start_sample = 0;
    while start_sample < samples.len() {
        let hard_end = (start_sample + max_chunk_samples).min(samples.len());
        if hard_end == samples.len() {
            boundaries.push((start_sample, hard_end));
            break;
        }

        let snapped_end = quietest_boundary_before(samples, start_sample, hard_end)
            .filter(|boundary| *boundary > start_sample + min_chunk_samples)
            .unwrap_or(hard_end);
        boundaries.push((start_sample, snapped_end));
        start_sample = snapped_end;
    }
    boundaries
}

#[expect(
    clippy::cast_precision_loss,
    reason = "frame energy comparison only needs approximate relative RMS values"
)]
fn quietest_boundary_before(
    samples: &[f32],
    start_sample: usize,
    hard_end: usize,
) -> Option<usize> {
    let sample_rate = crate::audio::TRANSCRIPTION_SAMPLE_RATE as usize;
    let search_samples = SPEECH_BOUNDARY_SEARCH_SECONDS.saturating_mul(sample_rate);
    let frame_samples = (SPEECH_BOUNDARY_FRAME_MS.saturating_mul(sample_rate) / 1000).max(1);
    let search_start = hard_end.saturating_sub(search_samples).max(start_sample);
    if search_start >= hard_end || hard_end.saturating_sub(search_start) < frame_samples {
        return None;
    }

    (search_start..=hard_end - frame_samples)
        .step_by(frame_samples)
        .map(|frame_start| {
            let frame_end = frame_start + frame_samples;
            let energy = samples[frame_start..frame_end]
                .iter()
                .map(|sample| sample * sample)
                .sum::<f32>()
                / frame_samples as f32;
            (frame_start + frame_samples / 2, energy)
        })
        .min_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(boundary, _)| boundary)
}

#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "chunk byte progress estimates map media bytes onto audio duration for human ETA display"
)]
fn estimate_chunk_bytes(
    total_bytes: u64,
    total_audio_seconds: f64,
    chunk_audio_seconds: f64,
) -> u64 {
    if total_audio_seconds <= f64::EPSILON || !total_audio_seconds.is_finite() {
        return 0;
    }
    ((total_bytes as f64) * (chunk_audio_seconds / total_audio_seconds)).round() as u64
}

fn default_prepared_output_path(cache_home: &crate::paths::CacheHome, input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("audio");
    cache_home
        .0
        .join("audio")
        .join("prepared")
        .join(format!("{stem}.16khz-mono.wav"))
}

fn write_timing_jsonl(path: &Path, records: &[TranscriptionTimingRecord]) -> eyre::Result<()> {
    // audio[impl cli.transcribe-timing-jsonl]
    let file = std::fs::File::create(path).wrap_err_with(|| {
        format!(
            "failed to create transcription timing JSONL file {}",
            path.display()
        )
    })?;
    let mut writer = std::io::BufWriter::new(file);
    for record in records {
        serde_json::to_writer(&mut writer, record).wrap_err_with(|| {
            format!(
                "failed to serialize transcription timing record for {} chunk {}/{}",
                record.input_path, record.chunk_index, record.chunk_total
            )
        })?;
        writer.write_all(b"\n").wrap_err_with(|| {
            format!(
                "failed to write transcription timing JSONL file {}",
                path.display()
            )
        })?;
    }
    writer.flush().wrap_err_with(|| {
        format!(
            "failed to flush transcription timing JSONL file {}",
            path.display()
        )
    })
}

fn find_vctk_demo_clips(count: usize) -> eyre::Result<Vec<VctkDemoClip>> {
    let roots = [
        PathBuf::from("g:/Datasets/VCTK/VCTK-Corpus-smaller"),
        PathBuf::from("G:/Datasets/VCTK/VCTK-Corpus-smaller"),
        PathBuf::from("./VCTK-Corpus-smaller"),
    ];
    for root in roots {
        if let Some(clips) = find_vctk_demo_clips_under(&root)? {
            return Ok(select_demo_clips(&clips, count));
        }
    }
    bail!("Could not find a local VCTK-Corpus-smaller dataset for --demo")
}

fn find_vctk_demo_clips_under(root: &Path) -> eyre::Result<Option<Vec<VctkDemoClip>>> {
    let wav_root = root.join("wav48");
    let txt_root = root.join("txt");
    if !wav_root.is_dir() || !txt_root.is_dir() {
        return Ok(None);
    }

    let mut clips = Vec::new();
    for speaker in std::fs::read_dir(&wav_root)
        .wrap_err_with(|| format!("failed to read VCTK wav root {}", wav_root.display()))?
    {
        let speaker = speaker?;
        if !speaker.file_type()?.is_dir() {
            continue;
        }
        for wav in std::fs::read_dir(speaker.path())? {
            let wav = wav?;
            let wav_path = wav.path();
            if wav_path
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .is_none_or(|extension| !extension.eq_ignore_ascii_case("wav"))
            {
                continue;
            }
            let Some(stem) = wav_path.file_stem().and_then(std::ffi::OsStr::to_str) else {
                continue;
            };
            let Some(speaker_name) = wav_path
                .parent()
                .and_then(Path::file_name)
                .and_then(std::ffi::OsStr::to_str)
            else {
                continue;
            };
            let transcript_path = txt_root.join(speaker_name).join(format!("{stem}.txt"));
            if transcript_path.is_file() {
                let expected_text = std::fs::read_to_string(&transcript_path)
                    .wrap_err_with(|| format!("failed to read {}", transcript_path.display()))?
                    .trim()
                    .to_owned();
                clips.push(VctkDemoClip {
                    wav_path,
                    expected_text,
                });
            }
        }
    }
    if clips.is_empty() {
        return Ok(None);
    }
    clips.sort_by(|left, right| left.wav_path.cmp(&right.wav_path));
    Ok(Some(clips))
}

fn select_demo_clips(clips: &[VctkDemoClip], count: usize) -> Vec<VctkDemoClip> {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos() as usize);
    let index = seed % clips.len();
    (0..count)
        .map(|offset| clips[(index + offset) % clips.len()].clone())
        .collect()
}

fn query_gpu_memory() -> Option<GpuMemorySnapshot> {
    let output = Command::new("nvidia-smi")
        .arg("--query-gpu=memory.used,memory.free")
        .arg("--format=csv,noheader,nounits")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next()?.trim();
    let mut parts = first_line.split(',').map(str::trim);
    let used_mib = parts.next()?.parse::<u64>().ok()?;
    let free_mib = parts.next()?.parse::<u64>().ok()?;
    Some(GpuMemorySnapshot {
        used_bytes: used_mib.saturating_mul(1024 * 1024),
        free_bytes: free_mib.saturating_mul(1024 * 1024),
    })
}

#[expect(
    clippy::cast_precision_loss,
    reason = "demo metrics render human-readable approximate byte sizes"
)]
fn human_bytes(bytes: u64) -> String {
    human_bytes_f64(bytes as f64)
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "demo throughput display clamps non-finite byte rates before integer formatting"
)]
fn human_bytes_f64(bytes: f64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if !bytes.is_finite() || bytes <= 0.0 {
        return "0 B".to_owned();
    }
    let mut value = bytes;
    let mut unit = UNITS[0];
    for candidate in &UNITS[1..] {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = candidate;
    }
    if unit == "B" {
        format!("{} {unit}", value.round() as u64)
    } else {
        format!("{value:.2} {unit}")
    }
}

fn format_eta(remaining: f64, per_second: f64) -> String {
    if remaining <= 0.0 {
        return "0s".to_owned();
    }
    if !per_second.is_finite() || per_second <= f64::EPSILON {
        return "unknown".to_owned();
    }
    format_duration(Duration::from_secs_f64(remaining / per_second))
}

fn format_clock_eta(remaining: f64, per_second: f64) -> String {
    if remaining <= 0.0 {
        return chrono::Local::now()
            .format("%I:%M%p")
            .to_string()
            .trim_start_matches('0')
            .to_owned();
    }
    if !per_second.is_finite() || per_second <= f64::EPSILON {
        return "unknown".to_owned();
    }
    let countdown = Duration::from_secs_f64(remaining / per_second);
    let Ok(countdown) = chrono::Duration::from_std(countdown) else {
        return "unknown".to_owned();
    };
    (chrono::Local::now() + countdown)
        .format("%I:%M%p")
        .to_string()
        .trim_start_matches('0')
        .to_owned()
}

fn format_duration(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{hours}h {minutes:02}m {seconds:02}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

fn format_audio_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "unknown".to_owned();
    }
    format_duration(Duration::from_secs_f64(seconds))
}

fn print_python_reference_comparison(input_path: &Path, model_name: &str) -> eyre::Result<()> {
    let script = r"
import json, sys, torch, whisper
audio_path = sys.argv[1]
model_name = sys.argv[2]
model = whisper.load_model(model_name, device='cuda' if torch.cuda.is_available() else 'cpu')
audio = whisper.load_audio(audio_path)
mel = whisper.log_mel_spectrogram(whisper.pad_or_trim(audio)).to(model.device)
tokenizer = whisper.tokenizer.get_tokenizer(model.is_multilingual, language='en', task='transcribe')
prompt = [tokenizer.sot]
if model.is_multilingual:
    prompt.append(tokenizer.language_token)
prompt.extend([tokenizer.transcribe, tokenizer.no_timestamps])
with torch.no_grad():
    encoder_output = model.encoder(mel.unsqueeze(0))
    logits = model.decoder(torch.tensor([prompt], device=model.device), encoder_output)
    top = torch.topk(logits[0, -1], 10)
    decoded = [tokenizer.decode([int(token_id)]) for token_id in top.indices.tolist()]
    text = whisper.decode(model, mel, whisper.DecodingOptions(language='en', task='transcribe', without_timestamps=True, fp16=torch.cuda.is_available())).text
print(json.dumps({
    'backend': 'openai-whisper-python',
    'device': str(model.device),
    'model': model_name,
    'is_multilingual': model.is_multilingual,
    'prompt': prompt,
    'transcript': text,
    'encoder_shape': list(encoder_output.shape),
    'decoder_logits_shape': list(logits.shape),
    'top_ids': top.indices.tolist(),
    'top_values': [float(value) for value in top.values],
    'top_decoded': decoded,
}, indent=2))
";
    let output = Command::new("uv")
        .arg("run")
        .arg("--no-project")
        .arg("--index")
        .arg("https://download.pytorch.org/whl/cu128")
        .arg("--with")
        .arg("numpy")
        .arg("--with")
        .arg("openai-whisper")
        .arg("--with")
        .arg("torch")
        .arg("python")
        .arg("-c")
        .arg(script)
        .arg(input_path)
        .arg(model_name)
        .output()
        .wrap_err("failed to run Python OpenAI Whisper comparison")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        bail!(
            "Python OpenAI Whisper comparison failed with {}\n{}",
            output.status,
            stderr.trim()
        );
    }
    eprintln!("Python reference:\n{}", stdout.trim());
    if !stderr.trim().is_empty() {
        tracing::debug!(stderr = %stderr.trim(), "Python comparison wrote stderr");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_count(seconds: usize) -> usize {
        seconds * crate::audio::TRANSCRIPTION_SAMPLE_RATE as usize
    }

    #[test]
    fn speech_aware_chunks_keep_short_audio_as_one_chunk() {
        let samples = vec![0.1; sample_count(12)];

        let chunks = audio_chunks(&samples);

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].start_sample, 0);
        assert_eq!(chunks[0].end_sample, samples.len());
        assert_eq!(chunks[0].total_samples, samples.len());
    }

    #[test]
    fn speech_aware_chunks_snap_to_quiet_boundary_before_window_limit() {
        let mut samples = vec![0.4; sample_count(61)];
        let silence_start = sample_count(29);
        let silence_end = sample_count(30);
        for sample in &mut samples[silence_start..silence_end] {
            *sample = 0.0;
        }

        let chunks = audio_chunks(&samples);

        assert!(chunks.len() >= 3);
        assert!(chunks[0].end_sample >= silence_start);
        assert!(chunks[0].end_sample <= silence_end);
        assert_eq!(chunks[1].start_sample, chunks[0].end_sample);
    }

    #[test]
    fn speech_aware_chunks_do_not_exceed_whisper_window() {
        let samples = vec![0.4; sample_count(95)];

        let chunks = audio_chunks(&samples);

        assert!(chunks.len() >= 4);
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.samples.len() <= crate::frontend::N_SAMPLES)
        );
        assert_eq!(chunks.first().expect("at least one chunk").start_sample, 0);
        assert_eq!(
            chunks.last().expect("at least one chunk").end_sample,
            samples.len()
        );
    }

    #[test]
    fn estimated_decode_work_advances_with_decoder_layers() {
        let samples = vec![0.4; sample_count(61)];
        let chunks = audio_chunks(&samples);
        let units_per_item = 12;

        let completed = estimated_decode_work_completed_for_batch(
            &chunks[1..3],
            Some(units_per_item),
            crate::whisper::BatchedGreedyDecodeProgress {
                phase: crate::whisper::BatchedGreedyDecodePhase::DecoderLayer,
                token_index: 2,
                decode_limit: 3,
                batch_size: 2,
                active_items: 2,
                decoder_layer_index: Some(2),
                decoder_layer_total: Some(4),
                elapsed_ms: 100,
            },
        )
        .expect("batch has chunks and configured work units");

        assert_eq!(completed, units_per_item + 2 * 6);
    }
}
