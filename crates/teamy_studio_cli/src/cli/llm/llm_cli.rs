use crate::cli::llm::model::LlmModelArgs;
use crate::cli::llm::prompt::LlmPromptArgs;
use crate::cli::output::CliOutput;
use arbitrary::Arbitrary;
use facet::Facet;
use figue as args;

/// LLM commands.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct LlmArgs {
    /// The LLM subcommand to run.
    #[facet(args::subcommand)]
    pub command: LlmCommand,
}

/// LLM subcommands.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[repr(u8)]
pub enum LlmCommand {
    /// Prepare and inspect local Teamy LLM model artifacts.
    Model(LlmModelArgs),
    /// Run a single local prompt through the Rust Burn runtime.
    Prompt(LlmPromptArgs),
}

impl LlmArgs {
    /// # Errors
    ///
    /// This function will return an error if the selected LLM action fails.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        match self.command {
            LlmCommand::Model(args) => args.invoke(app_home, cache_home),
            LlmCommand::Prompt(args) => args.invoke(app_home, cache_home),
        }
    }
}
