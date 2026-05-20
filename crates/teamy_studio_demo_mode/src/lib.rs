pub mod paths {
    pub use teamy_studio_paths::*;
}

#[path = "../../../src/app/windows_demo_mode_impl.rs"]
mod windows_demo_mode_impl;

pub use windows_demo_mode_impl::*;
