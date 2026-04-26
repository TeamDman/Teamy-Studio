use arbitrary::Arbitrary;
use eyre::Context;
use facet::Facet;
use figue as args;
use std::path::PathBuf;

use crate::cli::output::CliOutput;

/// Managed Burn Whisper model commands.
// audio[impl cli.model-command]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct AudioModelArgs {
    /// The model subcommand to run.
    #[facet(args::subcommand)]
    pub command: AudioModelCommand,
}

/// Burn Whisper model subcommands.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[repr(u8)]
pub enum AudioModelCommand {
    // audio[impl cli.model-list]
    /// List known and registered Burn Whisper model directories.
    List(AudioModelListArgs),
    // audio[impl cli.model-prepare]
    /// Download and convert a known Whisper model into Teamy's cache.
    Prepare(AudioModelPrepareArgs),
    // audio[impl cli.model-show]
    /// Show details for a managed or explicit Burn Whisper model directory.
    Show(AudioModelShowArgs),
}

impl AudioModelArgs {
    /// # Errors
    ///
    /// This function will return an error if the selected model action fails.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        match self.command {
            AudioModelCommand::List(args) => args.invoke(app_home, cache_home),
            AudioModelCommand::Prepare(args) => args.invoke(app_home, cache_home),
            AudioModelCommand::Show(args) => args.invoke(app_home, cache_home),
        }
    }
}

/// List known and registered Burn Whisper models.
// audio[impl cli.model-list]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct AudioModelListArgs;

impl AudioModelListArgs {
    /// # Errors
    ///
    /// This function will return an error if the registered model list cannot be read.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        let _ = self;
        println!(
            "Managed model root: {}",
            crate::model::managed_models_dir(cache_home).display()
        );
        println!("Known models:");
        for known in &crate::model::KNOWN_WHISPER_MODELS {
            let managed = crate::model::managed_model_dir(cache_home, known.name);
            let status = if managed.is_dir() {
                "prepared"
            } else {
                "missing"
            };
            println!("- {}: {} ({})", known.name, managed.display(), status);
        }
        println!(
            "Registered model directories:\n{}",
            crate::model::render_registered_model_dirs(&crate::model::list_registered_model_dirs(
                app_home
            )?)
        );
        Ok(CliOutput::none())
    }
}

/// Prepare a known Whisper model under Teamy's cache model root.
// audio[impl cli.model-prepare]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[facet(rename_all = "kebab-case")]
pub struct AudioModelPrepareArgs {
    /// Known model name to prepare.
    #[facet(args::positional, default = crate::model::DEFAULT_TRANSCRIPTION_MODEL_NAME.to_owned())]
    pub model: String,

    /// Replace the prepared managed model if it already exists.
    #[facet(args::named, default)]
    pub overwrite: bool,
}

impl AudioModelPrepareArgs {
    /// # Errors
    ///
    /// This function will return an error if the model cannot be downloaded, converted, or registered.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        let prepared = crate::model::prepare_known_whisper_model(
            app_home,
            cache_home,
            &self.model,
            self.overwrite,
        )?;
        println!(
            "Prepared managed model directory: {}",
            prepared.managed_dir.display()
        );
        println!("{}", crate::model::render_model_report(&prepared.artifacts));
        println!(
            "Registered model directory list:\n{}",
            crate::model::render_registered_model_dirs(&prepared.registered_model_dirs)
        );
        Ok(CliOutput::none())
    }
}

/// Show details for a managed model name or explicit model directory.
// audio[impl cli.model-show]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[facet(rename_all = "kebab-case")]
pub struct AudioModelShowArgs {
    /// Managed model name to show.
    #[facet(args::positional, default = crate::model::DEFAULT_TRANSCRIPTION_MODEL_NAME.to_owned())]
    pub model: String,

    /// Explicit model directory to inspect instead of `{cache_home}/models/<model>`.
    #[facet(args::named)]
    pub model_dir: Option<String>,
}

impl AudioModelShowArgs {
    /// # Errors
    ///
    /// This function will return an error if the selected model directory cannot be inspected.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        let explicit = self.model_dir.as_deref().map(PathBuf::from);
        let model_dir = crate::model::resolve_transcription_model_dir(
            app_home,
            cache_home,
            Some(&self.model),
            explicit.as_deref(),
        )?;
        let artifacts = crate::model::inspect_model_dir(&model_dir).wrap_err_with(|| {
            format!("failed to inspect model directory {}", model_dir.display())
        })?;
        println!("{}", crate::model::render_model_report(&artifacts));
        Ok(CliOutput::none())
    }
}
