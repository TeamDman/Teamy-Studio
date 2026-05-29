pub mod paths {
    pub use teamy_studio_paths::*;
}

pub mod shell_default {
    pub use teamy_studio_shell_default::*;
}

pub mod spatial {
    pub use teamy_studio_spatial::*;
}

pub mod teamy_terminal_engine {
    pub use teamy_studio_teamy_terminal_engine::*;
}

pub mod vt_types {
    pub use teamy_studio_vt_types::*;
}

pub mod windows_audio {
    pub use teamy_studio_windows_audio::*;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VtEngineChoice {
    #[default]
    Teamy,
}

impl VtEngineChoice {
    pub const CURRENT_TERMINAL_VT_ENGINE_ENV_VAR: &str = "TEAMY_STUDIO_CURRENT_TERMINAL_VT_ENGINE";

    #[must_use]
    pub const fn current_terminal_vt_engine_env_value(self) -> &'static str {
        match self {
            Self::Teamy => "teamy",
        }
    }
}

mod windows_terminal_impl;

pub mod windows_terminal {
    pub use super::windows_terminal_impl::*;
}

mod windows_terminal_self_test_impl;

pub use windows_terminal_impl::*;
pub use windows_terminal_self_test_impl::KeyboardInputSelfTestReport;

pub fn run_keyboard_input_self_test(
    app_home: &paths::AppHome,
    inside: bool,
    scenario: Option<&str>,
    artifact_output: Option<&std::path::Path>,
    vt_engine: VtEngineChoice,
) -> eyre::Result<KeyboardInputSelfTestReport> {
    windows_terminal_self_test_impl::run(app_home, inside, scenario, artifact_output, vt_engine)
}
