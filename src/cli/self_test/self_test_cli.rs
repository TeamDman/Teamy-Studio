use crate::cli::self_test::keyboard_input::SelfTestKeyboardInputArgs;
use arbitrary::Arbitrary;
use facet::Facet;
use figue as args;

/// Self-test commands for reproducible diagnostics.
/// cli[impl command.surface.self-test]
/// tool[impl cli.surface.self-test]
/// tool[impl cli.help.describes-self-test]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct SelfTestArgs {
    /// The self-test subcommand to run.
    #[facet(args::subcommand)]
    pub command: SelfTestCommand,
}

/// Self-test subcommands.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[repr(u8)]
pub enum SelfTestCommand {
    /// cli[impl command.surface.self-test-keyboard-input]
    /// Run the keyboard input self-test harness.
    KeyboardInput(SelfTestKeyboardInputArgs),
}

impl SelfTestArgs {
    /// # Errors
    ///
    /// This function will return an error if the subcommand fails.
    pub async fn invoke(self, app_home: &crate::paths::AppHome) -> eyre::Result<()> {
        match self.command {
            SelfTestCommand::KeyboardInput(args) => args.invoke(app_home).await,
        }
    }
}
