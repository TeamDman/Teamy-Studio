use arbitrary::Arbitrary;
use eyre::Result;
use facet::Facet;
use figue as args;

/// Persist the default shell command.
/// cli[impl command.surface.shell-default-set]
/// cli[impl shell.default.set.double-dash-trailing-args]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct ShellDefaultSetArgs {
    /// Program to launch as the default shell.
    #[facet(args::positional)]
    pub program: String,

    /// Use `--` before any shell argument that starts with `-` so Teamy Studio
    /// treats it as a trailing shell argument instead of a CLI flag.
    /// Additional arguments passed to the default shell.
    #[facet(args::positional, default)]
    pub args: Vec<String>,
}

impl ShellDefaultSetArgs {
    /// # Errors
    ///
    /// This function will return an error if the default shell cannot be saved.
    #[expect(clippy::unused_async)]
    pub async fn invoke(self, app_home: &crate::paths::AppHome) -> Result<()> {
        crate::shell_default::save_configured_argv(app_home, self.program, self.args)
    }
}
