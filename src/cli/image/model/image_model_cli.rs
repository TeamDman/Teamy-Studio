use crate::cli::output::CliOutput;
use arbitrary::Arbitrary;
use facet::Facet;
use figue as args;
use std::path::PathBuf;

/// Managed Burn image model commands.
// image[impl cli.model-command]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct ImageModelArgs {
    /// The image model subcommand to run.
    #[facet(args::subcommand)]
    pub command: ImageModelCommand,
}

/// Burn image model subcommands.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[repr(u8)]
pub enum ImageModelCommand {
    // image[impl cli.model-list]
    /// List known and managed Burn image model directories.
    List(ImageModelListArgs),
    // image[impl cli.model-prepare]
    /// Prepare a known waifu2x image model into Teamy's cache.
    Prepare(ImageModelPrepareArgs),
    // image[impl cli.model-show]
    /// Show details for a managed or explicit Burn image model directory.
    Show(ImageModelShowArgs),
}

impl ImageModelArgs {
    /// # Errors
    ///
    /// This function will return an error if the selected model action fails.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        match self.command {
            ImageModelCommand::List(args) => args.invoke(app_home, cache_home),
            ImageModelCommand::Prepare(args) => args.invoke(app_home, cache_home),
            ImageModelCommand::Show(args) => args.invoke(app_home, cache_home),
        }
    }
}

/// List known and managed Burn image models.
// image[impl cli.model-list]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct ImageModelListArgs;

impl ImageModelListArgs {
    /// # Errors
    ///
    /// This function will return an error if model listing fails.
    pub fn invoke(
        self,
        _app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        let _ = self;
        Ok(CliOutput::facet(crate::image_model::list_image_models(
            cache_home,
        )))
    }
}

/// Prepare a known waifu2x image model under Teamy's cache image model root.
// image[impl cli.model-prepare]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[facet(rename_all = "kebab-case")]
pub struct ImageModelPrepareArgs {
    /// Known model name to prepare.
    #[facet(args::positional, default = "waifu2x-art-2x".to_owned())]
    pub model: String,

    /// Replace the prepared managed model if it already exists.
    #[facet(args::named, default)]
    pub overwrite: bool,
}

impl ImageModelPrepareArgs {
    /// # Errors
    ///
    /// This function will return an error if image model preparation fails.
    pub fn invoke(
        self,
        _app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        Ok(CliOutput::facet(crate::image_model::prepare_image_model(
            cache_home,
            &self.model,
            self.overwrite,
        )?))
    }
}

/// Show details for a managed image model name or explicit model directory.
// image[impl cli.model-show]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[facet(rename_all = "kebab-case")]
pub struct ImageModelShowArgs {
    /// Managed model name to show.
    #[facet(args::positional, default = "waifu2x-art-2x".to_owned())]
    pub model: String,

    /// Explicit model directory to inspect instead of `{cache_home}/models/image/<model>`.
    #[facet(args::named)]
    pub model_dir: Option<String>,
}

impl ImageModelShowArgs {
    /// # Errors
    ///
    /// This function will return an error if model inspection fails.
    pub fn invoke(
        self,
        _app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        let explicit_model_dir = self.model_dir.as_deref().map(PathBuf::from);
        Ok(CliOutput::facet(crate::image_model::image_model_details(
            cache_home,
            &self.model,
            explicit_model_dir.as_deref(),
        )?))
    }
}
