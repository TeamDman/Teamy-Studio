use crate::cli::output::CliOutput;
use arbitrary::Arbitrary;
use eyre::ensure;
use facet::Facet;
use figue as args;

/// Compare Teamy's local LLM setup against the Python Transformers reference harness.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[facet(rename_all = "kebab-case")]
pub struct SelfTestLlmReferenceArgs {
    /// Reference model id to load in Python.
    #[facet(args::named, default = crate::llm::reference::DEFAULT_REFERENCE_MODEL_ID.to_owned())]
    pub model_id: String,

    /// Reference device, usually `cpu` or `cuda`.
    #[facet(args::named, default = "cpu".to_owned())]
    pub device: String,

    /// Optional system prompt for the reference run.
    #[facet(args::named)]
    pub system_prompt: Option<String>,

    /// User prompt text to test.
    #[facet(
        args::named,
        default = "Write a tiny Rust function that adds two i32 values.".to_owned()
    )]
    pub prompt: String,
}

impl SelfTestLlmReferenceArgs {
    /// # Errors
    ///
    /// This function will return an error if the Python LLM reference harness cannot run.
    pub fn invoke(
        self,
        _app_home: &crate::paths::AppHome,
        _cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        let imports = crate::llm::reference::read_llm_reference_import_report()?;
        ensure!(imports.ok, "LLM reference import check reported failure");
        println!(
            "LLM reference imports: torch {}, transformers {}, tokenizers {}, CUDA available: {}",
            imports.torch, imports.transformers, imports.tokenizers, imports.cuda_available
        );
        let config = crate::llm::reference::read_llm_reference_config_report(&self.model_id)?;
        ensure!(config.ok, "LLM reference config report reported failure");
        println!(
            "LLM reference config: class {} model_type {:?} text_model_type {:?}",
            config.config_class, config.model_type, config.text_model_type
        );
        println!(
            "LLM reference text stack: layers {:?} hidden {:?} heads {:?} kv {:?} head_dim {:?}",
            config.text_num_hidden_layers,
            config.text_hidden_size,
            config.text_num_attention_heads,
            config.text_num_key_value_heads,
            config.text_head_dim
        );
        println!(
            "LLM reference hybrid lane: full_attention_interval {:?} histogram {:?}",
            config.text_full_attention_interval, config.text_layer_histogram
        );

        let report = crate::llm::reference::read_llm_reference_prompt_report(
            &self.model_id,
            None,
            &self.device,
            self.system_prompt.as_deref(),
            &self.prompt,
            1,
            10,
        )?;
        ensure!(report.ok, "LLM reference prompt report reported failure");
        println!("LLM reference rendered prompt:\n{}", report.rendered_prompt);
        println!("LLM reference input token count: {}", report.input_token_count);
        println!("LLM reference top token ids: {:?}", report.top_token_ids);
        println!("LLM reference top token text: {:?}", report.top_token_text);
        println!("LLM reference top logits: {:?}", report.top_logits);
        println!("LLM reference one-token decode: {}", report.generated_text);
        Ok(CliOutput::none())
    }
}
