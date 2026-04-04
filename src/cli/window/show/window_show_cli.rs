use arbitrary::Arbitrary;
use facet::Facet;

/// Show the main Teamy Studio window.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct WindowShowArgs;

impl WindowShowArgs {
    /// # Errors
    ///
    /// This function will return an error if the application window cannot be created.
    #[expect(clippy::unused_async)]
    pub async fn invoke(self) -> eyre::Result<()> {
        crate::app::run()
    }
}
