use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::paths::AppHome;
use eyre::Context;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::UI::WindowsAndMessaging::MB_OK;
use windows::core::PCWSTR;

const MINIMUM_BELL_INTERVAL: Duration = Duration::from_millis(100);
const BELL_SOURCE_FILENAME: &str = "bell-source.txt";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum BellSource {
    #[default]
    Windows,
    File(PathBuf),
}

#[derive(Debug, Default)]
struct BellState {
    last_bell_at: Option<Instant>,
    source: BellSource,
}

// behavior[impl window.interaction.output.bell.audible]
pub fn ring_terminal_bell() {
    let Ok(mut bell_state) = bell_state().lock() else {
        return;
    };

    let now = Instant::now();
    if !should_ring_bell(now, bell_state.last_bell_at) {
        return;
    }

    bell_state.last_bell_at = Some(now);
    let source = bell_state.source.clone();
    drop(bell_state);

    match source {
        BellSource::Windows => ring_windows_bell(),
        BellSource::File(path) => ring_audio_file(&path),
    }
}

pub fn initialize_bell_source(app_home: &AppHome) -> eyre::Result<()> {
    let source = load_bell_source(app_home)?;
    let mut bell_state = bell_state()
        .lock()
        .map_err(|error| eyre::eyre!("failed to lock bell state: {error}"))?;
    bell_state.source = source;
    Ok(())
}

pub fn current_bell_source() -> BellSource {
    bell_state()
        .lock()
        .map(|state| state.source.clone())
        .unwrap_or_default()
}

pub fn current_bell_source_label() -> String {
    match current_bell_source() {
        BellSource::Windows => "Windows".to_owned(),
        BellSource::File(path) => path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string()),
    }
}

pub fn set_bell_source(app_home: &AppHome, source: BellSource) -> eyre::Result<()> {
    save_bell_source(app_home, &source)?;
    let mut bell_state = bell_state()
        .lock()
        .map_err(|error| eyre::eyre!("failed to lock bell state: {error}"))?;
    bell_state.source = source;
    Ok(())
}

fn bell_state() -> &'static Mutex<BellState> {
    static BELL_STATE: OnceLock<Mutex<BellState>> = OnceLock::new();

    BELL_STATE.get_or_init(|| Mutex::new(BellState::default()))
}

fn ring_windows_bell() {
    // Safety: `MessageBeep` is a process-local best-effort notification with no raw pointers.
    let _ = unsafe { MessageBeep(MB_OK) };
}

fn ring_audio_file(path: &Path) {
    if !path.exists() {
        ring_windows_bell();
        return;
    }

    let wide_path: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // Safety: the UTF-16 path is null-terminated and valid for the duration of the call.
    let result = unsafe {
        PlaySoundW(
            PCWSTR(wide_path.as_ptr()),
            None,
            SND_ASYNC | SND_FILENAME | SND_NODEFAULT,
        )
    };
    if !result.as_bool() {
        ring_windows_bell();
    }
}

fn bell_source_path(app_home: &AppHome) -> PathBuf {
    app_home.file_path(BELL_SOURCE_FILENAME)
}

fn load_bell_source(app_home: &AppHome) -> eyre::Result<BellSource> {
    let path = bell_source_path(app_home);
    if !path.exists() {
        return Ok(BellSource::Windows);
    }

    let config = fs::read_to_string(&path)
        .wrap_err_with(|| format!("failed to read bell source config from {}", path.display()))?;
    Ok(parse_bell_source_config(&config))
}

fn save_bell_source(app_home: &AppHome, source: &BellSource) -> eyre::Result<()> {
    app_home.ensure_dir()?;
    let path = bell_source_path(app_home);
    fs::write(&path, bell_source_config_text(source))
        .wrap_err_with(|| format!("failed to write bell source config to {}", path.display()))
}

fn parse_bell_source_config(config: &str) -> BellSource {
    let trimmed = config.trim();
    if trimmed.eq_ignore_ascii_case("windows") {
        return BellSource::Windows;
    }

    if let Some(path) = trimmed.strip_prefix("file\t") {
        return BellSource::File(PathBuf::from(path));
    }

    BellSource::Windows
}

fn bell_source_config_text(source: &BellSource) -> String {
    match source {
        BellSource::Windows => "windows\n".to_owned(),
        BellSource::File(path) => format!("file\t{}\n", path.display()),
    }
}

fn should_ring_bell(now: Instant, last_bell_at: Option<Instant>) -> bool {
    last_bell_at.is_none_or(|last_bell_at| {
        now.saturating_duration_since(last_bell_at) >= MINIMUM_BELL_INTERVAL
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use super::{
        BELL_SOURCE_FILENAME, BellSource, MINIMUM_BELL_INTERVAL, bell_source_config_text,
        parse_bell_source_config, should_ring_bell,
    };

    #[test]
    fn bell_rings_when_no_prior_bell_has_been_recorded() {
        assert!(should_ring_bell(Instant::now(), None));
    }

    #[test]
    fn bell_is_suppressed_inside_the_minimum_interval() {
        let now = Instant::now();
        let last_bell_at = now - Duration::from_millis(1);

        assert!(!should_ring_bell(now, Some(last_bell_at)));
    }

    #[test]
    fn bell_rings_again_after_the_minimum_interval_expires() {
        let now = Instant::now();
        let last_bell_at = now - MINIMUM_BELL_INTERVAL - Duration::from_millis(1);

        assert!(should_ring_bell(now, Some(last_bell_at)));
    }

    #[test]
    fn bell_source_config_roundtrips_windows_source() {
        assert_eq!(
            parse_bell_source_config(&bell_source_config_text(&BellSource::Windows)),
            BellSource::Windows,
        );
    }

    #[test]
    fn bell_source_config_roundtrips_file_source() {
        let source = BellSource::File(PathBuf::from("C:/bell.wav"));

        assert_eq!(parse_bell_source_config(&bell_source_config_text(&source)), source);
    }

    #[test]
    fn bell_source_filename_is_stable() {
        assert_eq!(BELL_SOURCE_FILENAME, "bell-source.txt");
    }
}
