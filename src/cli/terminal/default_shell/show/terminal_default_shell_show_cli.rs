use arbitrary::Arbitrary;
use eyre::Result;
use facet::Facet;

use crate::cli::output::CliOutput;

#[derive(Facet, Debug)]
struct TerminalDefaultShellShowReport {
    argv: Vec<String>,
    command_line: String,
}

/// Show the effective default shell command.
// cli[impl command.surface.terminal-default-shell-show]
// cli[impl shell.default.show-effective]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct TerminalDefaultShellShowArgs;

impl TerminalDefaultShellShowArgs {
    /// # Errors
    ///
    /// This function will return an error if the default shell cannot be loaded.
    pub fn invoke(
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> Result<CliOutput> {
        let _ = cache_home;
        let argv = crate::shell_default::load_effective_argv(app_home)?;
        let command_line = crate::shell_default::format_command_line(&argv);
        Ok(CliOutput::facet(TerminalDefaultShellShowReport {
            argv,
            command_line,
        }))
    }
}
