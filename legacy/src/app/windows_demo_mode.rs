use std::fs;
use std::sync::{Mutex, OnceLock};

use crate::paths::AppHome;
use eyre::Context;

const DEMO_MODE_FILENAME: &str = "demo-mode.txt";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DemoModeState {
    pub scramble_input_device_identifiers: bool,
}

pub fn initialize_demo_mode_state(app_home: &AppHome) -> eyre::Result<()> {
    let loaded = load_demo_mode_state(app_home)?;
    let mut state = demo_mode_state()
        .lock()
        .map_err(|error| eyre::eyre!("failed to lock demo mode state: {error}"))?;
    *state = loaded;
    Ok(())
}

pub fn current_demo_mode_state() -> DemoModeState {
    demo_mode_state()
        .lock()
        .map(|state| *state)
        .unwrap_or_default()
}

pub fn set_scramble_input_device_identifiers(
    app_home: &AppHome,
    enabled: bool,
) -> eyre::Result<()> {
    // windowing[impl demo-mode.persist-scramble-toggle]
    let state = DemoModeState {
        scramble_input_device_identifiers: enabled,
    };
    save_demo_mode_state(app_home, state)?;
    let mut stored = demo_mode_state()
        .lock()
        .map_err(|error| eyre::eyre!("failed to lock demo mode state: {error}"))?;
    *stored = state;
    Ok(())
}

fn demo_mode_state() -> &'static Mutex<DemoModeState> {
    static DEMO_MODE_STATE: OnceLock<Mutex<DemoModeState>> = OnceLock::new();

    DEMO_MODE_STATE.get_or_init(|| Mutex::new(DemoModeState::default()))
}

fn demo_mode_path(app_home: &AppHome) -> std::path::PathBuf {
    app_home.file_path(DEMO_MODE_FILENAME)
}

fn load_demo_mode_state(app_home: &AppHome) -> eyre::Result<DemoModeState> {
    let path = demo_mode_path(app_home);
    if !path.exists() {
        return Ok(DemoModeState::default());
    }

    let config = fs::read_to_string(&path)
        .wrap_err_with(|| format!("failed to read demo mode config from {}", path.display()))?;
    Ok(parse_demo_mode_config(&config))
}

fn save_demo_mode_state(app_home: &AppHome, state: DemoModeState) -> eyre::Result<()> {
    // windowing[impl demo-mode.persist-scramble-toggle]
    app_home.ensure_dir()?;
    let path = demo_mode_path(app_home);
    fs::write(&path, demo_mode_config_text(state))
        .wrap_err_with(|| format!("failed to write demo mode config to {}", path.display()))
}

fn parse_demo_mode_config(config: &str) -> DemoModeState {
    let mut state = DemoModeState::default();
    for line in config.lines().map(str::trim) {
        if let Some(value) = line.strip_prefix("scramble-input-device-identifiers=") {
            state.scramble_input_device_identifiers = value.eq_ignore_ascii_case("true")
                || value == "1"
                || value.eq_ignore_ascii_case("yes")
                || value.eq_ignore_ascii_case("on");
        }
    }
    state
}

fn demo_mode_config_text(state: DemoModeState) -> String {
    format!(
        "scramble-input-device-identifiers={}\n",
        state.scramble_input_device_identifiers
    )
}

#[cfg(test)]
mod tests {
    use super::{DemoModeState, demo_mode_config_text, parse_demo_mode_config};

    #[test]
    // windowing[verify demo-mode.persist-scramble-toggle]
    fn demo_mode_config_defaults_to_not_scrambling() {
        assert_eq!(
            parse_demo_mode_config(""),
            DemoModeState {
                scramble_input_device_identifiers: false,
            }
        );
    }

    #[test]
    // windowing[verify demo-mode.persist-scramble-toggle]
    fn demo_mode_config_round_trips_scramble_toggle() {
        let state = DemoModeState {
            scramble_input_device_identifiers: true,
        };

        assert_eq!(parse_demo_mode_config(&demo_mode_config_text(state)), state);
    }
}
