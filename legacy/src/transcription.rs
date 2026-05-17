#![expect(warnings, reason = "ported experimental Burn Whisper prototype")]

use crate::audio::{
    AudioMetadata, TRANSCRIPTION_CHANNELS, TRANSCRIPTION_SAMPLE_RATE, validate_for_transcription,
};
use crate::frontend::{WhisperLogMelSpectrogram, whisper_log_mel_spectrogram};
use crate::model::WhisperModelArtifacts;
use crate::whisper::{DecodeStopReason, GreedyDecodeSummary};
use eyre::{Context, ContextCompat, bail};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedAudio {
    pub path: PathBuf,
    pub metadata: AudioMetadata,
    pub samples: Vec<f32>,
}

impl ValidatedAudio {
    #[must_use]
    pub fn duration_seconds(&self) -> f64 {
        self.samples.len() as f64 / f64::from(TRANSCRIPTION_SAMPLE_RATE)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TranscriptionRequest {
    pub input: ValidatedAudio,
    pub features: WhisperLogMelSpectrogram,
    pub model: Option<WhisperModelArtifacts>,
    pub max_decode_tokens: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TranscriptionResult {
    pub text: String,
    pub diagnostics: Option<ModelTranscriptionDiagnostics>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelTranscriptionDiagnostics {
    pub prompt_token_ids: Vec<usize>,
    pub generated_token_ids: Vec<usize>,
    pub encoder_output_dims: [usize; 3],
    pub decoder_logits_dims: [usize; 3],
    pub terminated_on_end_of_text: bool,
    pub stop_reason: DecodeStopReason,
}

pub trait TranscriptionBackend {
    fn transcribe(&self, request: &TranscriptionRequest) -> eyre::Result<TranscriptionResult>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BurnWhisperBackend {
    pub max_decode_tokens: usize,
}

impl BurnWhisperBackend {
    #[must_use]
    pub fn new(max_decode_tokens: usize) -> Self {
        Self { max_decode_tokens }
    }
}

impl TranscriptionBackend for BurnWhisperBackend {
    fn transcribe(&self, request: &TranscriptionRequest) -> eyre::Result<TranscriptionResult> {
        tracing::debug!(
            samples = request.input.samples.len(),
            duration_seconds = request.input.duration_seconds(),
            mel_bins = request.features.n_mels,
            frames = request.features.n_frames,
            max_decode_tokens = self.max_decode_tokens.min(request.max_decode_tokens),
            "Burn Whisper backend received transcription request"
        );
        let model = request.model.as_ref().ok_or_else(|| {
            eyre::eyre!(
                "Burn Whisper backend requires a model directory. Loaded {} samples ({:.3} s) from {} and produced {}x{} Whisper frontend features, but no model was attached to the request.",
                request.input.samples.len(),
                request.input.duration_seconds(),
                request.input.path.display(),
                request.features.n_mels,
                request.features.n_frames,
            )
        })?;

        let started_at = Instant::now();
        let summary = crate::whisper::greedy_decode_with_model(
            model,
            &request.features,
            self.max_decode_tokens.min(request.max_decode_tokens),
        )?;
        tracing::debug!(elapsed_ms = started_at.elapsed().as_millis(), stop_reason = ?summary.stop_reason, generated_tokens = summary.generated_token_ids.len(), "Burn Whisper backend completed greedy decode");

        Ok(transcription_result_from_greedy_decode(summary))
    }
}

#[derive(Debug, Default)]
pub struct MissingBackend;

impl TranscriptionBackend for MissingBackend {
    fn transcribe(&self, request: &TranscriptionRequest) -> eyre::Result<TranscriptionResult> {
        bail!(
            "Transcription backend is not implemented yet. Loaded {} samples ({:.3} s) from {}, produced a {}x{} Whisper log-mel spectrogram, and validated the 16 kHz mono WAV contract. Model selected: {}. Provide `--model-dir` with a converted Teamy Studio Whisper model directory.",
            request.input.samples.len(),
            request.input.duration_seconds(),
            request.input.path.display(),
            request.features.n_mels,
            request.features.n_frames,
            request
                .model
                .as_ref()
                .map_or("none".to_owned(), |model| model.root.display().to_string()),
        );
    }
}

/// Load a WAV file that already satisfies the transcription contract.
///
/// # Errors
///
/// This function will return an error if the metadata is non-compliant or the WAV samples cannot be decoded.
pub fn load_validated_audio(path: &Path, metadata: AudioMetadata) -> eyre::Result<ValidatedAudio> {
    let started_at = Instant::now();
    let issues = validate_for_transcription(&metadata);
    if !issues.is_empty() {
        bail!(crate::audio::render_transcription_contract_error(
            &metadata, &issues,
        ));
    }

    let mut reader = hound::WavReader::open(path)
        .wrap_err_with(|| format!("Failed to open validated WAV file {}", path.display()))?;
    let spec = reader.spec();

    if spec.sample_rate != TRANSCRIPTION_SAMPLE_RATE {
        bail!(
            "Validated WAV loader expected {} Hz but found {} Hz in {}",
            TRANSCRIPTION_SAMPLE_RATE,
            spec.sample_rate,
            path.display()
        );
    }
    if spec.channels != TRANSCRIPTION_CHANNELS {
        bail!(
            "Validated WAV loader expected {} channel but found {} channels in {}",
            TRANSCRIPTION_CHANNELS,
            spec.channels,
            path.display()
        );
    }

    let sample_count = reader.duration() as usize;
    let samples = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<hound::Result<Vec<_>>>()
            .wrap_err_with(|| {
                format!("Failed to decode float WAV samples from {}", path.display())
            })?,
        hound::SampleFormat::Int => {
            let scale = integer_wav_scale(spec.bits_per_sample)?;
            reader
                .samples::<i32>()
                .map(|sample| sample.map(|value| value as f32 / scale))
                .collect::<hound::Result<Vec<_>>>()
                .wrap_err_with(|| {
                    format!("Failed to decode PCM WAV samples from {}", path.display())
                })?
        }
    };

    if samples.len() != sample_count {
        tracing::debug!(
            expected_sample_count = sample_count,
            decoded_sample_count = samples.len(),
            path = %path.display(),
            "Decoded sample count differed from WAV-reported duration"
        );
    }

    tracing::debug!(path = %path.display(), samples = samples.len(), elapsed_ms = started_at.elapsed().as_millis(), "Loaded validated transcription audio");
    Ok(ValidatedAudio {
        path: path.to_path_buf(),
        metadata,
        samples,
    })
}

fn integer_wav_scale(bits_per_sample: u16) -> eyre::Result<f32> {
    let exponent = u32::from(bits_per_sample)
        .checked_sub(1)
        .context("PCM WAV bits per sample must be at least 1")?;
    let scale = (1_i64)
        .checked_shl(exponent)
        .context("PCM WAV bits per sample is too large to scale safely")?;
    Ok(scale as f32)
}

#[must_use]
pub fn build_transcription_request(
    input: ValidatedAudio,
    model: Option<WhisperModelArtifacts>,
    max_decode_tokens: usize,
) -> TranscriptionRequest {
    let started_at = Instant::now();
    let features = whisper_log_mel_spectrogram(&input.samples);
    tracing::debug!(
        samples = input.samples.len(),
        mel_bins = features.n_mels,
        frames = features.n_frames,
        elapsed_ms = started_at.elapsed().as_millis(),
        "Computed Whisper log-mel frontend features"
    );
    TranscriptionRequest {
        input,
        features,
        model,
        max_decode_tokens,
    }
}

fn transcription_result_from_greedy_decode(summary: GreedyDecodeSummary) -> TranscriptionResult {
    TranscriptionResult {
        text: summary.text,
        diagnostics: Some(ModelTranscriptionDiagnostics {
            prompt_token_ids: summary.prompt_token_ids,
            generated_token_ids: summary.generated_token_ids,
            encoder_output_dims: summary.encoder_output_dims,
            decoder_logits_dims: summary.last_decoder_logits_dims,
            terminated_on_end_of_text: summary.terminated_on_end_of_text,
            stop_reason: summary.stop_reason,
        }),
    }
}
