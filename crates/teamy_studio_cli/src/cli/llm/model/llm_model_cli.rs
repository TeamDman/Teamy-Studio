use crate::cli::output::CliOutput;
use arbitrary::Arbitrary;
use facet::Facet;
use figue as args;
use std::path::PathBuf;

/// Managed Teamy LLM model commands.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct LlmModelArgs {
    /// The model subcommand to run.
    #[facet(args::subcommand)]
    pub command: LlmModelCommand,
}

/// Managed Teamy LLM model subcommands.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[repr(u8)]
pub enum LlmModelCommand {
    /// List known and managed local Teamy LLM model artifacts.
    List(LlmModelListArgs),
    /// Download and register a known model artifact bundle into Teamy's cache.
    Prepare(LlmModelPrepareArgs),
    /// Show details for a managed or explicit LLM model directory.
    Show(LlmModelShowArgs),
}

impl LlmModelArgs {
    /// # Errors
    ///
    /// This function will return an error if the selected model action fails.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        match self.command {
            LlmModelCommand::List(args) => args.invoke(app_home, cache_home),
            LlmModelCommand::Prepare(args) => args.invoke(app_home, cache_home),
            LlmModelCommand::Show(args) => args.invoke(app_home, cache_home),
        }
    }
}

#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct LlmModelListArgs;

impl LlmModelListArgs {
    /// # Errors
    ///
    /// This function will return an error if the Teamy LLM model registry cannot be read.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        let _ = self;
        println!(
            "managed_llm_model_root: {}",
            crate::llm::model::managed_models_dir(cache_home).display()
        );
        println!("known_models:");
        let statuses = crate::llm::model::list_known_llm_model_statuses(app_home, cache_home)?;
        for status in statuses {
            let known = crate::llm::model::known_llm_model(&status.model_name)
                .ok_or_else(|| eyre::eyre!("Missing known LLM model {}", status.model_name))?;
            println!("  {}:", known.name);
            println!("    display_name: {}", known.display_name);
            println!("    architecture: {}", known.architecture);
            println!("    quantization: {}", known.quantization);
            println!("    parameter_count: {}", known.parameter_count);
            println!("    size_estimate: {}", known.size_estimate);
            println!("    supports_vision: {}", known.supports_vision);
            println!("    supports_tool_calling: {}", known.supports_tool_calling);
            println!("    state: {:?}", status.state);
            for location in status.locations {
                println!(
                    "    location[{}]: {} (exists: {}, compatible: {})",
                    location.label,
                    location.path.display(),
                    location.exists,
                    location.compatible
                );
            }
        }
        println!("registered_model_directories:");
        println!(
            "{}",
            crate::llm::model::render_registered_model_dirs(
                &crate::llm::model::list_registered_model_dirs(app_home)?
            )
        );
        Ok(CliOutput::none())
    }
}

#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[facet(rename_all = "kebab-case")]
pub struct LlmModelPrepareArgs {
    /// Known model name to prepare.
    #[facet(args::positional, default = crate::llm::model::DEFAULT_LLM_MODEL_NAME.to_owned())]
    pub model: String,

    /// Replace the prepared managed model if it already exists.
    #[facet(args::named, default)]
    pub overwrite: bool,

    /// Also download the mmproj vision projection file beside the main model artifact.
    #[facet(args::named, default)]
    pub with_mmproj: bool,

    /// Also export a lazy-load Burn text runtime bundle beside the managed model directory.
    #[facet(args::named, default)]
    pub with_burn_text: bool,

    /// Floating-point storage dtype for the converted Burn text bundle.
    #[facet(
        args::named,
        default = crate::llm::burn_text::DEFAULT_BURN_TEXT_EXPORT_DTYPE.to_owned()
    )]
    pub burn_text_dtype: String,

    /// Skip the large GGUF download and prepare a Burn-text-only managed directory instead.
    #[facet(args::named, default)]
    pub burn_text_only: bool,
}

impl LlmModelPrepareArgs {
    /// # Errors
    ///
    /// This function will return an error if the model cannot be downloaded, validated, or registered.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        let prepared = crate::llm::model::prepare_known_llm_model(
            app_home,
            cache_home,
            &self.model,
            self.overwrite,
            self.with_mmproj,
            !self.burn_text_only,
        )?;
        println!(
            "Prepared managed LLM model directory: {}",
            prepared.managed_dir.display()
        );
        println!(
            "{}",
            crate::llm::model::render_model_report(&prepared.artifacts)
        );
        if self.with_burn_text {
            println!(
                "{}",
                crate::llm::model::prepare_burn_text_runtime_bundle(
                    &prepared.artifacts,
                    self.overwrite,
                    Some(&self.burn_text_dtype),
                )?
            );
            let refreshed = crate::llm::model::inspect_model_dir(&prepared.managed_dir)?;
            println!("{}", crate::llm::model::render_model_report(&refreshed));
        }
        if let Some(warning) = &prepared.registration_warning {
            eprintln!("warning: {warning}");
        }
        println!(
            "Registered model directory list:\n{}",
            crate::llm::model::render_registered_model_dirs(&prepared.registered_model_dirs)
        );
        Ok(CliOutput::none())
    }
}

#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[facet(rename_all = "kebab-case")]
pub struct LlmModelShowArgs {
    /// Managed model name to show.
    #[facet(args::positional, default = crate::llm::model::DEFAULT_LLM_MODEL_NAME.to_owned())]
    pub model: String,

    /// Explicit model directory to inspect instead of Teamy's managed cache directory.
    #[facet(args::named)]
    pub model_dir: Option<String>,
}

impl LlmModelShowArgs {
    /// # Errors
    ///
    /// This function will return an error if the selected model directory cannot be inspected.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        let explicit_model_dir = self.model_dir.as_deref().map(PathBuf::from);
        let resolved = crate::llm::model::resolve_llm_model_dir(
            app_home,
            cache_home,
            Some(&self.model),
            explicit_model_dir.as_deref(),
        )?;
        let artifacts = crate::llm::model::inspect_model_dir(&resolved)?;
        println!("{}", crate::llm::model::render_model_report(&artifacts));
        Ok(CliOutput::none())
    }
}
