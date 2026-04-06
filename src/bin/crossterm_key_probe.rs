#[cfg(windows)]
use std::io::Write;

#[cfg(windows)]
fn main() -> eyre::Result<()> {
    use crossterm::event::{self, Event};
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

    let event_limit = std::env::var("TEAMY_KEY_PROBE_EVENT_LIMIT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(16);

    enable_raw_mode()?;
    let _raw_mode_guard = RawModeGuard;

    println!("CROSSTERM_KEY_PROBE_READY");
    std::io::stdout().flush()?;

    let mut events_seen = 0_usize;
    while events_seen < event_limit {
        if !event::poll(std::time::Duration::from_secs(5))? {
            continue;
        }

        if let Event::Key(key) = event::read()? {
            events_seen += 1;
            println!("EVENT E{events_seen:02} {key:?}");
            std::io::stdout().flush()?;
        }
    }

    disable_raw_mode()?;
    Ok(())
}

#[cfg(not(windows))]
fn main() -> eyre::Result<()> {
    eyre::bail!("crossterm-key-probe only supports Windows")
}

#[cfg(windows)]
struct RawModeGuard;

#[cfg(windows)]
impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}