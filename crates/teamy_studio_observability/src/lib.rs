#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoggingConfig {
    pub debug: bool,
    pub log_filter: Option<String>,
    pub log_file: Option<String>,
}

#[path = "../../../src/logging_init_impl.rs"]
mod logging_init_impl;

pub use logging_init_impl::*;