use arbitrary::Arbitrary;
use eyre::Result;
use facet::Facet;

/// Show the cache path.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct CacheShowArgs;

impl CacheShowArgs {
    /// # Errors
    ///
    /// This function does not return any errors.
    pub fn invoke() -> Result<()> {
        println!("{}", crate::paths::CACHE_DIR.display());
        Ok(())
    }
}
