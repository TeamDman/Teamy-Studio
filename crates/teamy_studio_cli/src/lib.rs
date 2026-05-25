pub mod app;

pub mod audio {
    pub use teamy_studio_audio_core::*;
}

pub mod frontend {
    pub use teamy_studio_frontend::*;
}

pub mod image_model {
    pub use teamy_studio_image_models::*;
}

pub mod llm {
    pub use teamy_studio_llm_stack::*;
}

pub mod model {
    pub use teamy_studio_whisper_stack::model::*;
}

pub mod paths {
    pub use teamy_studio_paths::*;
}

pub mod shell_default {
    pub use teamy_studio_shell_default::*;
}

pub mod transcription {
    pub use teamy_studio_whisper_stack::transcription::*;
}

pub mod waifu2x_reference {
    pub use teamy_studio_waifu2x_reference::*;
}

pub mod whisper {
    pub use teamy_studio_whisper_stack::whisper::*;
}

#[path = "cli/mod.rs"]
mod cli_impl;

pub use cli_impl::*;

pub mod cli {
    pub use super::cli_impl::*;
}
