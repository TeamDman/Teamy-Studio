pub mod audio {
    pub use teamy_studio_audio_core::*;
}

pub mod frontend {
    pub use teamy_studio_frontend::*;
}

pub mod paths {
    pub use teamy_studio_paths::*;
}

#[path = "../../../src/model_impl.rs"]
pub mod model;

#[path = "../../../src/transcription_impl.rs"]
pub mod transcription;

#[path = "../../../src/whisper_impl.rs"]
pub mod whisper;

pub use model::*;
pub use transcription::*;
pub use whisper::*;
