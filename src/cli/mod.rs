pub mod facet_shape;
pub mod global_args;
pub mod self_test;
pub mod window;

use crate::cli::global_args::GlobalArgs;
use crate::cli::self_test::SelfTestArgs;
use crate::cli::window::WindowArgs;
use arbitrary::Arbitrary;
use eyre::Context;
use facet::Facet;
use figue::FigueBuiltins;
use figue::{self as args};

/// Teamy Studio launches a desktop window by default and exposes a small window command surface.
///
/// Environment variables:
/// - `TEAMY_STUDIO_HOME_DIR` overrides the resolved application home directory.
/// - `TEAMY_STUDIO_CACHE_DIR` overrides the resolved cache directory.
/// - `RUST_LOG` provides a tracing filter when `--log-filter` is omitted.
#[derive(Facet, Arbitrary, Debug)]
pub struct Cli {
    /// Global arguments (`debug`, `log_filter`, `log_file`).
    #[facet(flatten)]
    pub global_args: GlobalArgs,

    /// Standard CLI options (help, version, completions).
    #[facet(flatten)]
    #[arbitrary(default)]
    pub builtins: FigueBuiltins,

    /// The command to run.
    #[facet(args::subcommand)]
    pub command: Option<Command>,
}

impl PartialEq for Cli {
    fn eq(&self, other: &Self) -> bool {
        // Ignore builtins in comparison since FigueBuiltins doesn't implement PartialEq
        self.global_args == other.global_args && self.command == other.command
    }
}

impl Cli {
    /// # Errors
    ///
    /// This function will return an error if the tokio runtime cannot be built or if the command fails.
    pub fn invoke(self) -> eyre::Result<()> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .wrap_err("Failed to build tokio runtime")?;
        runtime.block_on(async move {
            match self.command {
                Some(command) => command.invoke().await,
                None => crate::app::run(),
            }
        })?;
        Ok(())
    }
}

/// Teamy Studio commands.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[repr(u8)]
pub enum Command {
    /// Run reproducible self-tests.
    SelfTest(SelfTestArgs),
    /// Launch window-related behaviors.
    Window(WindowArgs),
}

impl Command {
    /// # Errors
    ///
    /// This function will return an error if the subcommand fails.
    pub async fn invoke(self) -> eyre::Result<()> {
        match self {
            Command::SelfTest(args) => args.invoke().await,
            Command::Window(args) => args.invoke().await,
        }
    }
}
