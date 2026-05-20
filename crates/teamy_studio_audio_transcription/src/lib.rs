use std::path::PathBuf;

pub mod paths {
    pub use teamy_studio_paths::*;
}

pub mod win32_support {
    pub mod string {
        pub use teamy_studio_win32_support::string::*;
    }
}

#[must_use]
pub fn repo_root_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

mod audio_transcription_impl;

pub use audio_transcription_impl::*;
