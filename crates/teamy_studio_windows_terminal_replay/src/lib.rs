pub mod app {
    pub mod teamy_terminal_engine {
        pub use teamy_studio_teamy_terminal_engine::*;
    }
}

#[cfg(feature = "ghostty")]
pub mod windows_terminal_engine {
    pub use teamy_studio_windows_terminal_engine::*;
}

#[path = "../../../src/app/windows_terminal_replay_impl.rs"]
mod windows_terminal_replay_impl;

pub use windows_terminal_replay_impl::*;
