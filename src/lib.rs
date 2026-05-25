#![deny(clippy::disallowed_methods)]
#![deny(clippy::disallowed_macros)]

pub mod app {
    pub use teamy_studio_cli::app::*;
}

pub mod audio {
    pub use teamy_studio_audio_core::*;
}

pub mod cli {
    pub use teamy_studio_cli::*;
}

pub mod frontend {
    pub use teamy_studio_frontend::*;
}

pub mod image_model {
    pub use teamy_studio_image_models::*;
}

pub mod llm {
    pub use teamy_studio_llm_stack::*;
}

pub mod logging_init {
    use crate::cli::global_args::GlobalArgs;
    use teamy_studio_observability::LoggingConfig;

    /// Initialize logging based on the provided configuration.
    /// tool[impl logging.stderr-output]
    /// tool[impl logging.file-path-option]
    /// tool[impl logging.file-structured-ndjson]
    /// tool[impl logging.filter.from-env]
    /// tool[impl logging.filter.defaults]
    /// tool[impl logging.filter.debug-conflicts-with-log-filter]
    ///
    /// # Errors
    ///
    /// This function will return an error if creating the log file or directories fails.
    ///
    /// # Panics
    ///
    /// This function may panic if locking or cloning the log file handle fails.
    pub fn init_logging(global_args: &GlobalArgs) -> eyre::Result<()> {
        let config = LoggingConfig {
            debug: global_args.debug,
            log_filter: global_args.log_filter.clone(),
            log_file: global_args.log_file.clone(),
        };

        teamy_studio_observability::init_logging(&config)
    }
}

pub mod logs {
    pub use teamy_studio_logs::*;
}

pub mod model {
    pub use teamy_studio_whisper_stack::model::*;
}

pub mod paths {
    pub use teamy_studio_paths::*;
}

pub mod shell_default {
    pub use teamy_studio_shell_default::*;
}

pub use teamy_studio_timeline_core as timeline;

pub mod transcription {
    pub use teamy_studio_whisper_stack::transcription::*;
}

pub mod waifu2x_reference {
    pub use teamy_studio_waifu2x_reference::*;
}

pub mod whisper {
    pub use teamy_studio_whisper_stack::whisper::*;
}

pub mod win32_support {
    pub use teamy_studio_win32_support::*;
}

use crate::cli::Cli;
use crate::cli::output::CliOutput;

/// Version string combining package version and git revision.
/// tool[impl cli.version.includes-semver]
/// tool[impl cli.version.includes-git-revision]
const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (rev ",
    env!("GIT_REVISION"),
    ")"
);

/// Entrypoint for the program.
/// tool[impl cli.help.position-independent]
///
/// # Errors
///
/// This function will return an error if `color_eyre` installation, CLI parsing, logging initialization, or command execution fails.
///
/// # Panics
///
/// Panics if the CLI schema is invalid (should never happen with correct code).
pub fn main() -> eyre::Result<()> {
    // Install color_eyre for better error reports
    color_eyre::install()?;

    // Enable ANSI support on Windows.
    // This fails in a pipe scenario, so we ignore the error.
    let _ = win32_support::console::enable_ansi_support();

    win32_support::string::warn_if_utf8_not_enabled();

    // Parse command line arguments using figue
    // unwrap() is figue's intended CLI entry behavior:
    // it exits with proper codes for --help/--version/completions/parse-errors.
    let cli: Cli = figue::Driver::new(
        figue::builder::<Cli>()
            .expect("schema should be valid")
            .cli(move |cli| cli.args_os(std::env::args_os().skip(1)).strict())
            .help(move |help| {
                help.version(VERSION)
                    .include_implementation_source_file(true)
                    .include_implementation_git_url("TeamDman/Teamy-Studio", env!("GIT_REVISION"))
            })
            .build(),
    )
    .run()
    .unwrap();

    // Initialize logging
    logging_init::init_logging(&cli.global_args)?;

    // Invoke whatever command was requested
    let output_format = cli.global_args.output_format;
    let output: CliOutput = cli.invoke()?;
    output.emit(output_format)?;
    Ok(())
}
