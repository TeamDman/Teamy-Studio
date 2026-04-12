use arbitrary::Arbitrary;
use facet::Facet;

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
    ) -> eyre::Result<()> {
        let _ = app_home;
        let _ = cache_home;
        for window in crate::app::list_terminal_windows()? {
            println!("0x{:X}\t{}\t{}", window.hwnd, window.pid, window.title);
        }
        Ok(())
    }
}
