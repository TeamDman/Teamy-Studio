use crate::cli::image::model::ImageModelArgs;
use crate::cli::image::upscale::ImageUpscaleArgs;
use crate::cli::output::CliOutput;
use arbitrary::Arbitrary;
use facet::Facet;
use figue as args;

/// Image commands.
// image[impl cli.image-command]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct ImageArgs {
    /// The image subcommand to run.
    #[facet(args::subcommand)]
    pub command: ImageCommand,
}

/// Image subcommands.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[repr(u8)]
pub enum ImageCommand {
    // image[impl cli.model-command]
    /// Prepare and inspect local Burn image models.
    Model(ImageModelArgs),
    // image[impl cli.upscale-command]
    /// Upscale one image asset with the Rust Burn waifu2x backend.
    Upscale(ImageUpscaleArgs),
}

impl ImageArgs {
    /// # Errors
    ///
    /// This function will return an error if the selected image action fails.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        match self.command {
            ImageCommand::Model(args) => args.invoke(app_home, cache_home),
            ImageCommand::Upscale(args) => args.invoke(app_home, cache_home),
        }
    }
}
