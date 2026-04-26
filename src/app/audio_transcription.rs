use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Command;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use eyre::Context;
use facet::Facet;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::{PipeMode, ServerOptions};
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_WRITE, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, PAGE_READWRITE,
    UnmapViewOfFile,
};

use crate::paths::CacheHome;
use crate::win32_support::string::EasyPCWSTR;

pub const WHISPER_LOG_MEL_BINS: usize = 80;
pub const WHISPER_LOG_MEL_FRAMES: usize = 3_000;
pub const WHISPER_LOG_MEL_VALUE_COUNT: usize = WHISPER_LOG_MEL_BINS * WHISPER_LOG_MEL_FRAMES;
pub const WHISPER_LOG_MEL_BYTE_COUNT: usize = WHISPER_LOG_MEL_VALUE_COUNT * size_of::<f32>();
pub const WHISPER_LOG_MEL_DTYPE: &str = "f32-le";
pub const WHISPER_LOG_MEL_WINDOW_SECONDS: u32 = 30;
pub const WHISPER_AUDIO_SAMPLE_RATE_HZ: u32 = 16_000;
pub const WHISPER_AUDIO_DTYPE: &str = "f32-le";
pub const WHISPER_DAEMON_SOURCE_PARENT_DIR: &str = "python";
pub const WHISPER_DAEMON_SOURCE_DIR_NAME: &str = "whisperx-daemon";
pub const WHISPER_SHARED_MEMORY_MINIMUM_SLOTS: usize = 3;
pub const WHISPER_CONTROL_PROTOCOL_VERSION: u32 = 1;
pub const PYTORCH_CUDA_INDEX_URL: &str = "https://download.pytorch.org/whl/cu128";
pub const WHISPER_TRANSCRIPTION_MODELS: [&str; 6] = [
    "large-v3",
    "large-v2",
    "medium.en",
    "small.en",
    "base.en",
    "tiny.en",
];

static SELECTED_WHISPER_MODEL_INDEX: AtomicUsize = AtomicUsize::new(0);

#[must_use]
pub fn audio_transcription_named_pipe_path(pipe_name: &str) -> String {
    format!(r"\\.\pipe\{pipe_name}")
}

#[must_use]
// audio[impl transcription.model-selection]
pub fn audio_transcription_available_model_names() -> Vec<String> {
    WHISPER_TRANSCRIPTION_MODELS
        .iter()
        .map(|model| (*model).to_owned())
        .collect()
}

#[must_use]
// audio[impl transcription.model-selection]
pub fn audio_transcription_selected_model_index() -> usize {
    SELECTED_WHISPER_MODEL_INDEX
        .load(Ordering::Acquire)
        .min(WHISPER_TRANSCRIPTION_MODELS.len() - 1)
}

#[must_use]
// audio[impl transcription.model-selection]
pub fn audio_transcription_selected_model_name() -> String {
    WHISPER_TRANSCRIPTION_MODELS[audio_transcription_selected_model_index()].to_owned()
}

// audio[impl transcription.model-selection]
pub fn audio_transcription_select_model_index(index: usize) {
    SELECTED_WHISPER_MODEL_INDEX.store(
        index.min(WHISPER_TRANSCRIPTION_MODELS.len() - 1),
        Ordering::Release,
    );
}

#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "audio feature prep maps bounded sample and frame counts into normalized f32 bins"
)]
// audio[impl transcription.sample-derived-handoff]
pub fn audio_transcription_prepare_handoff_tensor_from_samples(
    samples: &[f32],
    sample_rate_hz: u32,
) -> WhisperLogMel80x3000 {
    #[cfg(feature = "tracy")]
    let _span = tracing::debug_span!("prepare_transcription_handoff_tensor_from_samples").entered();
    if samples.is_empty() || sample_rate_hz == 0 {
        return WhisperLogMel80x3000::zeros();
    }
    let target_sample_count = usize::try_from(sample_rate_hz)
        .unwrap_or(48_000)
        .saturating_mul(usize::try_from(WHISPER_LOG_MEL_WINDOW_SECONDS).unwrap_or(30));
    let frames_to_fill = ((samples.len().saturating_mul(WHISPER_LOG_MEL_FRAMES))
        / target_sample_count.max(1))
    .clamp(1, WHISPER_LOG_MEL_FRAMES);
    let mut values = vec![0.0_f32; WHISPER_LOG_MEL_VALUE_COUNT];
    for frame in 0..frames_to_fill {
        let start = (frame * samples.len()) / frames_to_fill;
        let end = (((frame + 1) * samples.len()) / frames_to_fill)
            .max(start + 1)
            .min(samples.len());
        let chunk = &samples[start..end];
        let rms = chunk_rms(chunk);
        let peak = chunk
            .iter()
            .map(|sample| sample.abs())
            .fold(0.0_f32, f32::max);
        for bin in 0..WHISPER_LOG_MEL_BINS {
            let mel_position = (bin + 1) as f32 / WHISPER_LOG_MEL_BINS as f32;
            let ripple = ((frame as f32 * 0.013) + (bin as f32 * 0.071)).sin().abs();
            let energy = ((rms * 0.76) + (peak * 0.24))
                * (0.42 + (mel_position.sqrt() * 0.58))
                * (0.72 + ripple * 0.28);
            values[(bin * WHISPER_LOG_MEL_FRAMES) + frame] = (energy + 1.0e-6).ln();
        }
    }
    WhisperLogMel80x3000 {
        values: values.into_boxed_slice(),
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "RMS energy divides by a bounded chunk sample count"
)]
fn chunk_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum = samples
        .iter()
        .map(|sample| sample.clamp(-1.0, 1.0).powi(2))
        .sum::<f32>();
    (sum / samples.len() as f32).sqrt()
}

#[must_use]
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "audio resampling maps bounded sample-rate ratios into sample indices"
)]
// audio[impl transcription.real-python-inference]
pub fn audio_transcription_resample_mono_to_16khz(
    samples: &[f32],
    sample_rate_hz: u32,
) -> Vec<f32> {
    if samples.is_empty() || sample_rate_hz == 0 {
        return Vec::new();
    }
    if sample_rate_hz == WHISPER_AUDIO_SAMPLE_RATE_HZ {
        return samples
            .iter()
            .map(|sample| sample.clamp(-1.0, 1.0))
            .collect();
    }
    let output_len = ((samples.len() as u64) * u64::from(WHISPER_AUDIO_SAMPLE_RATE_HZ)
        / u64::from(sample_rate_hz))
    .max(1) as usize;
    let mut output = Vec::with_capacity(output_len);
    for output_index in 0..output_len {
        let source_position = output_index as f64 * f64::from(sample_rate_hz)
            / f64::from(WHISPER_AUDIO_SAMPLE_RATE_HZ);
        let source_index = source_position.floor() as usize;
        let next_index = (source_index + 1).min(samples.len() - 1);
        let fraction = (source_position - source_index as f64) as f32;
        let sample = samples[source_index].mul_add(1.0 - fraction, samples[next_index] * fraction);
        output.push(sample.clamp(-1.0, 1.0));
    }
    output
}

#[must_use]
// audio[impl transcription.real-python-inference]
pub fn audio_transcription_f32_samples_to_little_endian_bytes(samples: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len().saturating_mul(size_of::<f32>()));
    for sample in samples.iter().copied() {
        bytes.extend_from_slice(&sample.clamp(-1.0, 1.0).to_le_bytes());
    }
    bytes
}

#[derive(Clone, Debug, PartialEq)]
pub struct WhisperLogMel80x3000 {
    values: Box<[f32]>,
}

impl WhisperLogMel80x3000 {
    #[must_use]
    // audio[impl transcription.log-mel-contract]
    pub fn zeros() -> Self {
        Self {
            values: vec![0.0; WHISPER_LOG_MEL_VALUE_COUNT].into_boxed_slice(),
        }
    }

    /// # Errors
    ///
    /// This function will return an error if `values` does not match the fixed Whisper tensor
    /// shape used by the Python daemon contract.
    // audio[impl transcription.log-mel-contract]
    pub fn from_vec(values: Vec<f32>) -> eyre::Result<Self> {
        if values.len() != WHISPER_LOG_MEL_VALUE_COUNT {
            eyre::bail!(
                "expected {WHISPER_LOG_MEL_VALUE_COUNT} log-mel values, got {}",
                values.len()
            );
        }
        Ok(Self {
            values: values.into_boxed_slice(),
        })
    }

    #[must_use]
    pub fn values(&self) -> &[f32] {
        &self.values
    }

    #[must_use]
    // audio[impl transcription.shared-memory-payload]
    pub fn to_little_endian_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(WHISPER_LOG_MEL_BYTE_COUNT);
        for value in self.values.iter().copied() {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}

#[derive(Clone, Debug, Facet, PartialEq, Eq)]
pub struct AudioTranscriptionDaemonStatusReport {
    pub daemon_source_dir: String,
    pub uv_venv_dir: String,
    pub model_cache_dir: String,
    pub tensor_dtype: String,
    pub tensor_mel_bins: usize,
    pub tensor_frames: usize,
    pub tensor_values: usize,
    pub tensor_bytes: usize,
    pub shared_memory_slot_bytes: usize,
    pub shared_memory_minimum_slots: usize,
    pub shared_memory_total_bytes: usize,
    pub queued_request_count: usize,
    pub oldest_queued_age_ms: u64,
    pub python_lag_ms: u64,
    pub control_transport: String,
    pub payload_transport: String,
    pub python_entrypoint: String,
    pub selected_model: String,
    pub available_models: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioTranscriptionSharedMemorySlotPoolStatus {
    pub slot_count: usize,
    pub total_bytes: usize,
    pub queued_request_count: usize,
    pub oldest_queued_age_ms: u64,
    pub python_lag_ms: u64,
}

impl AudioTranscriptionSharedMemorySlotPoolStatus {
    #[must_use]
    pub const fn initial() -> Self {
        Self {
            slot_count: WHISPER_SHARED_MEMORY_MINIMUM_SLOTS,
            total_bytes: WHISPER_LOG_MEL_BYTE_COUNT * WHISPER_SHARED_MEMORY_MINIMUM_SLOTS,
            queued_request_count: 0,
            oldest_queued_age_ms: 0,
            python_lag_ms: 0,
        }
    }
}

#[derive(Clone, Debug, Facet, PartialEq, Eq)]
pub struct AudioTranscriptionQueuedRequest {
    pub request_id: u64,
    pub slot_id: u64,
    pub slot_name: String,
    pub byte_len: usize,
}

#[derive(Clone, Debug, Facet, PartialEq, Eq)]
pub struct AudioTranscriptionControlRequest {
    pub protocol_version: u32,
    pub kind: String,
    pub request_id: u64,
    pub slot_id: u64,
    pub slot_name: String,
    pub byte_len: usize,
    pub tensor_dtype: String,
    pub tensor_mel_bins: usize,
    pub tensor_frames: usize,
}

#[derive(Clone, Debug, Facet, PartialEq, Eq)]
pub struct AudioTranscriptionControlResult {
    pub protocol_version: u32,
    pub kind: String,
    pub request_id: u64,
    pub slot_id: u64,
    pub release_slot: bool,
    pub ok: bool,
    pub transcript_text: String,
    pub error: Option<String>,
}

#[must_use]
// audio[impl transcription.named-pipe-control-protocol]
pub fn audio_transcription_control_request_for_queued_request(
    request: &AudioTranscriptionQueuedRequest,
) -> AudioTranscriptionControlRequest {
    AudioTranscriptionControlRequest {
        protocol_version: WHISPER_CONTROL_PROTOCOL_VERSION,
        kind: "transcribe-log-mel".to_owned(),
        request_id: request.request_id,
        slot_id: request.slot_id,
        slot_name: request.slot_name.clone(),
        byte_len: request.byte_len,
        tensor_dtype: WHISPER_LOG_MEL_DTYPE.to_owned(),
        tensor_mel_bins: WHISPER_LOG_MEL_BINS,
        tensor_frames: WHISPER_LOG_MEL_FRAMES,
    }
}

#[must_use]
// audio[impl transcription.real-python-inference]
pub fn audio_transcription_control_request_for_queued_audio_request(
    request: &AudioTranscriptionQueuedRequest,
) -> AudioTranscriptionControlRequest {
    AudioTranscriptionControlRequest {
        protocol_version: WHISPER_CONTROL_PROTOCOL_VERSION,
        kind: "transcribe-audio-f32".to_owned(),
        request_id: request.request_id,
        slot_id: request.slot_id,
        slot_name: request.slot_name.clone(),
        byte_len: request.byte_len,
        tensor_dtype: WHISPER_AUDIO_DTYPE.to_owned(),
        tensor_mel_bins: 1,
        tensor_frames: request.byte_len / size_of::<f32>(),
    }
}

/// # Errors
///
/// Returns an error if the request cannot be serialized as a JSONL control message.
// audio[impl transcription.named-pipe-control-protocol]
pub fn encode_audio_transcription_control_request_line(
    request: &AudioTranscriptionControlRequest,
) -> eyre::Result<String> {
    let mut line = facet_json::to_string(request)
        .wrap_err("failed to serialize audio transcription control request")?;
    line.push('\n');
    Ok(line)
}

/// # Errors
///
/// Returns an error if the line is not a valid daemon result control message.
// audio[impl transcription.named-pipe-control-protocol]
pub fn decode_audio_transcription_control_result_line(
    line: &str,
) -> eyre::Result<AudioTranscriptionControlResult> {
    let result: AudioTranscriptionControlResult = facet_json::from_str(line.trim_end())
        .wrap_err("failed to parse audio transcription control result")?;
    validate_audio_transcription_control_result(&result)?;
    Ok(result)
}

fn validate_audio_transcription_control_result(
    result: &AudioTranscriptionControlResult,
) -> eyre::Result<()> {
    if result.protocol_version != WHISPER_CONTROL_PROTOCOL_VERSION {
        eyre::bail!(
            "unsupported audio transcription control protocol version: {}",
            result.protocol_version
        );
    }
    if result.kind != "transcription-result" {
        eyre::bail!(
            "unsupported audio transcription control result kind: {}",
            result.kind
        );
    }
    Ok(())
}

/// # Errors
///
/// Returns an error if the pipe cannot be created, the daemon does not connect, or the daemon
/// returns an invalid result line.
// audio[impl transcription.live-named-pipe-transport]
pub async fn audio_transcription_named_pipe_request_roundtrip(
    pipe_path: &str,
    request: &AudioTranscriptionControlRequest,
) -> eyre::Result<AudioTranscriptionControlResult> {
    let mut server = ServerOptions::new()
        .pipe_mode(PipeMode::Byte)
        .create(pipe_path)
        .wrap_err_with(|| format!("failed to create audio transcription named pipe {pipe_path}"))?;
    server
        .connect()
        .await
        .wrap_err("failed to accept audio transcription daemon pipe client")?;
    let request_line = encode_audio_transcription_control_request_line(request)?;
    server
        .write_all(request_line.as_bytes())
        .await
        .wrap_err("failed to write transcription control request")?;
    server
        .flush()
        .await
        .wrap_err("failed to flush transcription control request")?;

    let mut reader = BufReader::new(server);
    let mut result_line = String::new();
    reader
        .read_line(&mut result_line)
        .await
        .wrap_err("failed to read transcription control result")?;
    if result_line.is_empty() {
        eyre::bail!("audio transcription daemon closed the named pipe without a result");
    }
    decode_audio_transcription_control_result_line(&result_line)
}

/// # Errors
///
/// Returns an error if the shared-memory slot cannot be created, the local Python daemon process
/// cannot be launched, or the debug pipe request does not complete successfully.
// audio[impl transcription.debug-runtime-tick]
pub fn audio_transcription_run_python_debug_request_once(
    request_id: u64,
    transcript_text: &str,
) -> eyre::Result<AudioTranscriptionControlResult> {
    audio_transcription_run_python_debug_request_once_with_tensor(
        request_id,
        &WhisperLogMel80x3000::zeros(),
        transcript_text,
    )
}

/// # Errors
///
/// Returns an error if the shared-memory slot cannot be created, the local Python daemon process
/// cannot be launched, or the debug pipe request does not complete successfully.
// audio[impl transcription.debug-runtime-tick]
pub fn audio_transcription_run_python_debug_request_once_with_tensor(
    request_id: u64,
    tensor: &WhisperLogMel80x3000,
    transcript_text: &str,
) -> eyre::Result<AudioTranscriptionControlResult> {
    let daemon_source_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(WHISPER_DAEMON_SOURCE_PARENT_DIR)
        .join(WHISPER_DAEMON_SOURCE_DIR_NAME);
    let pipe_path = audio_transcription_named_pipe_path(&format!(
        "TeamyStudioAudioTranscriptionDebug-{}-{request_id}",
        std::process::id()
    ));
    let mut pool = AudioTranscriptionSharedMemorySlotPool::new(1)?;
    let queued_request = pool.enqueue_tensor(request_id, tensor)?;
    let control_request = audio_transcription_control_request_for_queued_request(&queued_request);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .wrap_err("failed to start debug transcription runtime")?;
    let roundtrip = audio_transcription_named_pipe_request_roundtrip(&pipe_path, &control_request);
    let mut child = Command::new("python")
        .arg("-m")
        .arg("teamy_whisperx_daemon")
        .arg("--connect-pipe-once")
        .arg(&pipe_path)
        .arg("--validate-shared-memory-slot")
        .arg("--debug-transcript-text")
        .arg(transcript_text)
        .current_dir(&daemon_source_dir)
        .spawn()
        .wrap_err("failed to launch debug transcription daemon")?;
    let result = runtime
        .block_on(async { tokio::time::timeout(Duration::from_secs(10), roundtrip).await })
        .wrap_err("debug transcription daemon timed out")??;
    let child_status = child
        .wait()
        .wrap_err("failed to wait for debug transcription daemon")?;
    if !child_status.success() {
        eyre::bail!("debug transcription daemon exited with {child_status}");
    }
    if result.release_slot {
        pool.release_slot(result.slot_id);
    }
    Ok(result)
}

/// # Errors
///
/// Returns an error if no audio samples are available, the shared-memory slot cannot be created,
/// the managed Python daemon cannot be launched, or the transcription request times out.
#[expect(
    clippy::duration_suboptimal_units,
    reason = "the first real transcription run may install dependencies and download a model"
)]
// audio[impl transcription.real-python-inference]
pub fn audio_transcription_run_python_transcription_request_once_from_samples(
    request_id: u64,
    samples: &[f32],
    sample_rate_hz: u32,
) -> eyre::Result<AudioTranscriptionControlResult> {
    let resampled_samples = audio_transcription_resample_mono_to_16khz(samples, sample_rate_hz);
    if resampled_samples.is_empty() {
        eyre::bail!("no audio samples are available for transcription");
    }
    let payload = audio_transcription_f32_samples_to_little_endian_bytes(&resampled_samples);
    let daemon_source_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(WHISPER_DAEMON_SOURCE_PARENT_DIR)
        .join(WHISPER_DAEMON_SOURCE_DIR_NAME);
    let pipe_path = audio_transcription_named_pipe_path(&format!(
        "TeamyStudioAudioTranscription-{}-{request_id}",
        std::process::id()
    ));
    let mut slot = AudioTranscriptionSharedMemorySlot::new_with_byte_len(
        request_id,
        "AudioF32",
        payload.len(),
    )?;
    slot.write_payload(&payload)?;
    slot.queued_at = Some(Instant::now());
    let queued_request = AudioTranscriptionQueuedRequest {
        request_id,
        slot_id: slot.id,
        slot_name: slot.name.clone(),
        byte_len: slot.byte_len,
    };
    let control_request =
        audio_transcription_control_request_for_queued_audio_request(&queued_request);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .wrap_err("failed to start transcription runtime")?;
    let roundtrip = audio_transcription_named_pipe_request_roundtrip(&pipe_path, &control_request);
    let mut child = Command::new("uv")
        .arg("run")
        .arg("--no-project")
        .arg("--index")
        .arg(PYTORCH_CUDA_INDEX_URL)
        .arg("--with")
        .arg("numpy")
        .arg("--with")
        .arg("openai-whisper")
        .arg("python")
        .arg("-m")
        .arg("teamy_whisperx_daemon")
        .arg("--connect-pipe-once")
        .arg(&pipe_path)
        .arg("--validate-shared-memory-slot")
        .arg("--transcribe-shared-memory-slot")
        .arg("--model")
        .arg(audio_transcription_selected_model_name())
        .current_dir(&daemon_source_dir)
        .spawn()
        .wrap_err("failed to launch transcription daemon through uv")?;
    let result = runtime
        .block_on(async { tokio::time::timeout(Duration::from_secs(900), roundtrip).await })
        .wrap_err("transcription daemon timed out")??;
    let child_status = child
        .wait()
        .wrap_err("failed to wait for transcription daemon")?;
    if !child_status.success() {
        eyre::bail!("transcription daemon exited with {child_status}");
    }
    Ok(result)
}

/// # Errors
///
/// Returns an error if the Python daemon cannot be launched or the CUDA check exits with failure.
// audio[impl transcription.cuda-check]
pub fn audio_transcription_run_python_cuda_check() -> eyre::Result<String> {
    let daemon_source_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(WHISPER_DAEMON_SOURCE_PARENT_DIR)
        .join(WHISPER_DAEMON_SOURCE_DIR_NAME);
    let output = Command::new("uv")
        .arg("run")
        .arg("--no-project")
        .arg("--index")
        .arg(PYTORCH_CUDA_INDEX_URL)
        .arg("--with")
        .arg("torch")
        .arg("--with")
        .arg("numpy")
        .arg("python")
        .arg("-m")
        .arg("teamy_whisperx_daemon")
        .arg("--cuda-check")
        .current_dir(&daemon_source_dir)
        .output()
        .wrap_err("failed to launch CUDA check through uv")?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if !output.status.success() {
        eyre::bail!("CUDA check exited with {}\n{}", output.status, stderr);
    }
    Ok(if stderr.is_empty() {
        stdout
    } else {
        format!("{stdout}\n\n{stderr}")
    })
}

#[derive(Debug)]
// audio[impl transcription.shared-memory-slot-pool]
pub struct AudioTranscriptionSharedMemorySlotPool {
    slots: Vec<AudioTranscriptionSharedMemorySlot>,
    ready_queue: VecDeque<AudioTranscriptionQueuedRequest>,
    next_slot_id: u64,
}

impl AudioTranscriptionSharedMemorySlotPool {
    /// # Errors
    ///
    /// Returns an error if Windows cannot create or map the initial shared-memory slots.
    // audio[impl transcription.shared-memory-slot-pool]
    pub fn new(minimum_slots: usize) -> eyre::Result<Self> {
        let minimum_slots = minimum_slots.max(WHISPER_SHARED_MEMORY_MINIMUM_SLOTS);
        let mut pool = Self {
            slots: Vec::with_capacity(minimum_slots),
            ready_queue: VecDeque::new(),
            next_slot_id: 0,
        };
        for _ in 0..minimum_slots {
            pool.allocate_slot()?;
        }
        Ok(pool)
    }

    /// # Errors
    ///
    /// Returns an error if an elastic slot must be allocated and Windows cannot create or map it.
    // audio[impl transcription.shared-memory-slot-pool]
    pub fn enqueue_tensor(
        &mut self,
        request_id: u64,
        tensor: &WhisperLogMel80x3000,
    ) -> eyre::Result<AudioTranscriptionQueuedRequest> {
        let slot_index = self.next_available_slot_index()?;
        let slot = &mut self.slots[slot_index];
        slot.write_tensor(tensor)?;
        slot.queued_at = Some(Instant::now());
        let request = AudioTranscriptionQueuedRequest {
            request_id,
            slot_id: slot.id,
            slot_name: slot.name.clone(),
            byte_len: slot.byte_len,
        };
        self.ready_queue.push_back(request.clone());
        Ok(request)
    }

    #[must_use]
    pub fn next_ready_request(&self) -> Option<&AudioTranscriptionQueuedRequest> {
        self.ready_queue.front()
    }

    #[must_use]
    // audio[impl transcription.shared-memory-pool-status]
    pub fn status(&self) -> AudioTranscriptionSharedMemorySlotPoolStatus {
        let now = Instant::now();
        let oldest_queued_age_ms = self
            .slots
            .iter()
            .filter_map(|slot| slot.queued_at)
            .map(|queued_at| duration_millis_u64(now.duration_since(queued_at)))
            .max()
            .unwrap_or_default();
        AudioTranscriptionSharedMemorySlotPoolStatus {
            slot_count: self.slots.len(),
            total_bytes: self.slots.iter().map(|slot| slot.byte_len).sum(),
            queued_request_count: self.ready_queue.len(),
            oldest_queued_age_ms,
            python_lag_ms: oldest_queued_age_ms,
        }
    }

    pub fn release_slot(&mut self, slot_id: u64) {
        self.ready_queue
            .retain(|request| request.slot_id != slot_id);
        if let Some(slot) = self.slots.iter_mut().find(|slot| slot.id == slot_id) {
            slot.queued_at = None;
        }
    }

    fn next_available_slot_index(&mut self) -> eyre::Result<usize> {
        if let Some(index) = self.slots.iter().position(|slot| slot.queued_at.is_none()) {
            return Ok(index);
        }
        self.allocate_slot()?;
        Ok(self.slots.len() - 1)
    }

    fn allocate_slot(&mut self) -> eyre::Result<()> {
        let slot = AudioTranscriptionSharedMemorySlot::new(self.next_slot_id)?;
        self.next_slot_id += 1;
        self.slots.push(slot);
        Ok(())
    }
}

#[derive(Debug)]
struct AudioTranscriptionSharedMemorySlot {
    id: u64,
    name: String,
    byte_len: usize,
    _mapping: FileMappingHandle,
    view: MappedView,
    queued_at: Option<Instant>,
}

impl AudioTranscriptionSharedMemorySlot {
    fn new(id: u64) -> eyre::Result<Self> {
        Self::new_with_byte_len(id, "WhisperLogMel", WHISPER_LOG_MEL_BYTE_COUNT)
    }

    fn new_with_byte_len(id: u64, name_prefix: &str, byte_len: usize) -> eyre::Result<Self> {
        let name = format!(
            "Local\\TeamyStudio{name_prefix}-{}-{id}",
            std::process::id()
        );
        let wide_name = name.as_str().easy_pcwstr()?;
        let low_size = u32::try_from(byte_len)
            .wrap_err("Whisper shared-memory slot is too large for Win32 mapping size")?;
        // Safety: this creates a page-file-backed mapping with a valid UTF-16 name.
        let handle = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                None,
                PAGE_READWRITE,
                0,
                low_size,
                wide_name.as_ref(),
            )
        }
        .wrap_err("failed to create Whisper shared-memory slot")?;
        let mapping = FileMappingHandle(handle);
        // Safety: the mapping handle is valid and the requested view length matches the slot size.
        let view = unsafe { MapViewOfFile(mapping.0, FILE_MAP_WRITE, 0, 0, byte_len) };
        let view = MappedView::new(view).wrap_err("failed to map Whisper shared-memory slot")?;
        Ok(Self {
            id,
            name,
            byte_len,
            _mapping: mapping,
            view,
            queued_at: None,
        })
    }

    fn write_tensor(&mut self, tensor: &WhisperLogMel80x3000) -> eyre::Result<()> {
        let payload = tensor.to_little_endian_bytes();
        self.write_payload(&payload)
    }

    fn write_payload(&mut self, payload: &[u8]) -> eyre::Result<()> {
        if payload.len() != self.byte_len {
            eyre::bail!(
                "Whisper payload was {} bytes, expected {}",
                payload.len(),
                self.byte_len
            );
        }
        self.view
            .as_mut_slice(self.byte_len)
            .copy_from_slice(payload);
        Ok(())
    }
}

#[derive(Debug)]
struct FileMappingHandle(HANDLE);

impl Drop for FileMappingHandle {
    fn drop(&mut self) {
        // Safety: this handle is owned by `FileMappingHandle` and is closed exactly once here.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

#[derive(Debug)]
struct MappedView(MEMORY_MAPPED_VIEW_ADDRESS);

impl MappedView {
    fn new(address: MEMORY_MAPPED_VIEW_ADDRESS) -> eyre::Result<Self> {
        NonNull::new(address.Value).map_or_else(
            || Err(windows::core::Error::from_thread()).wrap_err("MapViewOfFile returned null"),
            |_| Ok(Self(address)),
        )
    }

    fn as_mut_slice(&mut self, byte_len: usize) -> &mut [u8] {
        // Safety: `MappedView` owns a live writable view of at least `byte_len` bytes.
        unsafe { std::slice::from_raw_parts_mut(self.0.Value.cast::<u8>(), byte_len) }
    }
}

impl Drop for MappedView {
    fn drop(&mut self) {
        // Safety: this view is owned by `MappedView` and is unmapped exactly once here.
        let _ = unsafe { UnmapViewOfFile(self.0) };
    }
}

#[must_use]
// audio[impl cli.daemon-status]
// audio[impl python.daemon-project]
// audio[impl transcription.shared-memory-payload]
pub fn audio_transcription_daemon_status(
    cache_home: &CacheHome,
) -> AudioTranscriptionDaemonStatusReport {
    audio_transcription_daemon_status_with_pool_status(
        cache_home,
        &AudioTranscriptionSharedMemorySlotPoolStatus::initial(),
    )
}

#[must_use]
pub fn audio_transcription_daemon_status_with_pool_status(
    cache_home: &CacheHome,
    pool_status: &AudioTranscriptionSharedMemorySlotPoolStatus,
) -> AudioTranscriptionDaemonStatusReport {
    let daemon_source_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(WHISPER_DAEMON_SOURCE_PARENT_DIR)
        .join(WHISPER_DAEMON_SOURCE_DIR_NAME);
    let daemon_cache_dir = cache_home.join("python").join("whisperx-daemon");
    AudioTranscriptionDaemonStatusReport {
        daemon_source_dir: daemon_source_dir.display().to_string(),
        uv_venv_dir: daemon_cache_dir.join(".venv").display().to_string(),
        model_cache_dir: cache_home
            .join("models")
            .join("whisperx")
            .display()
            .to_string(),
        tensor_dtype: WHISPER_LOG_MEL_DTYPE.to_owned(),
        tensor_mel_bins: WHISPER_LOG_MEL_BINS,
        tensor_frames: WHISPER_LOG_MEL_FRAMES,
        tensor_values: WHISPER_LOG_MEL_VALUE_COUNT,
        tensor_bytes: WHISPER_LOG_MEL_BYTE_COUNT,
        shared_memory_slot_bytes: WHISPER_LOG_MEL_BYTE_COUNT,
        // audio[impl transcription.shared-memory-pool-status]
        shared_memory_minimum_slots: pool_status.slot_count,
        shared_memory_total_bytes: pool_status.total_bytes,
        queued_request_count: pool_status.queued_request_count,
        oldest_queued_age_ms: pool_status.oldest_queued_age_ms,
        python_lag_ms: pool_status.python_lag_ms,
        control_transport: "windows named pipe".to_owned(),
        payload_transport: "rust-owned shared-memory slot".to_owned(),
        python_entrypoint: "teamy_whisperx_daemon".to_owned(),
        selected_model: audio_transcription_selected_model_name(),
        available_models: audio_transcription_available_model_names(),
    }
}

fn duration_millis_u64(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // audio[verify transcription.log-mel-contract]
    fn whisper_log_mel_contract_has_fixed_shape() {
        let tensor = WhisperLogMel80x3000::zeros();

        assert_eq!(tensor.values().len(), 240_000);
        assert_eq!(WHISPER_LOG_MEL_BYTE_COUNT, 960_000);
    }

    #[test]
    // audio[verify transcription.shared-memory-payload]
    fn whisper_log_mel_payload_is_little_endian_f32_bytes() {
        let mut values = vec![0.0; WHISPER_LOG_MEL_VALUE_COUNT];
        values[0] = 1.0;
        values[1] = -2.5;
        let tensor = WhisperLogMel80x3000::from_vec(values).expect("tensor should be fixed shape");

        let bytes = tensor.to_little_endian_bytes();

        assert_eq!(bytes.len(), WHISPER_LOG_MEL_BYTE_COUNT);
        assert_eq!(&bytes[0..4], &1.0_f32.to_le_bytes());
        assert_eq!(&bytes[4..8], &(-2.5_f32).to_le_bytes());
    }

    #[test]
    // audio[verify transcription.sample-derived-handoff]
    fn sample_derived_handoff_fills_fixed_tensor_from_samples() {
        let samples = vec![0.25_f32; 48_000];

        let tensor = audio_transcription_prepare_handoff_tensor_from_samples(&samples, 48_000);

        assert_eq!(tensor.values().len(), WHISPER_LOG_MEL_VALUE_COUNT);
        assert!(tensor.values().iter().any(|value| *value < 0.0));
        assert!(
            tensor.values()[WHISPER_LOG_MEL_FRAMES..]
                .iter()
                .any(|value| *value != 0.0)
        );
    }

    #[test]
    fn sample_derived_handoff_pads_short_chunks() {
        let samples = vec![0.5_f32; 4_800];

        let tensor = audio_transcription_prepare_handoff_tensor_from_samples(&samples, 48_000);

        assert!(tensor.values()[0] < 0.0);
        assert_eq!(tensor.values()[WHISPER_LOG_MEL_FRAMES - 1], 0.0);
    }

    #[test]
    fn whisper_log_mel_rejects_wrong_value_count() {
        let error =
            WhisperLogMel80x3000::from_vec(vec![0.0; 12]).expect_err("wrong length should fail");

        assert!(error.to_string().contains("expected 240000"));
    }

    #[test]
    // audio[verify transcription.shared-memory-pool-status]
    fn daemon_status_reports_initial_shared_memory_pool_metrics() {
        let report = audio_transcription_daemon_status(&CacheHome(PathBuf::from("cache")));

        assert_eq!(
            report.shared_memory_minimum_slots,
            WHISPER_SHARED_MEMORY_MINIMUM_SLOTS
        );
        assert_eq!(report.shared_memory_slot_bytes, WHISPER_LOG_MEL_BYTE_COUNT);
        assert_eq!(
            report.shared_memory_total_bytes,
            WHISPER_LOG_MEL_BYTE_COUNT * WHISPER_SHARED_MEMORY_MINIMUM_SLOTS
        );
        assert_eq!(report.queued_request_count, 0);
    }

    #[test]
    // audio[verify transcription.shared-memory-slot-pool]
    fn shared_memory_slot_pool_queues_and_releases_tensor_payloads() {
        let mut pool = AudioTranscriptionSharedMemorySlotPool::new(3)
            .expect("shared-memory pool should initialize");
        let request = pool
            .enqueue_tensor(42, &WhisperLogMel80x3000::zeros())
            .expect("tensor should enqueue into a shared-memory slot");

        assert_eq!(request.request_id, 42);
        assert_eq!(request.byte_len, WHISPER_LOG_MEL_BYTE_COUNT);
        assert_eq!(pool.next_ready_request(), Some(&request));
        assert_eq!(pool.status().queued_request_count, 1);

        pool.release_slot(request.slot_id);

        assert_eq!(pool.next_ready_request(), None);
        assert_eq!(pool.status().queued_request_count, 0);
    }

    #[test]
    fn shared_memory_slot_pool_grows_when_all_slots_are_queued() {
        let mut pool = AudioTranscriptionSharedMemorySlotPool::new(3)
            .expect("shared-memory pool should initialize");
        for request_id in 0..4 {
            pool.enqueue_tensor(request_id, &WhisperLogMel80x3000::zeros())
                .expect("tensor should enqueue into a shared-memory slot");
        }

        let status = pool.status();

        assert_eq!(status.slot_count, 4);
        assert_eq!(status.queued_request_count, 4);
        assert_eq!(status.total_bytes, WHISPER_LOG_MEL_BYTE_COUNT * 4);
    }

    #[test]
    // audio[verify transcription.named-pipe-control-protocol]
    fn control_request_line_reports_queued_shared_memory_slot() {
        let queued = AudioTranscriptionQueuedRequest {
            request_id: 7,
            slot_id: 3,
            slot_name: "Local\\TeamyStudioWhisperLogMel-test".to_owned(),
            byte_len: WHISPER_LOG_MEL_BYTE_COUNT,
        };
        let request = audio_transcription_control_request_for_queued_request(&queued);

        let line = encode_audio_transcription_control_request_line(&request)
            .expect("request should serialize as JSONL");

        assert!(line.ends_with('\n'));
        assert!(line.contains("\"kind\":\"transcribe-log-mel\""));
        assert!(line.contains("\"slot_name\":\"Local\\\\TeamyStudioWhisperLogMel-test\""));
    }

    #[test]
    // audio[verify transcription.real-python-inference]
    fn audio_control_request_reports_16khz_f32_payload_shape() {
        let queued = AudioTranscriptionQueuedRequest {
            request_id: 17,
            slot_id: 2,
            slot_name: "Local\\TeamyStudioAudioF32-test".to_owned(),
            byte_len: 32_000 * size_of::<f32>(),
        };

        let request = audio_transcription_control_request_for_queued_audio_request(&queued);

        assert_eq!(request.kind, "transcribe-audio-f32");
        assert_eq!(request.tensor_dtype, WHISPER_AUDIO_DTYPE);
        assert_eq!(request.tensor_mel_bins, 1);
        assert_eq!(request.tensor_frames, 32_000);
    }

    #[test]
    fn audio_resampler_outputs_16khz_mono_samples() {
        let samples = vec![0.25_f32; 48_000];

        let resampled = audio_transcription_resample_mono_to_16khz(&samples, 48_000);

        assert_eq!(resampled.len(), 16_000);
        assert!(
            resampled
                .iter()
                .all(|sample| (*sample - 0.25).abs() < f32::EPSILON)
        );
    }

    #[test]
    fn audio_sample_payload_is_little_endian_f32() {
        let payload = audio_transcription_f32_samples_to_little_endian_bytes(&[1.0, -0.5]);

        assert_eq!(&payload[0..4], &1.0_f32.to_le_bytes());
        assert_eq!(&payload[4..8], &(-0.5_f32).to_le_bytes());
    }

    #[test]
    fn control_result_line_validates_protocol_version_and_kind() {
        let line = r#"{"protocol_version":1,"kind":"transcription-result","request_id":7,"slot_id":3,"release_slot":true,"ok":true,"transcript_text":"hello","error":null}"#;

        let result = decode_audio_transcription_control_result_line(line)
            .expect("result should parse from daemon JSONL");

        assert_eq!(result.request_id, 7);
        assert!(result.release_slot);
        assert_eq!(result.transcript_text, "hello");
    }

    #[test]
    // audio[verify transcription.live-named-pipe-transport]
    fn named_pipe_roundtrip_exchanges_request_and_result_lines() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .expect("tokio runtime should start");
        runtime.block_on(async {
            use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
            use tokio::net::windows::named_pipe::ClientOptions;

            let pipe_path = audio_transcription_named_pipe_path(&format!(
                "TeamyStudioAudioTranscriptionTest-{}",
                std::process::id()
            ));
            let queued = AudioTranscriptionQueuedRequest {
                request_id: 11,
                slot_id: 5,
                slot_name: "Local\\TeamyStudioWhisperLogMel-test".to_owned(),
                byte_len: WHISPER_LOG_MEL_BYTE_COUNT,
            };
            let request = audio_transcription_control_request_for_queued_request(&queued);
            let client_pipe_path = pipe_path.clone();
            let client_task = tokio::spawn(async move {
                let client = loop {
                    match ClientOptions::new().open(&client_pipe_path) {
                        Ok(client) => break client,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            tokio::task::yield_now().await;
                        }
                        Err(error) => return Err(eyre::Report::new(error)),
                    }
                };
                let mut reader = BufReader::new(client);
                let mut request_line = String::new();
                reader.read_line(&mut request_line).await?;
                assert!(request_line.contains("\"request_id\":11"));
                let result = AudioTranscriptionControlResult {
                    protocol_version: WHISPER_CONTROL_PROTOCOL_VERSION,
                    kind: "transcription-result".to_owned(),
                    request_id: 11,
                    slot_id: 5,
                    release_slot: true,
                    ok: true,
                    transcript_text: "pipe ok".to_owned(),
                    error: None,
                };
                let mut client = reader.into_inner();
                client
                    .write_all(facet_json::to_string(&result)?.as_bytes())
                    .await?;
                client.write_all(b"\n").await?;
                client.flush().await?;
                Ok::<(), eyre::Report>(())
            });

            let result = audio_transcription_named_pipe_request_roundtrip(&pipe_path, &request)
                .await
                .expect("named-pipe roundtrip should finish");
            client_task
                .await
                .expect("client task should join")
                .expect("client task should succeed");

            assert_eq!(result.transcript_text, "pipe ok");
            assert!(result.release_slot);
        });
    }

    #[test]
    // audio[verify transcription.live-named-pipe-transport]
    fn python_debug_daemon_roundtrip_validates_real_shared_memory_slot() {
        let python_daemon_dir = std::env::current_dir()
            .expect("current dir should be readable")
            .join("python")
            .join("whisperx-daemon");
        if !python_daemon_dir.exists() {
            return;
        }

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .expect("tokio runtime should start");
        runtime.block_on(async {
            let pipe_path = audio_transcription_named_pipe_path(&format!(
                "TeamyStudioAudioTranscriptionPythonTest-{}",
                std::process::id()
            ));
            let mut pool = AudioTranscriptionSharedMemorySlotPool::new(1)
                .expect("shared-memory pool should be created");
            pool.enqueue_tensor(23, &WhisperLogMel80x3000::zeros())
                .expect("tensor should enqueue into shared memory");
            let queued = pool
                .next_ready_request()
                .expect("queued request should be available");
            let request = audio_transcription_control_request_for_queued_request(&queued);

            let roundtrip = audio_transcription_named_pipe_request_roundtrip(&pipe_path, &request);
            let mut child = std::process::Command::new("python")
                .arg("-m")
                .arg("teamy_whisperx_daemon")
                .arg("--connect-pipe-once")
                .arg(&pipe_path)
                .arg("--validate-shared-memory-slot")
                .current_dir(&python_daemon_dir)
                .spawn()
                .expect("python daemon smoke process should start");

            let result = tokio::time::timeout(std::time::Duration::from_secs(10), roundtrip)
                .await
                .expect("python daemon pipe smoke should not time out")
                .expect("python daemon pipe smoke should finish");
            let child_status = child.wait().expect("python daemon process should finish");

            assert!(child_status.success());
            assert_eq!(result.request_id, 23);
            assert_eq!(result.slot_id, queued.slot_id);
            assert!(result.release_slot);
            pool.release_slot(result.slot_id);
            assert_eq!(pool.status().queued_request_count, 0);
        });
    }

    #[test]
    // audio[verify transcription.debug-runtime-tick]
    fn python_debug_request_runner_returns_requested_transcript_text() {
        let python_daemon_dir = std::env::current_dir()
            .expect("current dir should be readable")
            .join("python")
            .join("whisperx-daemon");
        if !python_daemon_dir.exists() {
            return;
        }

        let result = audio_transcription_run_python_debug_request_once(71, "debug text from app")
            .expect("debug transcription request should finish");

        assert_eq!(result.request_id, 71);
        assert!(result.release_slot);
        assert_eq!(result.transcript_text, "debug text from app");
    }
}
