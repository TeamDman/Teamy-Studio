use arbitrary::Arbitrary;
use facet::Facet;
use figue as args;

#[derive(Facet, Arbitrary, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[facet(rename_all = "kebab-case")]
#[repr(u8)]
pub enum TerminalOpenVtEngine {
    Ghostty,
    #[default]
    Teamy,
}

impl From<TerminalOpenVtEngine> for crate::app::VtEngineChoice {
    fn from(value: TerminalOpenVtEngine) -> Self {
        match value {
            TerminalOpenVtEngine::Ghostty => Self::Ghostty,
            TerminalOpenVtEngine::Teamy => Self::Teamy,
        }
    }
}

/// Open a new terminal window.
// cli[impl command.surface.terminal-open]
// cli[impl terminal.open.program-positional]
// cli[impl terminal.open.double-dash-trailing-args]
// cli[impl terminal.open.stdin-flag]
// cli[impl terminal.open.title-flag]
// cli[impl terminal.open.vt-engine-flag]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[facet(rename_all = "kebab-case")]
pub struct TerminalOpenArgs {
    /// Program to launch in the new terminal window.
    #[facet(args::positional)]
    pub program: String,

    /// Optional text to write to terminal stdin after the window is shown.
    #[facet(args::named)]
    pub stdin: Option<String>,

    /// Optional title shown in the terminal chrome.
    #[facet(args::named)]
    pub title: Option<String>,

    /// Select which VT engine backs the new window.
    #[facet(args::named)]
    pub vt_engine: Option<TerminalOpenVtEngine>,

    /// Use `--` before any terminal argument that starts with `-` so Teamy Studio
    /// treats it as a trailing program argument instead of a CLI flag.
    #[facet(args::positional, default)]
    pub args: Vec<String>,
}

impl TerminalOpenArgs {
    /// # Errors
    ///
    /// This function will return an error if the terminal window cannot be launched.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<()> {
        let _ = cache_home;
        let mut command_argv = Vec::with_capacity(self.args.len() + 1);
        command_argv.push(self.program);
        command_argv.extend(self.args);
        crate::app::open_terminal_window(
            app_home,
            &command_argv,
            self.stdin.as_deref(),
            self.title.as_deref(),
            self.vt_engine.unwrap_or_default().into(),
        )
    }
}
