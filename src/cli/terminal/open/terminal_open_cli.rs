use arbitrary::Arbitrary;
use facet::Facet;
use figue as args;

use crate::cli::output::CliOutput;

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
// cli[impl terminal.open.default-shell-when-program-omitted]
// cli[impl terminal.open.double-dash-trailing-args]
// cli[impl terminal.open.stdin-flag]
// cli[impl terminal.open.title-flag]
// cli[impl terminal.open.vt-engine-flag]
#[derive(Facet, Debug, PartialEq)]
#[facet(rename_all = "kebab-case")]
pub struct TerminalOpenArgs {
    /// Optional program to launch in the new terminal window.
    #[facet(args::positional)]
    pub program: Option<String>,

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

impl<'a> Arbitrary<'a> for TerminalOpenArgs {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let program = Option::<String>::arbitrary(u)?;
        let stdin = Option::<String>::arbitrary(u)?;
        let title = Option::<String>::arbitrary(u)?;
        let vt_engine = Option::<TerminalOpenVtEngine>::arbitrary(u)?;
        let mut args = Vec::<String>::arbitrary(u)?;
        if program.is_none() {
            args.clear();
        }
        Ok(Self {
            program,
            stdin,
            title,
            vt_engine,
            args,
        })
    }
}

impl TerminalOpenArgs {
    /// # Errors
    ///
    /// This function will return an error if the terminal window cannot be launched.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        let _ = cache_home;
        let command_argv = self.program.map(|program| {
            let mut command_argv = Vec::with_capacity(self.args.len() + 1);
            command_argv.push(program);
            command_argv.extend(self.args);
            command_argv
        });
        crate::app::open_terminal_window(
            app_home,
            command_argv.as_deref(),
            self.stdin.as_deref(),
            self.title.as_deref(),
            self.vt_engine.unwrap_or_default().into(),
        )?;
        Ok(CliOutput::none())
    }
}

#[cfg(test)]
mod tests {
    use crate::cli::terminal::open::{TerminalOpenArgs, TerminalOpenVtEngine};
    use crate::cli::terminal::{TerminalArgs, TerminalCommand};
    use crate::cli::{Cli, Command};

    #[test]
    fn terminal_open_parser_allows_omitting_program() {
        let cli: Cli = figue::Driver::new(
            figue::builder::<Cli>()
                .expect("schema should be valid")
                .cli(move |cli| {
                    cli.args(["terminal", "open", "--title", "scratch"])
                        .strict()
                })
                .build(),
        )
        .run()
        .unwrap();

        assert_eq!(
            cli,
            Cli {
                global_args: Default::default(),
                builtins: Default::default(),
                command: Some(Command::Terminal(TerminalArgs {
                    command: TerminalCommand::Open(TerminalOpenArgs {
                        program: None,
                        stdin: None,
                        title: Some("scratch".to_owned()),
                        vt_engine: None,
                        args: Vec::new(),
                    }),
                })),
            }
        );
    }

    #[test]
    fn terminal_open_parser_keeps_explicit_program_and_flags() {
        let cli: Cli = figue::Driver::new(
            figue::builder::<Cli>()
                .expect("schema should be valid")
                .cli(move |cli| {
                    cli.args([
                        "terminal",
                        "open",
                        "pwsh",
                        "--title",
                        "scratch",
                        "--vt-engine",
                        "teamy",
                        "--",
                        "-NoProfile",
                    ])
                    .strict()
                })
                .build(),
        )
        .run()
        .unwrap();

        assert_eq!(
            cli,
            Cli {
                global_args: Default::default(),
                builtins: Default::default(),
                command: Some(Command::Terminal(TerminalArgs {
                    command: TerminalCommand::Open(TerminalOpenArgs {
                        program: Some("pwsh".to_owned()),
                        stdin: None,
                        title: Some("scratch".to_owned()),
                        vt_engine: Some(TerminalOpenVtEngine::Teamy),
                        args: vec!["-NoProfile".to_owned()],
                    }),
                })),
            }
        );
    }
}
