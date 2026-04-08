use arbitrary::Arbitrary;
use eyre::Result;
use facet::Facet;

/// Delete the cache files.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct CacheCleanArgs;

impl CacheCleanArgs {
    /// # Errors
    ///
    /// This function does not return any errors.
    pub fn invoke() -> Result<()> {
        Ok(())
    }
}
