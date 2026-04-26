#![expect(warnings, reason = "ported experimental Burn Whisper prototype")]

use eyre::{Context, ContextCompat, bail};
use facet::Facet;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const TRANSCRIPTION_SAMPLE_RATE: u32 = 16_000;
pub const TRANSCRIPTION_CHANNELS: u16 = 1;
pub const TRANSCRIPTION_CONTAINER: &str = "wav";
pub const TRANSCRIPTION_CODEC_PREFIX: &str = "pcm_";

#[derive(Clone, Debug, PartialEq)]
pub struct AudioMetadata {
    pub path: PathBuf,
    pub container: Option<String>,
    pub codec: Option<String>,
    pub sample_rate_hz: Option<u32>,
    pub channels: Option<u16>,
    pub bits_per_sample: Option<u16>,
    pub duration_seconds: Option<f64>,
}

impl AudioMetadata {
    #[must_use]
    pub fn is_wav_container(&self) -> bool {
        self.container.as_deref().is_some_and(|container| {
            container
                .split(',')
                .map(str::trim)
                .any(|part| part.eq_ignore_ascii_case(TRANSCRIPTION_CONTAINER))
        })
    }

    #[must_use]
    pub fn is_pcm_codec(&self) -> bool {
        self.codec.as_deref().is_some_and(|codec| {
            codec.eq_ignore_ascii_case("pcm")
                || codec
                    .to_ascii_lowercase()
                    .starts_with(TRANSCRIPTION_CODEC_PREFIX)
        })
    }

    #[must_use]
    pub fn display_container(&self) -> &str {
        self.container.as_deref().unwrap_or("unknown")
    }

    #[must_use]
    pub fn display_codec(&self) -> &str {
        self.codec.as_deref().unwrap_or("unknown")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AudioIssue {
    UnsupportedContainer {
        actual: Option<String>,
        expected: &'static str,
    },
    UnsupportedCodec {
        actual: Option<String>,
        expected_prefix: &'static str,
    },
    MissingSampleRate,
    WrongSampleRate {
        actual_hz: u32,
        expected_hz: u32,
    },
    MissingChannels,
    WrongChannelCount {
        actual: u16,
        expected: u16,
    },
}

impl AudioIssue {
    #[must_use]
    pub fn render(&self) -> String {
        match self {
            AudioIssue::UnsupportedContainer { actual, expected } => {
                format!(
                    "unsupported container: expected {expected}, found {}",
                    actual.as_deref().unwrap_or("unknown")
                )
            }
            AudioIssue::UnsupportedCodec {
                actual,
                expected_prefix,
            } => format!(
                "unsupported audio codec: expected {expected_prefix}*, found {}",
                actual.as_deref().unwrap_or("unknown")
            ),
            AudioIssue::MissingSampleRate => "sample rate is missing".to_owned(),
            AudioIssue::WrongSampleRate {
                actual_hz,
                expected_hz,
            } => format!(
                "incorrect sample rate: expected {} Hz, found {} Hz",
                expected_hz, actual_hz
            ),
            AudioIssue::MissingChannels => "channel count is missing".to_owned(),
            AudioIssue::WrongChannelCount { actual, expected } => {
                format!("incorrect channel count: expected {expected}, found {actual}")
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedAudio {
    pub path: PathBuf,
    pub metadata: AudioMetadata,
}

#[must_use]
pub fn validate_for_transcription(metadata: &AudioMetadata) -> Vec<AudioIssue> {
    let mut issues = Vec::new();

    if !metadata.is_wav_container() {
        issues.push(AudioIssue::UnsupportedContainer {
            actual: metadata.container.clone(),
            expected: TRANSCRIPTION_CONTAINER,
        });
    }

    if !metadata.is_pcm_codec() {
        issues.push(AudioIssue::UnsupportedCodec {
            actual: metadata.codec.clone(),
            expected_prefix: TRANSCRIPTION_CODEC_PREFIX,
        });
    }

    match metadata.sample_rate_hz {
        Some(sample_rate_hz) if sample_rate_hz != TRANSCRIPTION_SAMPLE_RATE => {
            issues.push(AudioIssue::WrongSampleRate {
                actual_hz: sample_rate_hz,
                expected_hz: TRANSCRIPTION_SAMPLE_RATE,
            });
        }
        Some(_) => {}
        None => issues.push(AudioIssue::MissingSampleRate),
    }

    match metadata.channels {
        Some(channels) if channels != TRANSCRIPTION_CHANNELS => {
            issues.push(AudioIssue::WrongChannelCount {
                actual: channels,
                expected: TRANSCRIPTION_CHANNELS,
            });
        }
        Some(_) => {}
        None => issues.push(AudioIssue::MissingChannels),
    }

    issues
}

#[must_use]
pub fn render_inspection_report(metadata: &AudioMetadata, issues: &[AudioIssue]) -> String {
    let mut lines = vec![
        format!("Path: {}", metadata.path.display()),
        format!("Container: {}", metadata.display_container()),
        format!("Codec: {}", metadata.display_codec()),
        format!(
            "Sample rate: {}",
            metadata
                .sample_rate_hz
                .map_or_else(|| "unknown".to_owned(), |value| format!("{value} Hz"))
        ),
        format!(
            "Channels: {}",
            metadata
                .channels
                .map_or_else(|| "unknown".to_owned(), |value| value.to_string())
        ),
        format!(
            "Bits per sample: {}",
            metadata
                .bits_per_sample
                .map_or_else(|| "unknown".to_owned(), |value| value.to_string())
        ),
        format!(
            "Duration: {}",
            metadata
                .duration_seconds
                .map_or_else(|| "unknown".to_owned(), |value| format!("{value:.3} s"))
        ),
    ];

    if issues.is_empty() {
        lines.push(format!(
            "Transcription contract: PASS ({} Hz mono PCM WAV)",
            TRANSCRIPTION_SAMPLE_RATE
        ));
    } else {
        lines.push(format!(
            "Transcription contract: FAIL ({} Hz mono PCM WAV)",
            TRANSCRIPTION_SAMPLE_RATE
        ));
        lines.push("Issues:".to_owned());
        lines.extend(issues.iter().map(|issue| format!("- {}", issue.render())));
    }

    lines.join("\n")
}

#[must_use]
pub fn render_transcription_contract_error(
    metadata: &AudioMetadata,
    issues: &[AudioIssue],
) -> String {
    format!(
        "Input audio is not ready for transcription.\n{}\nUse `teamy-studio audio transcribe <path> --resample --model-dir <dir>` to explicitly create a 16 kHz mono PCM WAV artifact before transcription.",
        render_inspection_report(metadata, issues)
    )
}

/// Inspect an audio file.
///
/// Prefers `ffprobe` for broad format support, with a WAV-only fallback when `ffprobe`
/// is unavailable.
///
/// # Errors
///
/// This function will return an error if the file does not exist or metadata cannot be read.
pub fn inspect_audio(path: &Path) -> eyre::Result<AudioMetadata> {
    ensure_existing_file(path)?;

    match inspect_audio_with_ffprobe(path) {
        Ok(metadata) => Ok(metadata),
        Err(ffprobe_error) if is_wav_path(path) => inspect_wav(path).wrap_err_with(|| {
            format!(
                "Failed to inspect audio with ffprobe and WAV fallback for {}: {ffprobe_error}",
                path.display()
            )
        }),
        Err(ffprobe_error) => Err(ffprobe_error),
    }
}

/// Prepare audio into a 16 kHz mono PCM WAV artifact.
///
/// # Errors
///
/// This function will return an error if input probing fails, the output path is invalid,
/// `ffmpeg` cannot be invoked, or the prepared artifact is still not compliant.
pub fn prepare_audio(input: &Path, output: &Path, overwrite: bool) -> eyre::Result<PreparedAudio> {
    ensure_existing_file(input)?;

    if input == output {
        bail!("Refusing to overwrite input in-place: {}", input.display());
    }

    if output.exists() && !overwrite {
        bail!(
            "Output already exists: {}. Re-run with --overwrite to replace it.",
            output.display()
        );
    }

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).wrap_err_with(|| {
            format!("Failed to create output directory for {}", output.display())
        })?;
    }

    let mut command = Command::new("ffmpeg");
    command.arg("-hide_banner").arg("-loglevel").arg("error");
    if overwrite {
        command.arg("-y");
    } else {
        command.arg("-n");
    }
    command
        .arg("-i")
        .arg(input)
        .arg("-ac")
        .arg(TRANSCRIPTION_CHANNELS.to_string())
        .arg("-ar")
        .arg(TRANSCRIPTION_SAMPLE_RATE.to_string())
        .arg("-c:a")
        .arg("pcm_s16le")
        .arg(output);

    let command_output = command
        .output()
        .wrap_err("Failed to launch ffmpeg for explicit audio preparation")?;

    if !command_output.status.success() {
        let stderr = String::from_utf8_lossy(&command_output.stderr);
        bail!(
            "ffmpeg failed while preparing audio.\nInput: {}\nOutput: {}\n{}",
            input.display(),
            output.display(),
            stderr.trim()
        );
    }

    let metadata = inspect_audio(output)?;
    let issues = validate_for_transcription(&metadata);
    if !issues.is_empty() {
        bail!(
            "Prepared audio is still not compliant.\n{}",
            render_inspection_report(&metadata, &issues)
        );
    }

    Ok(PreparedAudio {
        path: output.to_path_buf(),
        metadata,
    })
}

#[must_use]
pub fn default_prepared_output_path(input: &Path) -> PathBuf {
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    let stem = input
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("audio");
    parent.join(format!("{stem}.16khz-mono.wav"))
}

fn ensure_existing_file(path: &Path) -> eyre::Result<()> {
    if !path.exists() {
        bail!("Audio file does not exist: {}", path.display());
    }
    if !path.is_file() {
        bail!("Audio path is not a file: {}", path.display());
    }
    Ok(())
}

fn is_wav_path(path: &Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
}

fn inspect_wav(path: &Path) -> eyre::Result<AudioMetadata> {
    let reader = hound::WavReader::open(path)
        .wrap_err_with(|| format!("Failed to open WAV file {}", path.display()))?;
    let spec = reader.spec();

    let codec = match spec.sample_format {
        hound::SampleFormat::Float => Some("pcm_f32le".to_owned()),
        hound::SampleFormat::Int => Some("pcm".to_owned()),
    };

    Ok(AudioMetadata {
        path: path.to_path_buf(),
        container: Some("wav".to_owned()),
        codec,
        sample_rate_hz: Some(spec.sample_rate),
        channels: Some(spec.channels),
        bits_per_sample: Some(spec.bits_per_sample),
        duration_seconds: None,
    })
}

fn inspect_audio_with_ffprobe(path: &Path) -> eyre::Result<AudioMetadata> {
    let command_output = Command::new("ffprobe")
        .arg("-v")
        .arg("quiet")
        .arg("-print_format")
        .arg("json")
        .arg("-show_format")
        .arg("-show_streams")
        .arg(path)
        .output()
        .wrap_err("Failed to launch ffprobe for audio inspection")?;

    if !command_output.status.success() {
        let stderr = String::from_utf8_lossy(&command_output.stderr);
        bail!(
            "ffprobe failed while inspecting {}. {}",
            path.display(),
            stderr.trim()
        );
    }

    let stdout = String::from_utf8(command_output.stdout)
        .wrap_err("ffprobe output was not valid UTF-8 JSON")?;
    let parsed: FfprobeOutput =
        facet_json::from_str(&stdout).wrap_err("Failed to parse ffprobe JSON output")?;

    let audio_stream = parsed
        .streams
        .into_iter()
        .find(|stream| stream.codec_type.as_deref() == Some("audio"))
        .wrap_err_with(|| format!("No audio stream found in {}", path.display()))?;

    let duration_seconds = audio_stream
        .duration
        .as_deref()
        .and_then(parse_number::<f64>)
        .or_else(|| {
            parsed
                .format
                .as_ref()
                .and_then(|format| format.duration.as_deref())
                .and_then(parse_number::<f64>)
        });

    Ok(AudioMetadata {
        path: path.to_path_buf(),
        container: parsed.format.and_then(|format| format.format_name),
        codec: audio_stream.codec_name,
        sample_rate_hz: audio_stream
            .sample_rate
            .as_deref()
            .and_then(parse_number::<u32>),
        channels: audio_stream.channels,
        bits_per_sample: audio_stream.bits_per_sample.or_else(|| {
            audio_stream
                .bits_per_raw_sample
                .as_deref()
                .and_then(parse_number::<u16>)
        }),
        duration_seconds,
    })
}

fn parse_number<T>(value: &str) -> Option<T>
where
    T: std::str::FromStr,
{
    value.parse().ok()
}

#[derive(Debug, Facet)]
struct FfprobeOutput {
    #[facet(default)]
    streams: Vec<FfprobeStream>,
    format: Option<FfprobeFormat>,
}

#[derive(Debug, Facet)]
struct FfprobeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u16>,
    bits_per_sample: Option<u16>,
    bits_per_raw_sample: Option<String>,
    duration: Option<String>,
}

#[derive(Debug, Facet)]
struct FfprobeFormat {
    format_name: Option<String>,
    duration: Option<String>,
}
