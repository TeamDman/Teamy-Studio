#![deny(clippy::disallowed_methods)]
#![deny(clippy::disallowed_macros)]

pub mod app;
pub mod audio;
pub mod cli;
pub mod frontend;
pub mod logging_init;
pub mod logs;
pub mod model;
pub mod paths;
pub mod shell_default;
pub mod timeline;
pub mod transcription;
pub mod whisper;
pub mod win32_support;

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
