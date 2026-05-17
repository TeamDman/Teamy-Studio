use arbitrary::Arbitrary;
use facet::Facet;

use crate::app::TerminalWindowSummary;
use crate::cli::output::CliOutput;

#[derive(Facet, Debug)]
struct TerminalListReport {
    windows: Vec<TerminalWindowSummary>,
}

/// Enumerate live Teamy Studio terminal windows.
// cli[impl command.surface.terminal-list]
// cli[impl terminal.list.enumerates-live-windows]
// cli[impl terminal.list.prints-hwnd-pid-and-title]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct TerminalListArgs;

impl TerminalListArgs {
    /// # Errors
    ///
    /// This function will return an error if the terminal windows cannot be listed.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        let _ = app_home;
        let _ = cache_home;
        Ok(CliOutput::facet(TerminalListReport {
            windows: crate::app::list_terminal_windows()?,
        }))
    }
}
