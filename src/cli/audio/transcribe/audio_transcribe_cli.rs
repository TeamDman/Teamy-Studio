use arbitrary::Arbitrary;
use eyre::{Context, bail};
use facet::Facet;
use figue as args;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

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
        let demo = if self.demo {
            tracing::debug!("Searching for VCTK demo clip");
            Some(find_vctk_demo_clip()?)
        } else {
            None
        };
        let input_path = if let Some(demo) = &demo {
            eprintln!("Demo clip: {}", demo.wav_path.display());
            eprintln!("Expected text: {}", demo.expected_text);
            demo.wav_path.clone()
        } else {
            PathBuf::from(self.input.as_deref().ok_or_else(|| {
                eyre::eyre!("audio transcribe requires an INPUT path unless --demo is set")
            })?)
        };
        tracing::debug!(path = %input_path.display(), "Inspecting transcription input audio");
        let metadata = crate::audio::inspect_audio(&input_path)?;
        let issues = crate::audio::validate_for_transcription(&metadata);
        tracing::debug!(issue_count = issues.len(), sample_rate_hz = ?metadata.sample_rate_hz, channels = ?metadata.channels, "Inspected transcription input audio");

        if self.compare_python {
            print_python_reference_comparison(&input_path, &self.model)?;
        }

        let effective_audio = if issues.is_empty() {
            tracing::debug!(path = %input_path.display(), "Loading already-compliant transcription audio");
            crate::transcription::load_validated_audio(&input_path, metadata)?
        } else if self.resample || self.demo {
            let output_path = self.prepared_output.map_or_else(
                || default_prepared_output_path(cache_home, &input_path),
                PathBuf::from,
            );
            let resample_started_at = Instant::now();
            let overwrite = self.overwrite || self.demo;
            tracing::debug!(input = %input_path.display(), output = %output_path.display(), overwrite, "Preparing transcription audio with ffmpeg");
            let prepared = crate::audio::prepare_audio(&input_path, &output_path, overwrite)?;
            tracing::debug!(elapsed_ms = resample_started_at.elapsed().as_millis(), output = %prepared.path.display(), "Prepared transcription audio");
            crate::transcription::load_validated_audio(&prepared.path, prepared.metadata)?
        } else {
            bail!(crate::audio::render_transcription_contract_error(
                &metadata, &issues,
            ));
        };

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
        let max_decode_tokens = self
            .max_decode_tokens
            .unwrap_or(crate::whisper::DEFAULT_MAX_DECODE_TOKENS);
        tracing::debug!(
            max_decode_tokens,
            "Building Burn Whisper transcription request"
        );
        let request_started_at = Instant::now();
        let request = crate::transcription::build_transcription_request(
            effective_audio,
            Some(model),
            max_decode_tokens,
        );
        tracing::debug!(
            elapsed_ms = request_started_at.elapsed().as_millis(),
            mel_bins = request.features.n_mels,
            frames = request.features.n_frames,
            "Built Burn Whisper transcription request"
        );
        let backend = crate::transcription::BurnWhisperBackend::new(max_decode_tokens);
        let decode_started_at = Instant::now();
        tracing::debug!("Starting Burn Whisper decode");
        let result = crate::transcription::TranscriptionBackend::transcribe(&backend, &request)?;
        tracing::debug!(elapsed_ms = decode_started_at.elapsed().as_millis(), text = %result.text, diagnostics = ?result.diagnostics, "Finished Burn Whisper decode");
        tracing::debug!(
            elapsed_ms = command_started_at.elapsed().as_millis(),
            "Finished audio transcribe command"
        );

        println!("{}", result.text);
        Ok(CliOutput::none())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct VctkDemoClip {
    wav_path: PathBuf,
    expected_text: String,
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

fn find_vctk_demo_clip() -> eyre::Result<VctkDemoClip> {
    let roots = [
        PathBuf::from("g:/Datasets/VCTK/VCTK-Corpus-smaller"),
        PathBuf::from("G:/Datasets/VCTK/VCTK-Corpus-smaller"),
        PathBuf::from("./VCTK-Corpus-smaller"),
    ];
    for root in roots {
        if let Some(clip) = find_vctk_demo_clip_under(&root)? {
            return Ok(clip);
        }
    }
    bail!("Could not find a local VCTK-Corpus-smaller dataset for --demo")
}

fn find_vctk_demo_clip_under(root: &Path) -> eyre::Result<Option<VctkDemoClip>> {
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
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos() as usize);
    let index = seed % clips.len();
    Ok(clips.into_iter().nth(index))
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
