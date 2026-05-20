pub mod model {
    pub const DEFAULT_TRANSCRIPTION_MODEL_NAME: &str = "tiny.en";
}

pub mod timeline_impl;

pub use timeline_impl as timeline;
pub use timeline_impl::*;
