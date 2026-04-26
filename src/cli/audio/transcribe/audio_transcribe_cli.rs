use arbitrary::Arbitrary;
use eyre::{Context, bail};
use facet::Facet;
use figue as args;
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
        let model_dir = crate::model::resolve_transcription_model_dir(
            app_home,
            cache_home,
            Some(&self.model),
            explicit_model_dir.as_deref(),
        )?;
        tracing::debug!(model_dir = %model_dir.display(), "Resolved Burn Whisper model directory");
        let model_load_started_at = Instant::now();
        let model = crate::model::inspect_model_dir(&model_dir).wrap_err_with(|| {
            format!(
                "failed to load Burn Whisper model from {}. Run `teamy-studio audio model prepare {}` first, or pass --model-dir <dir>.",
                model_dir.display(),
                self.model,
            )
        })?;
        tracing::debug!(elapsed_ms = model_load_started_at.elapsed().as_millis(), layout = ?model.layout, "Inspected Burn Whisper model directory");
        let decoder = crate::whisper::LoadedWhisperGreedyDecoder::load(model, max_decode_tokens)?;
        let result = transcribe_one_input(
            &input_path,
            metadata,
            cache_home,
            &self,
            &decoder,
            false,
            max_decode_tokens,
        )?;
        tracing::debug!(
            elapsed_ms = command_started_at.elapsed().as_millis(),
            "Finished audio transcribe command"
        );

        println!("{}", result.text);
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
        if demo_count > 1 && self.prepared_output.is_some() {
            bail!("--prepared-output cannot be used with --demo counts greater than 1");
        }
        tracing::debug!(demo_count, "Searching for VCTK demo clips");
        let clips = find_vctk_demo_clips(demo_count)?;
        let items = clips
            .into_iter()
            .map(DemoBatchItem::inspect)
            .collect::<eyre::Result<Vec<_>>>()?;
        let totals = DemoBatchTotals::from_items(&items);

        let explicit_model_dir = self.model_dir.as_deref().map(PathBuf::from);
        let model_dir = crate::model::resolve_transcription_model_dir(
            app_home,
            cache_home,
            Some(&self.model),
            explicit_model_dir.as_deref(),
        )?;
        tracing::debug!(model_dir = %model_dir.display(), "Resolved Burn Whisper model directory for demo batch");
        let model = crate::model::inspect_model_dir(&model_dir).wrap_err_with(|| {
            format!(
                "failed to load Burn Whisper model from {}. Run `teamy-studio audio model prepare {}` first, or pass --model-dir <dir>.",
                model_dir.display(),
                self.model,
            )
        })?;
        let decoder = crate::whisper::LoadedWhisperGreedyDecoder::load(model, max_decode_tokens)?;
        let mut progress = DemoBatchProgress::new(totals);

        if demo_count > 1 {
            eprintln!("Demo batch: {} samples", items.len());
        }

        for (index, item) in items.into_iter().enumerate() {
            eprintln!("Demo clip: {}", item.clip.wav_path.display());
            eprintln!("Expected text: {}", item.clip.expected_text);
            if self.compare_python {
                print_python_reference_comparison(&item.clip.wav_path, &self.model)?;
            }
            let result = transcribe_one_input(
                &item.clip.wav_path,
                item.metadata,
                cache_home,
                self,
                &decoder,
                true,
                max_decode_tokens,
            )?;
            println!("{}", result.text);
            progress.record(&result);
            if demo_count > 1 {
                eprintln!(
                    "{}",
                    progress.render(index + 1, &item.clip, query_gpu_memory())
                );
            }
        }

        tracing::debug!(
            elapsed_ms = command_started_at.elapsed().as_millis(),
            "Finished audio transcribe demo batch"
        );
        Ok(CliOutput::none())
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

impl DemoBatchTotals {
    fn from_items(items: &[DemoBatchItem]) -> Self {
        Self {
            items: items.len(),
            bytes: items.iter().map(|item| item.bytes).sum(),
            audio_seconds: items.iter().map(|item| item.audio_seconds).sum(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct TranscribedInput {
    text: String,
    bytes: u64,
    audio_seconds: f64,
    words: usize,
}

#[derive(Clone, Debug)]
struct DemoBatchProgress {
    totals: DemoBatchTotals,
    started_at: Instant,
    processed_items: usize,
    processed_bytes: u64,
    processed_audio_seconds: f64,
    processed_words: usize,
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
        }
    }

    fn record(&mut self, input: &TranscribedInput) {
        self.processed_items = self.processed_items.saturating_add(1);
        self.processed_bytes = self.processed_bytes.saturating_add(input.bytes);
        self.processed_audio_seconds += input.audio_seconds;
        self.processed_words = self.processed_words.saturating_add(input.words);
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "demo progress metrics are human-scale floating point rates"
    )]
    fn render(
        &self,
        current_index: usize,
        clip: &VctkDemoClip,
        gpu: Option<GpuMemorySnapshot>,
    ) -> String {
        let elapsed = self.started_at.elapsed().as_secs_f64().max(f64::EPSILON);
        let bytes_per_second = self.processed_bytes as f64 / elapsed;
        let audio_seconds_per_second = self.processed_audio_seconds / elapsed;
        let items_per_second = self.processed_items as f64 / elapsed;
        let words_per_second = self.processed_words as f64 / elapsed;
        let bytes_remaining = self.totals.bytes.saturating_sub(self.processed_bytes);
        let items_remaining = self.totals.items.saturating_sub(self.processed_items);
        let audio_seconds_remaining =
            (self.totals.audio_seconds - self.processed_audio_seconds).max(0.0);
        let (vram_used, vram_remaining) = gpu.map_or_else(
            || ("unknown".to_owned(), "unknown".to_owned()),
            |snapshot| {
                (
                    human_bytes(snapshot.used_bytes),
                    human_bytes(snapshot.free_bytes),
                )
            },
        );

        [
            "demo_metrics:".to_owned(),
            format!("  item: {current_index}/{}", self.totals.items),
            format!("  clip: {}", clip.wav_path.display()),
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
            format!("  vram_used: {vram_used}"),
            format!("  vram_remaining: {vram_remaining}"),
        ]
        .join("\n")
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

fn transcribe_one_input(
    input_path: &Path,
    metadata: crate::audio::AudioMetadata,
    cache_home: &crate::paths::CacheHome,
    args: &AudioTranscribeArgs,
    decoder: &crate::whisper::LoadedWhisperGreedyDecoder,
    is_demo: bool,
    max_decode_tokens: usize,
) -> eyre::Result<TranscribedInput> {
    let bytes = std::fs::metadata(input_path)
        .wrap_err_with(|| format!("failed to stat {}", input_path.display()))?
        .len();
    let original_duration_seconds = metadata.duration_seconds;
    let issues = crate::audio::validate_for_transcription(&metadata);
    tracing::debug!(issue_count = issues.len(), sample_rate_hz = ?metadata.sample_rate_hz, channels = ?metadata.channels, "Inspected transcription input audio");

    let effective_audio = if issues.is_empty() {
        tracing::debug!(path = %input_path.display(), "Loading already-compliant transcription audio");
        crate::transcription::load_validated_audio(input_path, metadata)?
    } else if args.resample || is_demo {
        let output_path = args.prepared_output.as_ref().map_or_else(
            || default_prepared_output_path(cache_home, input_path),
            PathBuf::from,
        );
        let resample_started_at = Instant::now();
        let overwrite = args.overwrite || is_demo;
        tracing::debug!(input = %input_path.display(), output = %output_path.display(), overwrite, "Preparing transcription audio with ffmpeg");
        let prepared = crate::audio::prepare_audio(input_path, &output_path, overwrite)?;
        tracing::debug!(elapsed_ms = resample_started_at.elapsed().as_millis(), output = %prepared.path.display(), "Prepared transcription audio");
        crate::transcription::load_validated_audio(&prepared.path, prepared.metadata)?
    } else {
        bail!(crate::audio::render_transcription_contract_error(
            &metadata, &issues,
        ));
    };

    tracing::debug!(
        max_decode_tokens,
        "Building Burn Whisper transcription request"
    );
    let request_started_at = Instant::now();
    let request =
        crate::transcription::build_transcription_request(effective_audio, None, max_decode_tokens);
    tracing::debug!(
        elapsed_ms = request_started_at.elapsed().as_millis(),
        mel_bins = request.features.n_mels,
        frames = request.features.n_frames,
        "Built Burn Whisper transcription request"
    );
    let decode_started_at = Instant::now();
    tracing::debug!("Starting Burn Whisper decode");
    let summary = decoder.decode(&request.features)?;
    tracing::debug!(elapsed_ms = decode_started_at.elapsed().as_millis(), text = %summary.text, stop_reason = ?summary.stop_reason, generated_tokens = summary.generated_token_ids.len(), "Finished Burn Whisper decode");
    let audio_seconds =
        original_duration_seconds.unwrap_or_else(|| request.input.duration_seconds());
    let words = summary.text.split_whitespace().count();
    Ok(TranscribedInput {
        text: summary.text,
        bytes,
        audio_seconds,
        words,
    })
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
