use arbitrary::Arbitrary;
use eyre::Result;
use facet::Facet;
use figue as args;

/// Persist the default shell command.
// cli[impl command.surface.terminal-default-shell-set]
// cli[impl shell.default.set.double-dash-trailing-args]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct TerminalDefaultShellSetArgs {
    /// Program to launch as the default shell.
    #[facet(args::positional)]
    pub program: String,

    /// Use `--` before any shell argument that starts with `-` so Teamy Studio
    /// treats it as a trailing shell argument instead of a CLI flag.
    /// Additional arguments passed to the default shell.
    #[facet(args::positional, default)]
    pub args: Vec<String>,
}

impl TerminalDefaultShellSetArgs {
    /// # Errors
    ///
    /// This function will return an error if the default shell cannot be saved.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> Result<()> {
        let _ = cache_home;
        crate::shell_default::save_configured_argv(app_home, self.program, self.args)
    }
}
