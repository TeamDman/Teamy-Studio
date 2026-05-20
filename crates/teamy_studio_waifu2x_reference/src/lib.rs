use std::path::PathBuf;

#[must_use]
pub fn repo_root_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

#[path = "../../../src/waifu2x_reference_impl.rs"]
mod waifu2x_reference_impl;

pub use waifu2x_reference_impl::*;
