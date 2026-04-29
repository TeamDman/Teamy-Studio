use arbitrary::Arbitrary;
use eyre::Context;
use facet::Facet;
use std::ffi::c_void;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uom::si::f64::Time;
use uom::si::time::second;
use windows::Win32::Devices::Properties;
use windows::Win32::Foundation::{HWND, LPARAM, PROPERTYKEY, RPC_E_CHANGED_MODE, WPARAM};
use windows::Win32::Media::Audio::{
    AUDCLNT_SHAREMODE_SHARED, DEVICE_STATE_ACTIVE, ERole, IAudioCaptureClient, IAudioClient,
    IAudioRenderClient, IMMDevice, IMMDeviceCollection, IMMDeviceEnumerator, MMDeviceEnumerator,
    PlaySoundW, SND_ASYNC, SND_FILENAME, SND_FLAGS, SND_NODEFAULT, WAVEFORMATEX,
    WAVEFORMATEXTENSIBLE, eCapture, eRender,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Com::StructuredStorage::PropVariantClear;
use windows::Win32::System::Com::{
    CLSCTX_ALL, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize, STGM_READ,
};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::core::{GUID, PCWSTR};

use super::audio_transcription::{
    AudioTranscriptionControlResult, AudioTranscriptionSharedMemorySlotPool,
    audio_transcription_resample_mono_to_16khz, audio_transcription_selected_model_name,
};
use super::jobs;
use crate::audio::{AudioMetadata, TRANSCRIPTION_CHANNELS, TRANSCRIPTION_SAMPLE_RATE};
use crate::logs::ThreadBuilderSpanExt;
use crate::model::{WhisperModelArtifacts, inspect_model_dir, managed_model_dir};
use crate::paths::CacheHome;
use crate::transcription::{BurnWhisperBackend, TranscriptionBackend, build_transcription_request};
use crate::win32_support::string::EasyPCWSTR;

const GENERIC_WINDOWS_MIC_ICON_PATH: &str = "@%SystemRoot%\\system32\\mmres.dll,-3012";
const PKEY_DEVICE_ICON: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x259abffc_507a_4ce8_8c10_9640b8a1c907),
    pid: 10,
};
const PKEY_DEVICE_CLASS_ICON: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x259abffc_507a_4ce8_8c10_9640b8a1c907),
    pid: 12,
};
const AUDCLNT_BUFFERFLAGS_SILENT: u32 = 0x0000_0002;
const WAVE_FORMAT_PCM: u16 = 0x0001;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xfffe;
const KSDATAFORMAT_SUBTYPE_PCM: GUID = GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);
const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID =
    GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
const WASAPI_SHARED_BUFFER_100NS: i64 = 10_000_000;
const CAPTURE_POLL_INTERVAL: Duration = Duration::from_millis(10);
const PLAYBACK_SELECTION_TOLERANCE_PX: i32 = 2;
const PLAYBACK_STOP_FLAGS: SND_FLAGS = SND_FLAGS(0);
const TRANSCRIPTION_PREVIEW_COLUMNS: usize = 72;
const TRANSCRIPTION_PREVIEW_BINS: usize = 18;
const TRANSCRIPTION_PREVIEW_RECOMPUTE_INTERVAL: Duration = Duration::from_millis(250);
const TRANSCRIPTION_PREVIEW_MAX_SAMPLES_PER_CELL: usize = 12;
const TRANSCRIPTION_CHUNK_WINDOW_SECONDS: u32 = 30;

#[derive(Clone, Debug, Facet, PartialEq, Eq)]
pub struct AudioInputDeviceSummary {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub state: String,
    pub icon: String,
    pub sample_rate_hz: Option<u32>,
}

#[derive(Clone, Debug, Facet, PartialEq, Eq)]
pub struct AudioInputDeviceListReport {
    pub devices: Vec<AudioInputDeviceSummary>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioInputPickerKey {
    Up,
    Down,
    Tab,
    Enter,
    LegacyRecordingDevices,
    Escape,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AudioInputPickerState {
    pub selected_index: usize,
    pub devices: Vec<AudioInputDeviceSummary>,
}

#[derive(Debug)]
pub struct AudioInputDeviceWindowState {
    pub device: AudioInputDeviceSummary,
    pub armed_for_record: bool,
    pub loopback_enabled: bool,
    pub runtime: AudioInputRuntimeState,
    loopback_requested: Arc<AtomicBool>,
    capture_session: Option<AudioInputCaptureSession>,
    monitor_session: Option<AudioInputCaptureSession>,
    playback_file_path: Option<PathBuf>,
    transcription_worker: AudioInputTranscriptionWorkerState,
}

#[derive(Clone, Debug)]
pub struct RustTranscriptionResult {
    pub request_id: u64,
    pub ok: bool,
    pub transcript_text: String,
    pub error: Option<String>,
}

impl AudioInputDeviceWindowState {
    #[must_use]
    // audio[impl gui.selected-device-window]
    // audio[impl gui.arm-for-record]
    pub fn new(device: AudioInputDeviceSummary) -> Self {
        Self {
            device,
            armed_for_record: true,
            loopback_enabled: false,
            runtime: AudioInputRuntimeState::default(),
            loopback_requested: Arc::new(AtomicBool::new(false)),
            capture_session: None,
            monitor_session: None,
            playback_file_path: None,
            transcription_worker: AudioInputTranscriptionWorkerState::default(),
        }
    }

    #[must_use]
    // audio[impl gui.recording-state]
    pub fn is_recording(&self) -> bool {
        self.capture_session.is_some()
    }

    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.runtime.playback.started_at.is_some()
    }

    // audio[impl gui.recording-state]
    pub fn toggle_recording(&mut self) -> eyre::Result<()> {
        if self.is_recording() {
            return self.stop_recording();
        }
        self.start_recording()
    }

    // audio[impl gui.playback-transport]
    pub fn toggle_playback(&mut self) -> eyre::Result<()> {
        self.sync_playback_head();
        if self.is_playing() {
            self.pause_playback();
            return Ok(());
        }
        self.start_playback(1.0)
    }

    // audio[impl gui.transcription-toggle]
    pub fn toggle_transcription(&mut self) {
        self.runtime.transcription.enabled = !self.runtime.transcription.enabled;
        tracing::info!(
            enabled = self.runtime.transcription.enabled,
            device = %self.device.name,
            "Timeline transcription toggled"
        );
        if self.runtime.transcription.enabled {
            self.transcription_worker.reset_for_enabled_session();
            self.transcription_worker.flush_requested = true;
            self.runtime.transcription.last_sent_reason =
                Some("transcription enabled; chunk queued".to_owned());
        } else {
            self.runtime.transcription.last_sent_reason = Some("transcription disabled".to_owned());
        }
    }

    // audio[impl transcription.manual-flush]
    pub fn flush_transcription_chunk(&mut self) {
        self.runtime.transcription.enabled = true;
        self.runtime.refresh_transcription_preview_cache();
        self.transcription_worker.flush_requested = true;
        self.transcription_worker.debug_request_completed = false;
        self.runtime.transcription.last_sent_reason = Some("manual flush queued".to_owned());
    }

    pub fn set_transcription_model_name(&mut self, model_name: impl Into<String>) {
        self.runtime.transcription.selected_model_name = Some(model_name.into());
    }

    pub fn set_transcription_completion_notification_target(&mut self, hwnd: isize, message: u32) {
        self.transcription_worker.completion_notification =
            Some(AudioInputTranscriptionCompletionNotification { hwnd, message });
    }

    // audio[impl transcription.debug-runtime-tick]
    pub fn tick_debug_transcription_runtime(&mut self) {
        self.drain_debug_transcription_result();
        if !self.runtime.transcription.enabled
            || self.transcription_worker.request_in_flight
            || (self.transcription_worker.debug_request_completed
                && !self.transcription_worker.flush_requested)
        {
            return;
        }
        let request_id = self.transcription_worker.next_request_id;
        self.transcription_worker.next_request_id += 1;
        self.transcription_worker.request_in_flight = true;
        let flush_requested = self.transcription_worker.flush_requested;
        self.transcription_worker.flush_requested = false;
        let selected_model = self
            .runtime
            .transcription
            .selected_model_name
            .clone()
            .unwrap_or_else(audio_transcription_selected_model_name);
        self.runtime.transcription.last_sent_reason = Some(if flush_requested {
            format!("manual flush sent request {request_id} with {selected_model}")
        } else {
            format!("preview chunk sent request {request_id} with {selected_model}")
        });
        let (chunk_samples, sample_rate_hz) = self.runtime.transcription_chunk_samples();
        self.transcription_worker.sent_chunk_end_seconds = Some(
            self.runtime.transcription_head_seconds
                + seconds_from_samples(chunk_samples.len(), sample_rate_hz).get::<second>(),
        );
        if !flush_requested
            && chunk_samples.len() < usize::try_from(sample_rate_hz / 2).unwrap_or(0)
        {
            self.transcription_worker.request_in_flight = false;
            self.runtime.transcription.last_sent_reason =
                Some("waiting for audio chunk".to_owned());
            tracing::info!(
                request_id,
                samples = chunk_samples.len(),
                sample_rate_hz,
                "Timeline transcription is waiting for enough audio"
            );
            return;
        }
        tracing::info!(
            request_id,
            samples = chunk_samples.len(),
            sample_rate_hz,
            model = %selected_model,
            "Spawning Rust transcription worker"
        );
        let (sender, receiver) = mpsc::channel();
        self.transcription_worker.result_receiver = Some(receiver);
        let completion_notification = self.transcription_worker.completion_notification;
        let _ = thread::Builder::new()
            .name("teamy-studio-rust-transcription".to_owned())
            .spawn_with_current_span(move || {
                let result = run_rust_transcription_request_once_from_samples(
                    request_id,
                    selected_model,
                    chunk_samples,
                    sample_rate_hz,
                )
                .map_err(|error| error.to_string());
                let _ = sender.send(result);
                if let Some(notification) = completion_notification {
                    notification.post();
                }
            });
    }

    fn drain_debug_transcription_result(&mut self) {
        let Some(receiver) = self.transcription_worker.result_receiver.take() else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(result)) => {
                tracing::info!(
                    request_id = result.request_id,
                    ok = result.ok,
                    transcript_chars = result.transcript_text.chars().count(),
                    "Rust transcription worker completed"
                );
                self.transcription_worker.request_in_flight = false;
                self.transcription_worker.debug_request_completed = true;
                self.runtime.transcription.last_completed_request_id = Some(result.request_id);
                // audio[impl transcription.head-progress]
                if result.ok
                    && let Some(chunk_end_seconds) =
                        self.transcription_worker.sent_chunk_end_seconds.take()
                {
                    self.runtime.transcription_head_seconds =
                        clamp_head_seconds(chunk_end_seconds, self.runtime.duration_seconds());
                }
                if self.runtime.transcription.enabled {
                    if result.ok {
                        self.runtime
                            .transcription
                            .stage_transcript_text(&result.transcript_text);
                    } else {
                        let error = result
                            .error
                            .clone()
                            .unwrap_or_else(|| "transcription daemon returned an error".to_owned());
                        self.runtime.transcription.last_error = Some(error.clone());
                        self.runtime
                            .transcription
                            .stage_transcript_text(&format!("transcription failed: {error}"));
                    }
                }
            }
            Ok(Err(error)) => {
                tracing::error!(%error, "Rust transcription worker failed");
                self.transcription_worker.request_in_flight = false;
                self.transcription_worker.debug_request_completed = true;
                self.transcription_worker.sent_chunk_end_seconds = None;
                self.runtime.transcription.last_error = Some(error.clone());
                if self.runtime.transcription.enabled {
                    self.runtime
                        .transcription
                        .stage_transcript_text(&format!("transcription failed: {error}"));
                }
            }
            Err(TryRecvError::Empty) => {
                self.transcription_worker.result_receiver = Some(receiver);
            }
            Err(TryRecvError::Disconnected) => {
                self.transcription_worker.request_in_flight = false;
                self.transcription_worker.debug_request_completed = true;
                self.transcription_worker.sent_chunk_end_seconds = None;
                self.runtime.transcription.last_error =
                    Some("transcription path disconnected".to_owned());
                if self.runtime.transcription.enabled {
                    self.runtime
                        .transcription
                        .stage_transcript_text("transcription path disconnected");
                }
            }
        }
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "called by the daemon result loop in the next transcription slice"
        )
    )]
    // audio[impl transcription.result-staging]
    pub fn apply_transcription_result(
        &mut self,
        pool: &mut AudioTranscriptionSharedMemorySlotPool,
        result: &AudioTranscriptionControlResult,
    ) {
        if result.release_slot {
            pool.release_slot(result.slot_id);
        }
        if result.ok {
            self.runtime
                .transcription
                .stage_transcript_text(&result.transcript_text);
        }
    }

    pub fn pause_playback(&mut self) {
        self.sync_playback_head();
        stop_windows_playback();
        self.runtime.playback.started_at = None;
        self.runtime.playback.speed = 0.0;
    }

    pub fn toggle_loopback(&mut self) -> eyre::Result<()> {
        self.loopback_enabled = !self.loopback_enabled;
        self.loopback_requested
            .store(self.loopback_enabled, Ordering::Release);
        if !self.loopback_enabled {
            self.stop_loopback_monitor();
            return Ok(());
        }
        if !self.is_recording() {
            self.start_loopback_monitor()?;
        }
        Ok(())
    }

    pub fn playback_forward(&mut self) -> eyre::Result<()> {
        let next_speed = if self.runtime.playback.speed > 0.0 {
            (self.runtime.playback.speed + 1.0).min(8.0)
        } else {
            1.0
        };
        self.start_playback(next_speed)
    }

    pub fn playback_backward(&mut self) -> eyre::Result<()> {
        self.sync_playback_head();
        let next_speed = if self.runtime.playback.speed < 0.0 {
            (self.runtime.playback.speed - 1.0).max(-8.0)
        } else {
            -1.0
        };
        self.start_playback(next_speed)
    }

    pub fn sync_transport(&mut self) {
        self.sync_playback_head();
        self.runtime.refresh_transcription_preview_cache();
        self.tick_debug_transcription_runtime();
        self.runtime.recording_head_seconds = self.runtime.write_head_seconds();
    }

    pub fn set_head_seconds(&mut self, head: AudioInputTimelineHeadKind, seconds: f64) {
        match head {
            AudioInputTimelineHeadKind::Recording => {
                self.runtime.set_write_head_seconds(seconds);
                self.runtime.recording_head_seconds = self.runtime.write_head_seconds();
            }
            AudioInputTimelineHeadKind::Playback => {
                self.pause_playback();
                self.runtime.playback.head_seconds =
                    clamp_head_seconds(seconds, self.runtime.duration_seconds());
            }
            AudioInputTimelineHeadKind::Transcription => {
                self.runtime.transcription_head_seconds =
                    clamp_head_seconds(seconds, self.runtime.duration_seconds());
            }
        }
    }

    pub fn begin_head_interaction(
        &mut self,
        head: AudioInputTimelineHeadKind,
        pointer_seconds: f64,
    ) {
        let head_seconds = self.head_seconds(head);
        self.runtime.timeline_drag = Some(AudioInputTimelineDrag {
            interaction: AudioInputTimelineInteraction::Head(head),
            anchor_seconds: head_seconds,
            current_seconds: pointer_seconds,
            origin_x: 0,
            pointer_offset_seconds: pointer_seconds - head_seconds,
        });
    }

    #[must_use]
    pub fn head_seconds(&self, head: AudioInputTimelineHeadKind) -> f64 {
        match head {
            AudioInputTimelineHeadKind::Recording => self.runtime.recording_head_seconds,
            AudioInputTimelineHeadKind::Playback => self.runtime.playback.head_seconds,
            AudioInputTimelineHeadKind::Transcription => self.runtime.transcription_head_seconds,
        }
    }

    // audio[impl gui.waveform-selection]
    pub fn begin_timeline_interaction(&mut self, seconds: f64, x: i32) {
        self.runtime.timeline_drag = Some(AudioInputTimelineDrag {
            interaction: AudioInputTimelineInteraction::Selection,
            anchor_seconds: seconds,
            current_seconds: seconds,
            origin_x: x,
            pointer_offset_seconds: 0.0,
        });
        self.runtime.selection = None;
    }

    // audio[impl gui.waveform-selection]
    pub fn update_timeline_interaction(&mut self, seconds: f64) {
        let Some((interaction, anchor_seconds, pointer_offset_seconds)) =
            self.runtime.timeline_drag.as_mut().map(|drag| {
                drag.current_seconds = seconds;
                (
                    drag.interaction,
                    drag.anchor_seconds,
                    drag.pointer_offset_seconds,
                )
            })
        else {
            return;
        };
        match interaction {
            AudioInputTimelineInteraction::Selection => {
                self.runtime.selection = AudioInputSelection::new(anchor_seconds, seconds);
            }
            AudioInputTimelineInteraction::Head(head) => {
                self.set_head_seconds(head, seconds - pointer_offset_seconds);
            }
        }
    }

    // audio[impl gui.waveform-selection]
    pub fn complete_timeline_interaction(&mut self, seconds: f64, x: i32) -> eyre::Result<()> {
        let Some(drag) = self.runtime.timeline_drag.take() else {
            return Ok(());
        };
        if let AudioInputTimelineInteraction::Head(head) = drag.interaction {
            self.set_head_seconds(head, seconds - drag.pointer_offset_seconds);
            return Ok(());
        }
        if (x - drag.origin_x).abs() <= PLAYBACK_SELECTION_TOLERANCE_PX {
            let was_playing = self.is_playing();
            let speed = if self.runtime.playback.speed == 0.0 {
                1.0
            } else {
                self.runtime.playback.speed
            };
            self.pause_playback();
            self.runtime.playback.head_seconds = seconds;
            self.runtime.selection = None;
            if was_playing {
                self.start_playback(speed)?;
            }
            return Ok(());
        }
        self.runtime.selection = AudioInputSelection::new(drag.anchor_seconds, seconds);
        Ok(())
    }

    fn start_recording(&mut self) -> eyre::Result<()> {
        self.pause_playback();
        self.stop_loopback_monitor();
        self.runtime.clear_error();
        self.runtime.sync_write_head_from_recording_head();
        self.runtime.recording_head_seconds = self.runtime.write_head_seconds();
        let session = AudioInputCaptureSession::start(
            self.device.id.clone(),
            Arc::clone(&self.runtime.shared),
            Arc::clone(&self.loopback_requested),
            true,
        )?;
        self.capture_session = Some(session);
        Ok(())
    }

    fn stop_recording(&mut self) -> eyre::Result<()> {
        if let Some(session) = self.capture_session.take() {
            session.stop();
        }
        self.runtime.recording_head_seconds = self.runtime.write_head_seconds();
        if self.loopback_enabled {
            self.start_loopback_monitor()?;
        }
        Ok(())
    }

    fn start_loopback_monitor(&mut self) -> eyre::Result<()> {
        if self.monitor_session.is_some() {
            return Ok(());
        }
        self.runtime.clear_error();
        let session = AudioInputCaptureSession::start(
            self.device.id.clone(),
            Arc::clone(&self.runtime.shared),
            Arc::clone(&self.loopback_requested),
            false,
        )?;
        self.monitor_session = Some(session);
        Ok(())
    }

    fn stop_loopback_monitor(&mut self) {
        if let Some(session) = self.monitor_session.take() {
            session.stop();
        }
    }

    fn start_playback(&mut self, speed: f64) -> eyre::Result<()> {
        self.sync_playback_head();
        let samples = self.runtime.samples();
        if samples.is_empty() {
            return Ok(());
        }
        let duration_seconds = self.runtime.duration_seconds();
        let sample_rate = self.runtime.sample_rate_hz();
        let start_head_seconds = playback_start_head_seconds(
            self.runtime.playback.head_seconds,
            duration_seconds,
            speed,
        );
        self.runtime.playback.head_seconds = start_head_seconds;
        let playback_samples =
            playback_samples_from_head(&samples, sample_rate, start_head_seconds, speed);
        if playback_samples.is_empty() {
            self.runtime.playback.started_at = None;
            self.runtime.playback.speed = 0.0;
            return Ok(());
        }
        stop_windows_playback();
        let playback_path = write_audio_input_playback_wav(sample_rate, &playback_samples)?;
        let wide_path = playback_path.as_path().easy_pcwstr()?;
        // Safety: the path guard owns a valid nul-terminated UTF-16 string for this call.
        let result = unsafe {
            PlaySoundW(
                wide_path.as_ref(),
                None,
                SND_ASYNC | SND_FILENAME | SND_NODEFAULT,
            )
        };
        if !result.as_bool() {
            eyre::bail!("failed to play recorded audio buffer")
        }
        self.playback_file_path = Some(playback_path);
        self.runtime.playback.started_at = Some(Instant::now());
        self.runtime.playback.speed = speed;
        Ok(())
    }

    fn sync_playback_head(&mut self) {
        let Some(started_at) = self.runtime.playback.started_at else {
            return;
        };
        let elapsed = started_at.elapsed().as_secs_f64() * self.runtime.playback.speed.abs();
        if self.runtime.playback.speed < 0.0 {
            self.runtime.playback.head_seconds =
                (self.runtime.playback.head_seconds - elapsed).max(0.0);
            if self.runtime.playback.head_seconds <= 0.0 {
                self.runtime.playback.started_at = None;
                self.runtime.playback.speed = 0.0;
            } else {
                self.runtime.playback.started_at = Some(Instant::now());
            }
            return;
        }
        self.runtime.playback.head_seconds =
            (self.runtime.playback.head_seconds + elapsed).min(self.runtime.duration_seconds());
        if self.runtime.playback.head_seconds >= self.runtime.duration_seconds() {
            self.runtime.playback.started_at = None;
            self.runtime.playback.speed = 0.0;
        } else {
            self.runtime.playback.started_at = Some(Instant::now());
        }
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "worker thread owns the sample chunk and model name while the UI thread keeps running"
)]
fn run_rust_transcription_request_once_from_samples(
    request_id: u64,
    selected_model: String,
    samples: Vec<f32>,
    sample_rate_hz: u32,
) -> eyre::Result<RustTranscriptionResult> {
    let job = jobs::start_job(
        "Rust transcription chunk",
        format!("request {request_id}: resampling audio for {selected_model}"),
    );
    let result = run_rust_transcription_request_once_from_samples_inner(
        request_id,
        &selected_model,
        &samples,
        sample_rate_hz,
        &job,
    );
    match &result {
        Ok(result) if result.ok => job.complete(format!(
            "request {request_id}: decoded {} characters with Rust Whisper",
            result.transcript_text.chars().count()
        )),
        Ok(result) => job.fail(result.error.clone().unwrap_or_else(|| {
            format!("request {request_id}: Rust Whisper returned no transcript")
        })),
        Err(error) => job.fail(format!("request {request_id}: {error}")),
    }
    result
}

fn run_rust_transcription_request_once_from_samples_inner(
    request_id: u64,
    selected_model: &str,
    samples: &[f32],
    sample_rate_hz: u32,
    job: &jobs::JobHandle,
) -> eyre::Result<RustTranscriptionResult> {
    let _span = tracing::info_span!(
        "rust_live_transcription_chunk",
        request_id,
        selected_model,
        sample_rate_hz,
        samples = samples.len()
    )
    .entered();
    let resampled_samples = audio_transcription_resample_mono_to_16khz(samples, sample_rate_hz);
    if resampled_samples.is_empty() {
        eyre::bail!("no audio samples are available for transcription");
    }

    job.update(format!(
        "request {request_id}: loading Rust model artifacts for {selected_model}"
    ));
    let model = load_live_transcription_model(selected_model)?;

    job.update(format!(
        "request {request_id}: building Whisper frontend features"
    ));
    let duration_seconds =
        seconds_from_samples(resampled_samples.len(), TRANSCRIPTION_SAMPLE_RATE).get::<second>();
    let input = crate::transcription::ValidatedAudio {
        path: PathBuf::from(format!("live-audio-request-{request_id}.wav")),
        metadata: AudioMetadata {
            path: PathBuf::from(format!("live-audio-request-{request_id}.wav")),
            container: Some("wav".to_owned()),
            codec: Some("pcm_f32le".to_owned()),
            sample_rate_hz: Some(TRANSCRIPTION_SAMPLE_RATE),
            channels: Some(TRANSCRIPTION_CHANNELS),
            bits_per_sample: Some(32),
            duration_seconds: Some(duration_seconds),
        },
        samples: resampled_samples,
    };
    let request = build_transcription_request(input, Some(model), 96);

    job.update(format!(
        "request {request_id}: sending chunk through the Rust ML pipeline"
    ));
    let backend = BurnWhisperBackend::new(96);
    let result = backend.transcribe(&request)?;
    Ok(RustTranscriptionResult {
        request_id,
        ok: true,
        transcript_text: result.text,
        error: None,
    })
}

fn load_live_transcription_model(selected_model: &str) -> eyre::Result<WhisperModelArtifacts> {
    let model_dir = managed_model_dir(
        &CacheHome(crate::paths::CACHE_DIR.0.clone()),
        selected_model,
    );
    inspect_model_dir(&model_dir).wrap_err_with(|| {
        format!(
            "failed to inspect Rust Whisper model directory {}; prepare the model before live transcription",
            model_dir.display()
        )
    })
}

impl Drop for AudioInputDeviceWindowState {
    fn drop(&mut self) {
        let _ = self.stop_recording();
        stop_windows_playback();
    }
}

#[derive(Clone, Debug)]
pub struct AudioInputRuntimeState {
    shared: Arc<Mutex<AudioInputSharedBuffer>>,
    pub playback: AudioInputPlaybackState,
    pub transcription: AudioInputTranscriptionState,
    pub transcription_head_seconds: f64,
    pub recording_head_seconds: f64,
    pub selection: Option<AudioInputSelection>,
    pub timeline_drag: Option<AudioInputTimelineDrag>,
}

impl Default for AudioInputRuntimeState {
    fn default() -> Self {
        Self {
            shared: Arc::new(Mutex::new(AudioInputSharedBuffer::default())),
            playback: AudioInputPlaybackState::default(),
            transcription: AudioInputTranscriptionState::default(),
            transcription_head_seconds: 0.0,
            recording_head_seconds: 0.0,
            selection: None,
            timeline_drag: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioInputTranscriptionState {
    pub enabled: bool,
    pub selected_model_name: Option<String>,
    pub staged_text: String,
    pub preview: AudioInputMelSpectrogramPreview,
    pub chunk_seconds: f64,
    pub energy_rms: f32,
    pub last_sent_reason: Option<String>,
    pub last_completed_request_id: Option<u64>,
    pub last_error: Option<String>,
    preview_last_updated_at: Option<Instant>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioInputMelSpectrogramPreview {
    pub columns: usize,
    pub bins: usize,
    pub intensities: Vec<f32>,
    pub cache_key: AudioInputMelSpectrogramPreviewCacheKey,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioInputMelSpectrogramPreviewCacheKey {
    pub sample_count: usize,
    pub sample_rate_hz: u32,
    pub head_millis: u64,
    pub columns: usize,
    pub bins: usize,
}

struct AudioInputTranscriptionWorkerState {
    next_request_id: u64,
    request_in_flight: bool,
    debug_request_completed: bool,
    flush_requested: bool,
    sent_chunk_end_seconds: Option<f64>,
    result_receiver: Option<Receiver<Result<RustTranscriptionResult, String>>>,
    completion_notification: Option<AudioInputTranscriptionCompletionNotification>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AudioInputTranscriptionCompletionNotification {
    hwnd: isize,
    message: u32,
}

impl AudioInputTranscriptionCompletionNotification {
    fn post(self) {
        let hwnd = HWND(self.hwnd as *mut c_void);
        // Safety: the target is captured from the owning timeline UI window; stale windows are tolerated by PostMessageW.
        let _ = unsafe { PostMessageW(Some(hwnd), self.message, WPARAM(0), LPARAM(0)) };
    }
}

impl Default for AudioInputTranscriptionWorkerState {
    fn default() -> Self {
        Self {
            next_request_id: 1,
            request_in_flight: false,
            debug_request_completed: false,
            flush_requested: false,
            sent_chunk_end_seconds: None,
            result_receiver: None,
            completion_notification: None,
        }
    }
}

impl std::fmt::Debug for AudioInputTranscriptionWorkerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioInputTranscriptionWorkerState")
            .field("next_request_id", &self.next_request_id)
            .field("request_in_flight", &self.request_in_flight)
            .field("debug_request_completed", &self.debug_request_completed)
            .field("flush_requested", &self.flush_requested)
            .field("sent_chunk_end_seconds", &self.sent_chunk_end_seconds)
            .field("has_result_receiver", &self.result_receiver.is_some())
            .field("completion_notification", &self.completion_notification)
            .finish()
    }
}

impl AudioInputTranscriptionWorkerState {
    fn reset_for_enabled_session(&mut self) {
        self.request_in_flight = false;
        self.debug_request_completed = false;
        self.flush_requested = false;
        self.sent_chunk_end_seconds = None;
        self.result_receiver = None;
    }
}

impl AudioInputTranscriptionState {
    // audio[impl transcription.result-staging]
    pub fn stage_transcript_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if !self.staged_text.is_empty() {
            self.staged_text.push('\n');
        }
        self.staged_text.push_str(text);
    }

    pub fn take_staged_transcript_text(&mut self) -> Option<String> {
        (!self.staged_text.is_empty()).then(|| std::mem::take(&mut self.staged_text))
    }
}

impl AudioInputRuntimeState {
    // audio[impl transcription.cached-preview]
    pub fn refresh_transcription_preview_cache(&mut self) {
        #[cfg(feature = "tracy")]
        let _span = tracing::debug_span!("refresh_transcription_preview_cache").entered();
        if !self.transcription.enabled {
            return;
        }
        let (lookahead, sample_rate_hz, cache_key) = {
            let Ok(buffer) = self.shared.lock() else {
                return;
            };
            let head_millis = seconds_to_millis(self.transcription_head_seconds);
            let cache_key = AudioInputMelSpectrogramPreviewCacheKey {
                sample_count: buffer.samples.len(),
                sample_rate_hz: buffer.sample_rate_hz,
                head_millis,
                columns: TRANSCRIPTION_PREVIEW_COLUMNS,
                bins: TRANSCRIPTION_PREVIEW_BINS,
            };
            if self.transcription.preview.cache_key == cache_key {
                return;
            }
            if self.transcription.preview.cache_key.sample_rate_hz == cache_key.sample_rate_hz
                && self.transcription.preview.cache_key.head_millis == cache_key.head_millis
                && self.transcription.preview.cache_key.columns == cache_key.columns
                && self.transcription.preview.cache_key.bins == cache_key.bins
                && self
                    .transcription
                    .preview_last_updated_at
                    .is_some_and(|updated_at| {
                        updated_at.elapsed() < TRANSCRIPTION_PREVIEW_RECOMPUTE_INTERVAL
                    })
            {
                return;
            }
            let start_index = sample_index_from_seconds(
                self.transcription_head_seconds,
                buffer.sample_rate_hz,
                buffer.samples.len(),
            );
            let end_index = transcription_chunk_end_index(
                start_index,
                buffer.sample_rate_hz,
                buffer.samples.len(),
            );
            (
                buffer.samples[start_index..end_index].to_vec(),
                buffer.sample_rate_hz,
                cache_key,
            )
        };
        self.transcription.chunk_seconds =
            seconds_from_samples(lookahead.len(), sample_rate_hz).get::<second>();
        self.transcription.energy_rms = audio_rms_energy(&lookahead);
        self.transcription.preview = build_mel_spectrogram_preview(
            &lookahead,
            TRANSCRIPTION_PREVIEW_COLUMNS,
            TRANSCRIPTION_PREVIEW_BINS,
            cache_key,
        );
        self.transcription.preview_last_updated_at = Some(Instant::now());
    }

    #[must_use]
    // audio[impl transcription.sample-derived-handoff]
    pub fn transcription_chunk_samples(&self) -> (Vec<f32>, u32) {
        let Ok(buffer) = self.shared.lock() else {
            return (Vec::new(), 48_000);
        };
        let start_index = sample_index_from_seconds(
            self.transcription_head_seconds,
            buffer.sample_rate_hz,
            buffer.samples.len(),
        );
        let end_index =
            transcription_chunk_end_index(start_index, buffer.sample_rate_hz, buffer.samples.len());
        (
            buffer.samples[start_index..end_index].to_vec(),
            buffer.sample_rate_hz,
        )
    }

    #[must_use]
    pub fn samples(&self) -> Vec<f32> {
        self.shared
            .lock()
            .map(|buffer| buffer.samples.clone())
            .unwrap_or_default()
    }

    #[must_use]
    pub fn sample_rate_hz(&self) -> u32 {
        self.shared
            .lock()
            .map_or(48_000, |buffer| buffer.sample_rate_hz)
    }

    #[must_use]
    pub fn duration_seconds(&self) -> f64 {
        let Ok(buffer) = self.shared.lock() else {
            return 0.0;
        };
        seconds_from_samples(buffer.samples.len(), buffer.sample_rate_hz).get::<second>()
    }

    #[must_use]
    pub fn write_head_seconds(&self) -> f64 {
        let Ok(buffer) = self.shared.lock() else {
            return self.recording_head_seconds;
        };
        seconds_from_samples(buffer.write_head_index, buffer.sample_rate_hz).get::<second>()
    }

    #[must_use]
    pub fn last_error(&self) -> Option<String> {
        self.shared
            .lock()
            .ok()
            .and_then(|buffer| buffer.last_error.clone())
    }

    fn clear_error(&mut self) {
        if let Ok(mut buffer) = self.shared.lock() {
            buffer.last_error = None;
        }
    }

    fn set_write_head_seconds(&mut self, seconds: f64) {
        if let Ok(mut buffer) = self.shared.lock() {
            buffer.write_head_index = sample_index_for_seconds(
                buffer.sample_rate_hz,
                clamp_head_seconds(
                    seconds,
                    seconds_from_samples(buffer.samples.len(), buffer.sample_rate_hz)
                        .get::<second>(),
                ),
                buffer.samples.len().saturating_add(1),
            )
            .min(buffer.samples.len());
        }
    }

    fn sync_write_head_from_recording_head(&mut self) {
        self.set_write_head_seconds(self.recording_head_seconds);
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "timeline seconds are clamped before conversion to sample indices"
)]
fn sample_index_from_seconds(seconds: f64, sample_rate_hz: u32, sample_count: usize) -> usize {
    ((seconds.max(0.0) * f64::from(sample_rate_hz)) as usize).min(sample_count)
}

fn transcription_chunk_end_index(
    start_index: usize,
    sample_rate_hz: u32,
    sample_count: usize,
) -> usize {
    let max_chunk_samples = usize::try_from(sample_rate_hz)
        .unwrap_or(48_000)
        .saturating_mul(usize::try_from(TRANSCRIPTION_CHUNK_WINDOW_SECONDS).unwrap_or(30));
    start_index
        .saturating_add(max_chunk_samples)
        .min(sample_count)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "RMS energy divides by a small display buffer sample count"
)]
fn audio_rms_energy(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum = samples
        .iter()
        .map(|sample| sample.clamp(-1.0, 1.0).powi(2))
        .sum::<f32>();
    (sum / samples.len() as f32).sqrt()
}

#[expect(
    clippy::cast_precision_loss,
    reason = "preview bins are deliberately normalized into display intensities"
)]
fn build_mel_spectrogram_preview(
    samples: &[f32],
    columns: usize,
    bins: usize,
    cache_key: AudioInputMelSpectrogramPreviewCacheKey,
) -> AudioInputMelSpectrogramPreview {
    if samples.is_empty() {
        return AudioInputMelSpectrogramPreview {
            columns,
            bins,
            intensities: vec![0.0; columns * bins],
            cache_key,
        };
    }
    let peak = samples
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0_f32, f32::max)
        .max(0.015);
    let mut intensities = vec![0.0; columns * bins];
    for column in 0..columns {
        let start = (column * samples.len()) / columns;
        let end = (((column + 1) * samples.len()) / columns)
            .max(start + 1)
            .min(samples.len());
        let chunk = &samples[start..end];
        for bin in 0..bins {
            let mel_position = (bin + 1) as f32 / bins as f32;
            let sample_stride = (chunk.len() / TRANSCRIPTION_PREVIEW_MAX_SAMPLES_PER_CELL).max(1);
            let folded_energy = chunk
                .iter()
                .step_by(sample_stride)
                .enumerate()
                .map(|(index, sample)| {
                    let phase = (index as f32 * (bin + 1) as f32 * 0.071).sin().abs();
                    sample.abs() * (0.34 + phase * 0.66) * mel_position.sqrt()
                })
                .sum::<f32>()
                / chunk.len().max(1) as f32;
            intensities[(bin * columns) + column] = (folded_energy / peak).clamp(0.0, 1.0);
        }
    }
    AudioInputMelSpectrogramPreview {
        columns,
        bins,
        intensities,
        cache_key,
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "seconds are display-scale values and millis saturate for cache keys"
)]
fn seconds_to_millis(seconds: f64) -> u64 {
    (seconds.max(0.0) * 1_000.0).round() as u64
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AudioInputPlaybackState {
    pub head_seconds: f64,
    pub speed: f64,
    started_at: Option<Instant>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioInputSelection {
    pub begin_seconds: f64,
    pub end_seconds: f64,
}

impl AudioInputSelection {
    #[must_use]
    pub fn new(first_seconds: f64, second_seconds: f64) -> Option<Self> {
        if (first_seconds - second_seconds).abs() <= f64::EPSILON {
            return None;
        }
        Some(Self {
            begin_seconds: first_seconds.min(second_seconds),
            end_seconds: first_seconds.max(second_seconds),
        })
    }

    #[must_use]
    pub fn duration_seconds(self) -> f64 {
        self.end_seconds - self.begin_seconds
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioInputTimelineDrag {
    pub interaction: AudioInputTimelineInteraction,
    pub anchor_seconds: f64,
    pub current_seconds: f64,
    pub origin_x: i32,
    pub pointer_offset_seconds: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioInputTimelineInteraction {
    Selection,
    Head(AudioInputTimelineHeadKind),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioInputTimelineHeadKind {
    Recording,
    Playback,
    Transcription,
}

#[derive(Debug)]
struct AudioInputSharedBuffer {
    samples: Vec<f32>,
    write_head_index: usize,
    sample_rate_hz: u32,
    last_error: Option<String>,
}

impl Default for AudioInputSharedBuffer {
    fn default() -> Self {
        Self {
            samples: Vec::new(),
            write_head_index: 0,
            sample_rate_hz: 48_000,
            last_error: None,
        }
    }
}

#[derive(Debug)]
struct AudioInputCaptureSession {
    stop_requested: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl AudioInputCaptureSession {
    fn start(
        endpoint_id: String,
        shared: Arc<Mutex<AudioInputSharedBuffer>>,
        loopback_requested: Arc<AtomicBool>,
        write_to_buffer: bool,
    ) -> eyre::Result<Self> {
        let stop_requested = Arc::new(AtomicBool::new(false));
        let capture_stop_requested = Arc::clone(&stop_requested);
        let thread = thread::Builder::new()
            .name("teamy-studio-audio-input-capture".to_owned())
            .spawn_with_current_span(move || {
                if let Err(error) = capture_audio_input(
                    endpoint_id,
                    &shared,
                    &capture_stop_requested,
                    &loopback_requested,
                    write_to_buffer,
                ) && let Ok(mut buffer) = shared.lock()
                {
                    buffer.last_error = Some(error.to_string());
                }
            })
            .wrap_err("failed to spawn audio input capture thread")?;
        Ok(Self {
            stop_requested,
            thread,
        })
    }

    fn stop(self) {
        self.stop_requested.store(true, Ordering::Release);
        let _ = self.thread.join();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AudioSampleFormat {
    Float32,
    Pcm16,
    Pcm24,
    Pcm32,
}

impl AudioInputPickerState {
    #[must_use]
    pub fn new(devices: Vec<AudioInputDeviceSummary>) -> Self {
        Self {
            selected_index: 0,
            devices,
        }
    }

    #[must_use]
    pub fn selected_device(&self) -> Option<&AudioInputDeviceSummary> {
        self.devices.get(self.selected_index)
    }

    pub fn move_selection_up(&mut self) {
        if self.devices.is_empty() {
            self.selected_index = 0;
            return;
        }
        self.selected_index = self.selected_index.saturating_sub(1);
    }

    pub fn move_selection_down(&mut self) {
        if self.devices.is_empty() {
            self.selected_index = 0;
            return;
        }
        self.selected_index = (self.selected_index + 1).min(self.devices.len() - 1);
    }

    pub fn select_index(&mut self, index: usize) {
        if self.devices.is_empty() {
            self.selected_index = 0;
            return;
        }
        self.selected_index = index.min(self.devices.len() - 1);
    }

    #[must_use]
    // audio[impl gui.keyboard-navigation]
    pub fn handle_key(&mut self, key: AudioInputPickerKey) -> AudioInputPickerKeyResult {
        match key {
            AudioInputPickerKey::Up => {
                self.move_selection_up();
                AudioInputPickerKeyResult::Handled
            }
            AudioInputPickerKey::Down | AudioInputPickerKey::Tab => {
                self.move_selection_down();
                AudioInputPickerKeyResult::Handled
            }
            AudioInputPickerKey::Enter => AudioInputPickerKeyResult::Choose,
            AudioInputPickerKey::LegacyRecordingDevices => {
                AudioInputPickerKeyResult::OpenLegacyRecordingDevices
            }
            AudioInputPickerKey::Escape => AudioInputPickerKeyResult::Close,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioInputPickerKeyResult {
    Handled,
    Choose,
    OpenLegacyRecordingDevices,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegacyRecordingDevicesCommand {
    pub program: &'static str,
    pub args: &'static [&'static str],
}

#[must_use]
pub const fn legacy_recording_devices_command() -> LegacyRecordingDevicesCommand {
    LegacyRecordingDevicesCommand {
        program: "control.exe",
        args: &["mmsys.cpl,,1"],
    }
}

/// Open the legacy Windows Recording Devices dialog.
///
/// # Errors
///
/// Returns an error when Windows cannot spawn the legacy control panel command.
// audio[impl gui.legacy-recording-dialog]
pub fn open_legacy_recording_devices_dialog() -> eyre::Result<()> {
    let command = legacy_recording_devices_command();
    Command::new(command.program)
        .args(command.args)
        .spawn()
        .wrap_err("failed to open Windows legacy recording devices dialog")?;
    Ok(())
}

#[derive(Facet, Arbitrary, Debug, PartialEq, Eq)]
pub struct AudioInputDeviceReportFixture {
    pub id: String,
    pub name: String,
}

/// List active Windows audio input endpoints.
///
/// # Errors
///
/// This function will return an error if COM or Core Audio endpoint enumeration fails.
// audio[impl enumerate.active-windows-recording]
#[expect(
    clippy::undocumented_unsafe_blocks,
    reason = "Core Audio enumeration requires small, documented FFI calls"
)]
pub fn list_active_audio_input_devices() -> eyre::Result<Vec<AudioInputDeviceSummary>> {
    let _com = ComApartment::initialize()?;
    let enumerator: IMMDeviceEnumerator = unsafe {
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER)
            .wrap_err("failed to create Windows audio endpoint enumerator")?
    };
    let default_id = default_capture_endpoint_id(&enumerator);
    let collection: IMMDeviceCollection = unsafe {
        enumerator
            .EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)
            .wrap_err("failed to enumerate active Windows capture endpoints")?
    };
    let count = unsafe { collection.GetCount()? };
    let mut devices = Vec::with_capacity(usize::try_from(count).unwrap_or_default());

    for index in 0..count {
        let device: IMMDevice = unsafe { collection.Item(index)? };
        // audio[impl enumerate.endpoint-id]
        let id = unsafe { device.GetId()? };
        let id = unsafe { id.to_string()? };
        let properties = unsafe { device.OpenPropertyStore(STGM_READ).ok() };
        let name = properties
            .as_ref()
            .and_then(|properties| device_friendly_name(properties).ok())
            .unwrap_or_else(|| "Unknown microphone".to_owned());
        let icon = properties
            .as_ref()
            .and_then(device_icon_path)
            .unwrap_or_else(|| GENERIC_WINDOWS_MIC_ICON_PATH.to_owned());
        devices.push(AudioInputDeviceSummary {
            is_default: default_id
                .as_ref()
                .is_some_and(|default_id| default_id == &id),
            id,
            name,
            state: "active".to_owned(),
            // audio[impl enumerate.windows-icon]
            icon,
            // audio[impl enumerate.sample-rate]
            sample_rate_hz: device_mix_sample_rate_hz(&device).ok(),
        });
    }

    Ok(devices)
}

#[expect(
    clippy::undocumented_unsafe_blocks,
    reason = "Core Audio default endpoint lookup is an FFI call with no raw buffer ownership"
)]
fn default_capture_endpoint_id(enumerator: &IMMDeviceEnumerator) -> Option<String> {
    let default_device = unsafe {
        enumerator
            .GetDefaultAudioEndpoint(eCapture, ERole(1))
            .ok()?
    };
    let id = unsafe { default_device.GetId().ok()? };
    unsafe { id.to_string().ok() }
}

fn device_friendly_name(properties: &IPropertyStore) -> eyre::Result<String> {
    let friendly_name_key =
        std::ptr::from_ref(&Properties::DEVPKEY_Device_FriendlyName).cast::<PROPERTYKEY>();
    property_store_string_value(properties, friendly_name_key)
}

fn device_icon_path(properties: &IPropertyStore) -> Option<String> {
    let icon_key = PKEY_DEVICE_ICON;
    if let Ok(icon_path) = property_store_string_value(properties, std::ptr::from_ref(&icon_key)) {
        return Some(icon_path);
    }
    let class_icon_key = PKEY_DEVICE_CLASS_ICON;
    if let Ok(icon_path) =
        property_store_string_value(properties, std::ptr::from_ref(&class_icon_key))
    {
        return Some(icon_path);
    }
    None
}

#[expect(
    clippy::undocumented_unsafe_blocks,
    clippy::multiple_unsafe_ops_per_block,
    reason = "PROPVARIANT string extraction follows the Windows property-store layout"
)]
fn property_store_string_value(
    properties: &IPropertyStore,
    key: *const PROPERTYKEY,
) -> eyre::Result<String> {
    let mut value = unsafe { properties.GetValue(key)? };
    let variant_type = unsafe { value.Anonymous.Anonymous.vt };
    if variant_type != VT_LPWSTR {
        unsafe { PropVariantClear(&raw mut value)? };
        eyre::bail!("property value is not a UTF-16 string")
    }
    let name = unsafe {
        let pwstr = value.Anonymous.Anonymous.Anonymous.pwszVal;
        if pwstr.is_null() {
            String::new()
        } else {
            pwstr.to_string()?
        }
    };
    unsafe { PropVariantClear(&raw mut value)? };
    Ok(name)
}

#[expect(
    clippy::undocumented_unsafe_blocks,
    reason = "IAudioClient mix-format query activates metadata only and frees COM memory immediately"
)]
fn device_mix_sample_rate_hz(device: &IMMDevice) -> eyre::Result<u32> {
    let audio_client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None)? };
    let mix_format = unsafe { audio_client.GetMixFormat()? };
    if mix_format.is_null() {
        eyre::bail!("audio client returned a null mix format")
    }
    let sample_rate_ptr = unsafe { std::ptr::addr_of!((*mix_format).nSamplesPerSec) };
    let sample_rate = unsafe { sample_rate_ptr.read_unaligned() };
    unsafe { CoTaskMemFree(Some(mix_format.cast::<c_void>())) };
    Ok(sample_rate)
}

#[expect(
    clippy::undocumented_unsafe_blocks,
    reason = "WASAPI capture requires COM activation, borrowed format memory, and raw audio buffers"
)]
fn capture_audio_input(
    endpoint_id: String,
    shared: &Arc<Mutex<AudioInputSharedBuffer>>,
    stop_requested: &Arc<AtomicBool>,
    loopback_requested: &Arc<AtomicBool>,
    write_to_buffer: bool,
) -> eyre::Result<()> {
    let _com = ComApartment::initialize()?;
    let enumerator: IMMDeviceEnumerator = unsafe {
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER)
            .wrap_err("failed to create Windows audio endpoint enumerator")?
    };
    let endpoint_id = endpoint_id.easy_pcwstr()?;
    let device = unsafe { enumerator.GetDevice(endpoint_id.as_ref())? };
    let audio_client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None)? };
    let mix_format = unsafe { audio_client.GetMixFormat()? };
    if mix_format.is_null() {
        eyre::bail!("audio client returned a null capture mix format")
    }

    let capture_format = unsafe { audio_capture_format(mix_format)? };
    if let Ok(mut buffer) = shared.lock() {
        buffer.sample_rate_hz = capture_format.sample_rate_hz;
        buffer.last_error = None;
    }

    let initialize_result = unsafe {
        audio_client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            0,
            WASAPI_SHARED_BUFFER_100NS,
            0,
            mix_format,
            None,
        )
    };
    initialize_result?;
    let capture_client: IAudioCaptureClient = unsafe { audio_client.GetService()? };
    unsafe { audio_client.Start()? };
    let capture_result = poll_capture_client(
        &capture_client,
        capture_format,
        shared,
        stop_requested,
        loopback_requested,
        write_to_buffer,
    );
    let _ = unsafe { audio_client.Stop() };
    unsafe { CoTaskMemFree(Some(mix_format.cast::<c_void>())) };
    capture_result
}

#[expect(
    clippy::undocumented_unsafe_blocks,
    reason = "WASAPI packet access exposes borrowed raw buffers until ReleaseBuffer"
)]
fn poll_capture_client(
    capture_client: &IAudioCaptureClient,
    capture_format: AudioCaptureFormat,
    shared: &Arc<Mutex<AudioInputSharedBuffer>>,
    stop_requested: &Arc<AtomicBool>,
    loopback_requested: &Arc<AtomicBool>,
    write_to_buffer: bool,
) -> eyre::Result<()> {
    let mut loopback_session = AudioLoopbackRenderSession::open_default().ok();
    while !stop_requested.load(Ordering::Acquire) {
        let mut packet_frames = unsafe { capture_client.GetNextPacketSize()? };
        while packet_frames > 0 {
            let mut data = std::ptr::null_mut();
            let mut frames_to_read = 0;
            let mut flags = 0;
            let buffer_result = unsafe {
                capture_client.GetBuffer(
                    &raw mut data,
                    &raw mut frames_to_read,
                    &raw mut flags,
                    None,
                    None,
                )
            };
            buffer_result?;
            let mut samples = Vec::new();
            if flags & AUDCLNT_BUFFERFLAGS_SILENT != 0 {
                samples.resize(usize::try_from(frames_to_read).unwrap_or_default(), 0.0);
            } else if !data.is_null() {
                samples = unsafe { capture_frames_as_mono(data, frames_to_read, capture_format) };
            }
            unsafe { capture_client.ReleaseBuffer(frames_to_read)? };
            if write_to_buffer
                && !samples.is_empty()
                && let Ok(mut buffer) = shared.lock()
            {
                write_samples_into_audio_buffer(&mut buffer, &samples);
            }
            if loopback_requested.load(Ordering::Acquire)
                && let Some(session) = loopback_session.as_mut()
            {
                session.render_samples(&samples)?;
            }
            packet_frames = unsafe { capture_client.GetNextPacketSize()? };
        }
        thread::sleep(CAPTURE_POLL_INTERVAL);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AudioCaptureFormat {
    sample_rate_hz: u32,
    channels: u16,
    block_align: u16,
    sample_format: AudioSampleFormat,
}

struct AudioLoopbackRenderSession {
    audio_client: IAudioClient,
    render_client: IAudioRenderClient,
    render_format: AudioCaptureFormat,
    buffer_frame_count: u32,
}

impl AudioLoopbackRenderSession {
    #[expect(
        clippy::undocumented_unsafe_blocks,
        reason = "WASAPI render startup requires COM activation and shared-mode client initialization"
    )]
    fn open_default() -> eyre::Result<Self> {
        let enumerator: IMMDeviceEnumerator = unsafe {
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER)
                .wrap_err("failed to create Windows audio endpoint enumerator for loopback")?
        };
        let device = unsafe {
            enumerator
                .GetDefaultAudioEndpoint(eRender, ERole(1))
                .wrap_err("failed to resolve the default render endpoint for microphone loopback")?
        };
        let audio_client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None)? };
        let mix_format = unsafe { audio_client.GetMixFormat()? };
        if mix_format.is_null() {
            eyre::bail!("audio client returned a null render mix format")
        }
        let render_format = unsafe { audio_capture_format(mix_format)? };
        unsafe {
            audio_client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                0,
                WASAPI_SHARED_BUFFER_100NS,
                0,
                mix_format,
                None,
            )?;
        };
        let render_client: IAudioRenderClient = unsafe { audio_client.GetService()? };
        let buffer_frame_count = unsafe { audio_client.GetBufferSize()? };
        unsafe { audio_client.Start()? };
        unsafe { CoTaskMemFree(Some(mix_format.cast::<c_void>())) };
        Ok(Self {
            audio_client,
            render_client,
            render_format,
            buffer_frame_count,
        })
    }

    #[expect(
        clippy::undocumented_unsafe_blocks,
        reason = "loopback copies normalized mono samples into the default render mix format"
    )]
    fn render_samples(&mut self, mono_samples: &[f32]) -> eyre::Result<()> {
        if mono_samples.is_empty() {
            return Ok(());
        }
        let padding = unsafe { self.audio_client.GetCurrentPadding()? };
        let available_frames = self.buffer_frame_count.saturating_sub(padding);
        if available_frames == 0 {
            return Ok(());
        }
        let render_frames = usize::try_from(available_frames).unwrap_or_default();
        let converted = resample_mono_for_render(mono_samples, render_frames, self.render_format);
        let frame_count = converted.len() / usize::from(self.render_format.channels.max(1));
        if frame_count == 0 {
            return Ok(());
        }
        let buffer = unsafe {
            self.render_client
                .GetBuffer(u32::try_from(frame_count).unwrap_or_default())?
        };
        unsafe { write_render_frames(buffer.cast::<u8>(), &converted, self.render_format) };
        unsafe {
            self.render_client
                .ReleaseBuffer(u32::try_from(frame_count).unwrap_or_default(), 0)?;
        };
        Ok(())
    }
}

impl Drop for AudioLoopbackRenderSession {
    fn drop(&mut self) {
        // Safety: stopping an initialized shared-mode audio client during drop releases the render stream.
        let _ = unsafe { self.audio_client.Stop() };
    }
}

#[expect(
    clippy::undocumented_unsafe_blocks,
    clippy::multiple_unsafe_ops_per_block,
    reason = "WASAPI returns packed format structs that require unaligned reads"
)]
unsafe fn audio_capture_format(format: *const WAVEFORMATEX) -> eyre::Result<AudioCaptureFormat> {
    let wave_format = unsafe { format.read_unaligned() };
    let format_tag = wave_format.wFormatTag;
    let bits_per_sample = wave_format.wBitsPerSample;
    let sample_format = if format_tag == WAVE_FORMAT_EXTENSIBLE {
        let extensible = format.cast::<WAVEFORMATEXTENSIBLE>();
        let sub_format = unsafe { std::ptr::addr_of!((*extensible).SubFormat).read_unaligned() };
        if sub_format == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT && bits_per_sample == 32 {
            AudioSampleFormat::Float32
        } else if sub_format == KSDATAFORMAT_SUBTYPE_PCM {
            pcm_sample_format(bits_per_sample)?
        } else {
            eyre::bail!("unsupported WASAPI extensible capture sample format")
        }
    } else if format_tag == WAVE_FORMAT_IEEE_FLOAT && bits_per_sample == 32 {
        AudioSampleFormat::Float32
    } else if format_tag == WAVE_FORMAT_PCM {
        pcm_sample_format(bits_per_sample)?
    } else {
        eyre::bail!("unsupported WASAPI capture sample format tag {format_tag}")
    };
    Ok(AudioCaptureFormat {
        sample_rate_hz: wave_format.nSamplesPerSec,
        channels: wave_format.nChannels.max(1),
        block_align: wave_format.nBlockAlign,
        sample_format,
    })
}

fn pcm_sample_format(bits_per_sample: u16) -> eyre::Result<AudioSampleFormat> {
    match bits_per_sample {
        16 => Ok(AudioSampleFormat::Pcm16),
        24 => Ok(AudioSampleFormat::Pcm24),
        32 => Ok(AudioSampleFormat::Pcm32),
        _ => eyre::bail!("unsupported WASAPI PCM bit depth {bits_per_sample}"),
    }
}

#[expect(
    clippy::undocumented_unsafe_blocks,
    reason = "captured WASAPI frames are raw bytes converted into normalized mono f32 samples"
)]
unsafe fn capture_frames_as_mono(
    data: *const u8,
    frame_count: u32,
    capture_format: AudioCaptureFormat,
) -> Vec<f32> {
    let frame_count = usize::try_from(frame_count).unwrap_or_default();
    let channels = usize::from(capture_format.channels);
    let block_align = usize::from(capture_format.block_align);
    let bytes_per_sample = block_align / channels.max(1);
    let mut samples = Vec::with_capacity(frame_count);
    for frame_index in 0..frame_count {
        let frame_base = unsafe { data.add(frame_index * block_align) };
        let mut sum = 0.0;
        for channel_index in 0..channels {
            let sample_base = unsafe { frame_base.add(channel_index * bytes_per_sample) };
            sum += unsafe { read_capture_sample(sample_base, capture_format.sample_format) };
        }
        samples.push(sum / f32::from(capture_format.channels));
    }
    samples
}

#[expect(
    clippy::undocumented_unsafe_blocks,
    clippy::multiple_unsafe_ops_per_block,
    clippy::cast_lossless,
    clippy::cast_precision_loss,
    reason = "sample decoding reads packed PCM bytes and normalizes them to f32"
)]
unsafe fn read_capture_sample(sample_base: *const u8, sample_format: AudioSampleFormat) -> f32 {
    match sample_format {
        AudioSampleFormat::Float32 => {
            unsafe { sample_base.cast::<f32>().read_unaligned() }.clamp(-1.0, 1.0)
        }
        AudioSampleFormat::Pcm16 => {
            f32::from(unsafe { sample_base.cast::<i16>().read_unaligned() }) / 32768.0
        }
        AudioSampleFormat::Pcm24 => {
            let byte0 = unsafe { *sample_base.add(0) } as i32;
            let byte1 = unsafe { *sample_base.add(1) } as i32;
            let byte2 = unsafe { *sample_base.add(2) } as i32;
            let value = (byte0 | (byte1 << 8) | (byte2 << 16)) << 8 >> 8;
            value as f32 / 8_388_608.0
        }
        AudioSampleFormat::Pcm32 => {
            (unsafe { sample_base.cast::<i32>().read_unaligned() }) as f32 / 2_147_483_648.0
        }
    }
}

fn resample_mono_for_render(
    mono_samples: &[f32],
    output_frame_capacity: usize,
    render_format: AudioCaptureFormat,
) -> Vec<f32> {
    if mono_samples.is_empty() || output_frame_capacity == 0 {
        return Vec::new();
    }
    let output_frames = mono_samples.len().min(output_frame_capacity);
    let channels = usize::from(render_format.channels.max(1));
    let mut converted = Vec::with_capacity(output_frames.saturating_mul(channels));
    for frame_index in 0..output_frames {
        let source_index = (frame_index * mono_samples.len()) / output_frames;
        let sample = mono_samples[source_index].clamp(-1.0, 1.0);
        for _ in 0..channels {
            converted.push(sample);
        }
    }
    converted
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::undocumented_unsafe_blocks,
    reason = "loopback render writes normalized mono samples into the render endpoint buffer"
)]
unsafe fn write_render_frames(buffer: *mut u8, samples: &[f32], render_format: AudioCaptureFormat) {
    match render_format.sample_format {
        AudioSampleFormat::Float32 => {
            for (index, sample) in samples.iter().copied().enumerate() {
                let bytes = sample.to_le_bytes();
                let sample_base = unsafe { buffer.add(index * size_of::<f32>()) };
                for (offset, byte) in bytes.into_iter().enumerate() {
                    let target = sample_base.wrapping_add(offset);
                    unsafe { target.write(byte) };
                }
            }
        }
        AudioSampleFormat::Pcm16 => {
            for (index, sample) in samples.iter().copied().enumerate() {
                let bytes = ((sample * f32::from(i16::MAX)).round() as i16).to_le_bytes();
                let sample_base = unsafe { buffer.add(index * size_of::<i16>()) };
                for (offset, byte) in bytes.into_iter().enumerate() {
                    let target = sample_base.wrapping_add(offset);
                    unsafe { target.write(byte) };
                }
            }
        }
        AudioSampleFormat::Pcm24 => {
            for (index, sample) in samples.iter().copied().enumerate() {
                let sample = ((sample * 8_388_607.0).round() as i32).clamp(-8_388_608, 8_388_607);
                let sample_bytes = sample.to_le_bytes();
                let sample_base = unsafe { buffer.add(index * 3) };
                unsafe { sample_base.write(sample_bytes[0]) };
                unsafe { sample_base.wrapping_add(1).write(sample_bytes[1]) };
                unsafe { sample_base.wrapping_add(2).write(sample_bytes[2]) };
            }
        }
        AudioSampleFormat::Pcm32 => {
            for (index, sample) in samples.iter().copied().enumerate() {
                let bytes = ((sample * 2_147_483_647.0).round() as i32).to_le_bytes();
                let sample_base = unsafe { buffer.add(index * size_of::<i32>()) };
                for (offset, byte) in bytes.into_iter().enumerate() {
                    let target = sample_base.wrapping_add(offset);
                    unsafe { target.write(byte) };
                }
            }
        }
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "timeline durations are display-scale f64 values derived from sample counts"
)]
fn seconds_from_samples(sample_count: usize, sample_rate_hz: u32) -> Time {
    if sample_rate_hz == 0 {
        return Time::new::<second>(0.0);
    }
    Time::new::<second>(sample_count as f64 / f64::from(sample_rate_hz))
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "playback head seconds are clamped before converting to a sample index"
)]
fn sample_index_for_seconds(sample_rate_hz: u32, seconds: f64, sample_len: usize) -> usize {
    if sample_rate_hz == 0 || sample_len == 0 {
        return 0;
    }
    let sample_index = (seconds.max(0.0) * f64::from(sample_rate_hz)).floor();
    if !sample_index.is_finite() || sample_index <= 0.0 {
        return 0;
    }
    let sample_index = sample_index as usize;
    sample_index.min(sample_len.saturating_sub(1))
}

fn clamp_head_seconds(seconds: f64, duration_seconds: f64) -> f64 {
    seconds.clamp(0.0, duration_seconds.max(0.0))
}

fn playback_start_head_seconds(head_seconds: f64, duration_seconds: f64, speed: f64) -> f64 {
    let duration_seconds = duration_seconds.max(0.0);
    if duration_seconds <= f64::EPSILON {
        return 0.0;
    }
    let clamped = clamp_head_seconds(head_seconds, duration_seconds);
    if speed < 0.0 {
        return clamped.min(duration_seconds);
    }
    if clamped >= duration_seconds {
        return 0.0;
    }
    clamped
}

fn write_samples_into_audio_buffer(buffer: &mut AudioInputSharedBuffer, incoming_samples: &[f32]) {
    let write_start = buffer.write_head_index.min(buffer.samples.len());
    let overwrite_len = incoming_samples
        .len()
        .min(buffer.samples.len().saturating_sub(write_start));
    if overwrite_len > 0 {
        buffer.samples[write_start..write_start + overwrite_len]
            .copy_from_slice(&incoming_samples[..overwrite_len]);
    }
    if overwrite_len < incoming_samples.len() {
        buffer
            .samples
            .extend_from_slice(&incoming_samples[overwrite_len..]);
    }
    buffer.write_head_index = write_start.saturating_add(incoming_samples.len());
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "playback transport maps display seconds and shuttle speed onto sample indexes"
)]
fn playback_samples_from_head(
    samples: &[f32],
    sample_rate_hz: u32,
    head_seconds: f64,
    speed: f64,
) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    let speed = if speed == 0.0 {
        1.0
    } else {
        speed.clamp(-8.0, 8.0)
    };
    let stride = speed.abs().round().max(1.0) as usize;
    let head_index = sample_index_for_seconds(sample_rate_hz, head_seconds, samples.len());
    if speed < 0.0 {
        return samples[..=head_index]
            .iter()
            .rev()
            .step_by(stride)
            .copied()
            .collect();
    }
    samples[head_index..]
        .iter()
        .step_by(stride)
        .copied()
        .collect()
}

#[expect(
    clippy::undocumented_unsafe_blocks,
    reason = "PlaySoundW accepts null sound/module pointers to stop current async playback"
)]
fn stop_windows_playback() {
    let _ = unsafe { PlaySoundW(PCWSTR::null(), None, PLAYBACK_STOP_FLAGS) };
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "WAV playback writes clamped f32 samples into intentional 16-bit PCM values"
)]
fn write_audio_input_playback_wav(sample_rate_hz: u32, samples: &[f32]) -> eyre::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "teamy-studio-audio-input-{}.wav",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let mut file = fs::File::create(&path)
        .wrap_err_with(|| format!("failed to create playback wav at {}", path.display()))?;
    let data_bytes = u32::try_from(samples.len().saturating_mul(2)).unwrap_or(u32::MAX);
    let chunk_size = 36u32.saturating_add(data_bytes);
    file.write_all(b"RIFF")?;
    file.write_all(&chunk_size.to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&sample_rate_hz.to_le_bytes())?;
    file.write_all(&sample_rate_hz.saturating_mul(2).to_le_bytes())?;
    file.write_all(&2u16.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_bytes.to_le_bytes())?;
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
        file.write_all(&value.to_le_bytes())?;
    }
    Ok(path)
}

struct ComApartment {
    uninitialize_on_drop: bool,
}

impl ComApartment {
    #[expect(
        clippy::undocumented_unsafe_blocks,
        reason = "COM apartment initialization is a process API with no borrowed pointers"
    )]
    fn initialize() -> eyre::Result<Self> {
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result.is_ok() {
            return Ok(Self {
                uninitialize_on_drop: true,
            });
        }
        if result == RPC_E_CHANGED_MODE {
            return Ok(Self {
                uninitialize_on_drop: false,
            });
        }
        eyre::bail!("failed to initialize COM for audio endpoint enumeration: {result:?}")
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.uninitialize_on_drop {
            // Safety: this instance only sets the flag when `CoInitializeEx` succeeded on this thread.
            unsafe { CoUninitialize() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str, name: &str) -> AudioInputDeviceSummary {
        AudioInputDeviceSummary {
            id: id.to_owned(),
            name: name.to_owned(),
            is_default: false,
            state: "active".to_owned(),
            icon: "microphone".to_owned(),
            sample_rate_hz: None,
        }
    }

    #[test]
    // audio[verify gui.keyboard-navigation]
    fn picker_navigation_clamps_to_available_devices() {
        let mut state = AudioInputPickerState::new(vec![device("a", "A"), device("b", "B")]);

        state.move_selection_down();
        state.move_selection_down();
        assert_eq!(state.selected_index, 1);

        state.move_selection_up();
        state.move_selection_up();
        assert_eq!(state.selected_index, 0);
    }

    #[test]
    // audio[verify gui.keyboard-navigation]
    fn picker_enter_chooses_current_device() {
        let mut state = AudioInputPickerState::new(vec![device("a", "A")]);

        assert_eq!(
            state.handle_key(AudioInputPickerKey::Enter),
            AudioInputPickerKeyResult::Choose
        );
        assert_eq!(
            state.selected_device().map(|device| device.id.as_str()),
            Some("a")
        );
    }

    #[test]
    // audio[verify gui.arm-for-record]
    fn selected_device_window_starts_armed() {
        let state = AudioInputDeviceWindowState::new(device("endpoint-id", "Studio Mic"));

        assert!(state.armed_for_record);
    }

    #[test]
    // audio[verify gui.legacy-recording-dialog]
    fn legacy_recording_devices_command_opens_recording_tab() {
        let command = legacy_recording_devices_command();

        assert_eq!(command.program, "control.exe");
        assert_eq!(command.args, &["mmsys.cpl,,1"]);
    }

    #[test]
    // audio[verify gui.playback-transport]
    fn playback_samples_start_at_playback_head() {
        let samples = [0.0, 0.1, 0.2, 0.3, 0.4];

        assert_eq!(
            playback_samples_from_head(&samples, 10, 0.2, 1.0),
            vec![0.2, 0.3, 0.4]
        );
    }

    #[test]
    // audio[verify gui.playback-transport]
    fn reverse_playback_samples_walk_back_from_playback_head() {
        let samples = [0.0, 0.1, 0.2, 0.3, 0.4];

        assert_eq!(
            playback_samples_from_head(&samples, 10, 0.3, -1.0),
            vec![0.3, 0.2, 0.1, 0.0]
        );
    }

    #[test]
    // audio[verify gui.playback-transport]
    fn fast_playback_samples_stride_forward_from_playback_head() {
        let samples = [0.0, 0.1, 0.2, 0.3, 0.4, 0.5];

        assert_eq!(
            playback_samples_from_head(&samples, 10, 0.1, 2.0),
            vec![0.1, 0.3, 0.5]
        );
    }

    #[test]
    // audio[verify gui.playback-transport]
    fn playback_restarts_from_beginning_when_playing_forward_at_end() {
        assert_eq!(playback_start_head_seconds(2.0, 2.0, 1.0), 0.0);
    }

    #[test]
    // audio[verify gui.playback-transport]
    fn reverse_playback_keeps_end_head_when_shuttling_backward() {
        assert_eq!(playback_start_head_seconds(2.0, 2.0, -1.0), 2.0);
    }

    #[test]
    // audio[verify gui.audio-buffer-waveform]
    fn writing_samples_appends_at_current_write_head() {
        let mut buffer = AudioInputSharedBuffer {
            samples: vec![0.1, 0.2, 0.3],
            write_head_index: 3,
            sample_rate_hz: 10,
            last_error: None,
        };

        write_samples_into_audio_buffer(&mut buffer, &[0.4, 0.5]);

        assert_eq!(buffer.samples, vec![0.1, 0.2, 0.3, 0.4, 0.5]);
        assert_eq!(buffer.write_head_index, 5);
    }

    #[test]
    // audio[verify gui.audio-buffer-waveform]
    fn writing_samples_overwrites_from_repositioned_write_head() {
        let mut buffer = AudioInputSharedBuffer {
            samples: vec![0.1, 0.2, 0.3, 0.4],
            write_head_index: 1,
            sample_rate_hz: 10,
            last_error: None,
        };

        write_samples_into_audio_buffer(&mut buffer, &[0.9, 0.8, 0.7]);

        assert_eq!(buffer.samples, vec![0.1, 0.9, 0.8, 0.7]);
        assert_eq!(buffer.write_head_index, 4);
    }

    #[test]
    // audio[verify transcription.result-staging]
    fn transcription_result_releases_slot_and_stages_text() {
        use super::super::audio_transcription::{
            AudioTranscriptionControlResult, AudioTranscriptionSharedMemorySlotPool,
            WHISPER_CONTROL_PROTOCOL_VERSION, WhisperLogMel80x3000,
        };

        let mut pool = AudioTranscriptionSharedMemorySlotPool::new(1)
            .expect("shared-memory pool should be created");
        let queued = pool
            .enqueue_tensor(41, &WhisperLogMel80x3000::zeros())
            .expect("tensor should enqueue");
        let mut state = AudioInputDeviceWindowState::new(device("endpoint-id", "Studio Mic"));
        let result = AudioTranscriptionControlResult {
            protocol_version: WHISPER_CONTROL_PROTOCOL_VERSION,
            kind: "transcription-result".to_owned(),
            request_id: 41,
            slot_id: queued.slot_id,
            release_slot: true,
            ok: true,
            transcript_text: "hello from the pipe".to_owned(),
            error: None,
        };

        state.apply_transcription_result(&mut pool, &result);

        assert_eq!(pool.status().queued_request_count, 0);
        assert_eq!(
            state.runtime.transcription.staged_text,
            "hello from the pipe"
        );
    }

    #[test]
    // audio[verify transcription.head-progress]
    fn successful_transcription_result_advances_transcription_head() {
        let mut state = AudioInputDeviceWindowState::new(device("endpoint-id", "Studio Mic"));
        state.runtime.transcription.enabled = true;
        {
            let mut buffer = state
                .runtime
                .shared
                .lock()
                .expect("shared buffer should lock");
            buffer.sample_rate_hz = 10;
            buffer.samples = vec![0.0; 30];
        }
        let (sender, receiver) = mpsc::channel();
        sender
            .send(Ok(RustTranscriptionResult {
                request_id: 5,
                ok: true,
                transcript_text: "done".to_owned(),
                error: None,
            }))
            .expect("result should send");
        state.transcription_worker.result_receiver = Some(receiver);
        state.transcription_worker.sent_chunk_end_seconds = Some(1.7);

        state.drain_debug_transcription_result();

        assert_eq!(state.runtime.transcription_head_seconds, 1.7);
    }

    #[test]
    // timeline[verify transcription.completion-refresh]
    fn transcription_completion_notification_target_is_stored_for_worker_requests() {
        let mut state = AudioInputDeviceWindowState::new(device("endpoint-id", "Studio Mic"));

        state.set_transcription_completion_notification_target(42, 0x405);

        assert_eq!(
            state.transcription_worker.completion_notification,
            Some(AudioInputTranscriptionCompletionNotification {
                hwnd: 42,
                message: 0x405,
            })
        );
    }

    #[test]
    // audio[verify transcription.cached-preview]
    fn transcription_preview_cache_tracks_chunk_energy_and_shape() {
        let mut runtime = AudioInputRuntimeState::default();
        runtime.transcription.enabled = true;
        {
            let mut buffer = runtime.shared.lock().expect("shared buffer should lock");
            buffer.sample_rate_hz = 48_000;
            buffer.samples = vec![0.25; 4_800];
        }

        runtime.refresh_transcription_preview_cache();

        assert_eq!(runtime.transcription.preview.columns, 72);
        assert_eq!(runtime.transcription.preview.bins, 18);
        assert_eq!(runtime.transcription.preview.intensities.len(), 72 * 18);
        assert_eq!(runtime.transcription.chunk_seconds, 0.1);
        assert!(runtime.transcription.energy_rms > 0.0);
    }

    #[test]
    // audio[verify transcription.sample-derived-handoff]
    fn transcription_chunk_samples_start_at_transcription_head() {
        let mut runtime = AudioInputRuntimeState::default();
        runtime.transcription_head_seconds = 0.2;
        {
            let mut buffer = runtime.shared.lock().expect("shared buffer should lock");
            buffer.sample_rate_hz = 10;
            buffer.samples = vec![0.0, 0.1, 0.2, 0.3, 0.4];
        }

        let (samples, sample_rate_hz) = runtime.transcription_chunk_samples();

        assert_eq!(sample_rate_hz, 10);
        assert_eq!(samples, vec![0.2, 0.3, 0.4]);
    }

    #[test]
    // audio[verify transcription.manual-flush]
    fn manual_flush_enables_transcription_and_queues_chunk_send() {
        let mut state = AudioInputDeviceWindowState::new(device("endpoint-id", "Studio Mic"));

        state.flush_transcription_chunk();

        assert!(state.runtime.transcription.enabled);
        assert!(state.transcription_worker.flush_requested);
        assert!(!state.transcription_worker.debug_request_completed);
        assert_eq!(
            state.runtime.transcription.last_sent_reason.as_deref(),
            Some("manual flush queued")
        );
    }
}
