use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::UI::WindowsAndMessaging::MB_OK;

const MINIMUM_BELL_INTERVAL: Duration = Duration::from_millis(100);

// behavior[impl window.interaction.output.bell.audible]
pub fn ring_terminal_bell() {
    static LAST_BELL_AT: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

    let last_bell_at = LAST_BELL_AT.get_or_init(|| Mutex::new(None));
    let Ok(mut last_bell_at) = last_bell_at.lock() else {
        return;
    };

    let now = Instant::now();
    if !should_ring_bell(now, *last_bell_at) {
        return;
    }

    *last_bell_at = Some(now);

    // Safety: `MessageBeep` is a process-local best-effort notification with no raw pointers.
    let _ = unsafe { MessageBeep(MB_OK) };
}

fn should_ring_bell(now: Instant, last_bell_at: Option<Instant>) -> bool {
    last_bell_at.is_none_or(|last_bell_at| {
        now.saturating_duration_since(last_bell_at) >= MINIMUM_BELL_INTERVAL
    })
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{MINIMUM_BELL_INTERVAL, should_ring_bell};

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
}
