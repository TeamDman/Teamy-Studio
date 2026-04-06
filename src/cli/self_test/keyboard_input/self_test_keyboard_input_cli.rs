use arbitrary::Arbitrary;
use facet::Facet;
use figue::{self as args};

/// Run the keyboard input self-test harness.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[facet(rename_all = "kebab-case")]
pub struct SelfTestKeyboardInputArgs {
    /// Run the terminal-side probe instead of the outer harness.
    #[facet(args::named, default)]
    pub inside: bool,
}

impl SelfTestKeyboardInputArgs {
    /// # Errors
    ///
    /// This function will return an error if the keyboard input self-test fails.
    #[expect(clippy::unused_async)]
    pub async fn invoke(self) -> eyre::Result<()> {
        crate::app::run_keyboard_input_self_test(self.inside)
    }
}