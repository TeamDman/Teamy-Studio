use crate::cli::output::CliOutput;
use arbitrary::Arbitrary;
use facet::Facet;
use figue as args;
use std::path::PathBuf;

/// Run a single Teamy-managed prompt through the Rust Burn lane.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[facet(rename_all = "kebab-case")]
pub struct LlmPromptArgs {
    /// User prompt text to send to the local LLM.
    #[facet(args::positional)]
    pub prompt: String,

    /// Managed model name under Teamy's cache.
    #[facet(args::named, default = crate::llm::model::DEFAULT_LLM_MODEL_NAME.to_owned())]
    pub model: String,

    /// Optional system prompt.
    #[facet(args::named)]
    pub system_prompt: Option<String>,

    /// Explicit managed model directory containing `model.gguf` and tokenizer files.
    #[facet(args::named)]
    pub model_dir: Option<String>,

    /// Maximum number of new tokens to request from the Rust runtime.
    #[facet(args::named, default = crate::llm::runtime::DEFAULT_MAX_NEW_TOKENS)]
    pub max_new_tokens: usize,

    /// Print a Python Transformers reference report before running the Rust prompt.
    #[facet(args::named, default)]
    pub compare_python: bool,

    /// Python reference device, usually `cpu` or `cuda`.
    #[facet(args::named, default = "cpu".to_owned())]
    pub python_device: String,
}

impl LlmPromptArgs {
    /// # Errors
    ///
    /// This function will return an error if the model cannot be inspected, the prompt cannot be
    /// tokenized, or the Rust runtime fails.
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

        if self.compare_python {
            let reference = crate::llm::reference::read_llm_reference_prompt_report(
                &artifacts.metadata.tokenizer_repo_id,
                &self.python_device,
                self.system_prompt.as_deref(),
                &self.prompt,
                1,
                10,
            )?;
            println!("Python reference rendered prompt:\n{}", reference.rendered_prompt);
            println!(
                "Python reference input token count: {}",
                reference.input_token_count
            );
            println!(
                "Python reference top token ids: {:?}",
                reference.top_token_ids
            );
            println!(
                "Python reference top token text: {:?}",
                reference.top_token_text
            );
            println!("Python reference top logits: {:?}", reference.top_logits);
        }

        let result = crate::llm::runtime::run_prompt(
            &artifacts,
            &crate::llm::runtime::LlmPromptRequest {
                system_prompt: self.system_prompt,
                user_prompt: self.prompt,
                max_new_tokens: self.max_new_tokens,
            },
        )?;
        println!("Rendered prompt:\n{}", result.rendered_prompt);
        println!("\nRust Burn output:\n{}", result.output_text);
        Ok(CliOutput::none())
    }
}
