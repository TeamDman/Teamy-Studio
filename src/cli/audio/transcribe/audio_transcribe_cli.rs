use arbitrary::Arbitrary;
use eyre::{Context, bail};
use facet::Facet;
use figue as args;
use std::path::PathBuf;

use crate::cli::output::CliOutput;

/// Transcribe a WAV file with the Rust Burn Whisper backend.
// audio[impl cli.transcribe-command]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[facet(rename_all = "kebab-case")]
pub struct AudioTranscribeArgs {
    /// Audio file to transcribe.
    #[facet(args::positional)]
    pub input: String,

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
        let _ = cache_home;

        let input_path = PathBuf::from(&self.input);
        let metadata = crate::audio::inspect_audio(&input_path)?;
        let issues = crate::audio::validate_for_transcription(&metadata);

        let effective_audio = if issues.is_empty() {
            crate::transcription::load_validated_audio(&input_path, metadata)?
        } else if self.resample {
            let output_path = self.prepared_output.map_or_else(
                || crate::audio::default_prepared_output_path(&input_path),
                PathBuf::from,
            );
            let prepared = crate::audio::prepare_audio(&input_path, &output_path, self.overwrite)?;
            crate::transcription::load_validated_audio(&prepared.path, prepared.metadata)?
        } else {
            bail!(crate::audio::render_transcription_contract_error(
                &metadata, &issues,
            ));
        };

        let model_dir = if let Some(model_dir) = self.model_dir.as_deref() {
            PathBuf::from(model_dir)
        } else {
            crate::model::resolve_default_model_dir(app_home)?.ok_or_else(|| {
                eyre::eyre!(
                    "No Burn Whisper model directory was provided and no default model is registered. Re-run with `--model-dir <dir>` pointing at a converted model directory containing tokenizer.json, model.bpk, and dims.json."
                )
            })?
        };
        let model = crate::model::inspect_model_dir(&model_dir).wrap_err_with(|| {
            format!(
                "failed to load Burn Whisper model from {}",
                model_dir.display()
            )
        })?;
        let max_decode_tokens = self
            .max_decode_tokens
            .unwrap_or(crate::whisper::DEFAULT_MAX_DECODE_TOKENS);
        let request = crate::transcription::build_transcription_request(
            effective_audio,
            Some(model),
            max_decode_tokens,
        );
        let backend = crate::transcription::BurnWhisperBackend::new(max_decode_tokens);
        let result = crate::transcription::TranscriptionBackend::transcribe(&backend, &request)?;

        println!("{}", result.text);
        Ok(CliOutput::none())
    }
}
