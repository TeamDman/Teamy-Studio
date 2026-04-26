use std::path::PathBuf;

use facet::Facet;

use crate::paths::CacheHome;

pub const WHISPER_LOG_MEL_BINS: usize = 80;
pub const WHISPER_LOG_MEL_FRAMES: usize = 3_000;
pub const WHISPER_LOG_MEL_VALUE_COUNT: usize = WHISPER_LOG_MEL_BINS * WHISPER_LOG_MEL_FRAMES;
pub const WHISPER_LOG_MEL_BYTE_COUNT: usize = WHISPER_LOG_MEL_VALUE_COUNT * size_of::<f32>();
pub const WHISPER_LOG_MEL_DTYPE: &str = "f32-le";
pub const WHISPER_DAEMON_SOURCE_PARENT_DIR: &str = "python";
pub const WHISPER_DAEMON_SOURCE_DIR_NAME: &str = "whisperx-daemon";

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
    pub control_transport: String,
    pub payload_transport: String,
    pub python_entrypoint: String,
}

#[must_use]
// audio[impl cli.daemon-status]
// audio[impl python.daemon-project]
// audio[impl transcription.shared-memory-payload]
pub fn audio_transcription_daemon_status(
    cache_home: &CacheHome,
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
        control_transport: "windows named pipe".to_owned(),
        payload_transport: "rust-owned shared-memory slot".to_owned(),
        python_entrypoint: "teamy_whisperx_daemon".to_owned(),
    }
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
    fn whisper_log_mel_rejects_wrong_value_count() {
        let error =
            WhisperLogMel80x3000::from_vec(vec![0.0; 12]).expect_err("wrong length should fail");

        assert!(error.to_string().contains("expected 240000"));
    }
}
