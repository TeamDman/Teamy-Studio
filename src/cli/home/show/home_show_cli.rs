use arbitrary::Arbitrary;
use eyre::Result;
use facet::Facet;

/// Show the home path.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct HomeShowArgs;

impl HomeShowArgs {
    /// # Errors
    ///
    /// This function does not return any errors.
    pub fn invoke() -> Result<()> {
        println!("{}", crate::paths::APP_HOME.display());
        Ok(())
    }
}
