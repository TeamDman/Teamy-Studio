use eyre::Context;
use facet::Facet;

pub use teamy_studio_audio_transcription::*;
pub use teamy_studio_cursor_info::{CursorInfoConfig, CursorInfoPixelSize, CursorInfoRenderMode};
pub use teamy_studio_jobs::*;
pub use teamy_studio_terminal_core::VtEngineChoice;

pub mod app {
    pub mod windows_d3d12_renderer {
        pub use teamy_studio_shell::windows_d3d12_renderer::*;
    }

    pub mod windows_terminal {
        pub use teamy_studio_terminal_core::windows_terminal::*;
    }
}

pub mod cell_grid {
    pub use teamy_studio_shell::cell_grid::*;
}

pub mod jobs {
    pub use teamy_studio_jobs::*;
}

pub mod logs {
    pub use teamy_studio_logs::*;
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

pub mod spatial {
    pub use teamy_studio_spatial::*;
}

pub mod timeline {
    pub use teamy_studio_timeline_core::*;
}

pub mod vt_types {
    pub use teamy_studio_vt_types::*;
}

pub mod windows_audio {
    pub use teamy_studio_windows_audio::*;
}

pub mod windows_audio_input {
    pub use teamy_studio_audio_input::*;
}

pub mod windows_cursor_info {
    pub use teamy_studio_cursor_info::*;
}

pub mod windows_d3d12_renderer {
    pub use teamy_studio_shell::windows_d3d12_renderer::*;
}

pub mod windows_demo_mode {
    pub use teamy_studio_demo_mode::*;
}

pub mod windows_dialogs {
    pub use teamy_studio_windows_dialogs::*;
}

pub mod windows_scene {
    pub use teamy_studio_shell::windows_scene::*;
}

pub mod windows_terminal {
    pub use teamy_studio_terminal_core::windows_terminal::*;
}

pub mod win32_support {
    pub mod clipboard {
        pub use teamy_studio_win32_support::clipboard::*;
    }

    pub mod module {
        pub use teamy_studio_win32_support::module::*;
    }

    pub mod string {
        pub use teamy_studio_win32_support::string::*;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalThroughputBenchmarkMode {
    MeasureCommandOutHost,
    StreamSmallBatches,
    WideLines,
    ScrollFlood,
    PromptBursts,
    ResizeDuringOutput,
}

#[derive(Clone, Debug, Facet, PartialEq, Eq)]
pub struct TerminalWindowSummary {
    pub hwnd: usize,
    pub pid: u32,
    pub title: String,
}

pub fn open_terminal_window(
    app_home: &paths::AppHome,
    command_argv: Option<&[String]>,
    initial_stdin: Option<&str>,
    title: Option<&str>,
    vt_engine: VtEngineChoice,
) -> eyre::Result<()> {
    let working_dir =
        std::env::current_dir().wrap_err("failed to resolve the current working directory")?;
    windows_app_impl::run(
        app_home,
        &working_dir,
        command_argv,
        initial_stdin,
        title,
        vt_engine,
    )
    .map_err(|error| {
        error.wrap_err(format!(
            "failed to open terminal window{}",
            title.map_or_else(String::new, |value| format!(" `{value}`"))
        ))
    })
}

#[path = "../../../src/app/windows_app_impl.rs"]
mod windows_app_impl;

pub mod windows_app {
    pub use super::windows_app_impl::*;
}

pub use windows_app_impl::*;
