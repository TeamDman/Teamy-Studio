pub mod paths {
    pub use teamy_studio_paths::*;
}

pub mod win32_support {
    pub mod string {
        pub use teamy_studio_win32_support::string::*;
    }
}

mod windows_audio_impl;

pub use windows_audio_impl::*;
