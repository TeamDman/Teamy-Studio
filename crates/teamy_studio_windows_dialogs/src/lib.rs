pub mod win32_support {
    pub mod string {
        pub use teamy_studio_win32_support::string::*;
    }
}

#[path = "../../../src/app/windows_dialogs_impl.rs"]
mod windows_dialogs_impl;

pub use windows_dialogs_impl::*;
