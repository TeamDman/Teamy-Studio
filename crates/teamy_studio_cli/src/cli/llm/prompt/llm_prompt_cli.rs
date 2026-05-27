use crate::cli::output::CliOutput;
use arbitrary::Arbitrary;
use facet::Facet;
use figue as args;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

const LLM_TIMEOUT_CHILD_ENV: &str = "TEAMY_STUDIO_LLM_TIMEOUT_CHILD";
const LLM_TIMEOUT_KILL_GRACE: Duration = Duration::from_secs(10);

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

    /// Optional wall-clock timeout for token generation, for example `5m` or `90s`.
    #[facet(args::named)]
    pub timeout: Option<String>,

    /// Deprecated compatibility alias for `--timeout`.
    #[facet(args::named)]
    pub generation_timeout: Option<String>,

    /// Print a Python Transformers reference report before running the Rust prompt.
    #[facet(args::named, default)]
    pub compare_python: bool,

    /// Compare Rust Burn per-layer outputs against the Python reference model.
    #[facet(args::named, default)]
    pub compare_python_layers: bool,

    /// Compare the stable Burn generation path against the experimental incremental Burn path.
    #[facet(args::named, default)]
    pub compare_incremental: bool,

    /// Add hidden-state diagnostics for the experimental incremental Burn path.
    #[facet(args::named, default)]
    pub compare_incremental_hidden: bool,

    /// Add per-layer hidden diagnostics for the experimental incremental Burn path.
    #[facet(args::named, default)]
    pub compare_incremental_layers: bool,

    /// Python reference device, usually `cpu` or `cuda`.
    #[facet(args::named, default = "cpu".to_owned())]
    pub python_device: String,

    /// Optional local model path for the Python reference instead of a Hugging Face repo id.
    #[facet(args::named)]
    pub python_model_path: Option<String>,

    /// Optional tokenizer directory for the Python reference.
    #[facet(args::named)]
    pub python_tokenizer_path: Option<String>,
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
        let generation_timeout = resolve_timeout_arg(
            self.timeout.as_deref(),
            self.generation_timeout.as_deref(),
        )?;
        if let Some(timeout) = generation_timeout
            && !is_timeout_supervision_child()
        {
            return supervise_llm_prompt_timeout(timeout);
        }
        maybe_warn_about_dev_profile();
        let explicit_model_dir = self.model_dir.as_deref().map(PathBuf::from);
        let resolved = crate::llm::model::resolve_llm_model_dir(
            app_home,
            cache_home,
            Some(&self.model),
            explicit_model_dir.as_deref(),
        )?;
        let artifacts = crate::llm::model::inspect_model_dir(&resolved)?;

        let python_model_source = self
            .python_model_path
            .as_deref()
            .unwrap_or(artifacts.metadata.source_repo_id.as_str());
        let python_tokenizer_path = self
            .python_tokenizer_path
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| artifacts.root.clone());

        if self.compare_python {
            let reference = crate::llm::reference::read_llm_reference_prompt_report(
                python_model_source,
                Some(&python_tokenizer_path),
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

        if self.compare_python_layers
            || self.compare_incremental
            || self.compare_incremental_hidden
            || self.compare_incremental_layers
        {
            let rendered = crate::llm::runtime::render_qwen_single_turn_prompt(
                self.system_prompt.as_deref(),
                &self.prompt,
            );
            let prompt_token_ids = crate::llm::runtime::tokenize_rendered_prompt(
                &artifacts.tokenizer_path,
                &rendered.prompt,
            )?;
            println!("Rendered prompt:\n{}", rendered.prompt);
            if self.compare_python_layers {
                let python_report = crate::llm::reference::read_llm_reference_layer_report(
                    python_model_source,
                    Some(&python_tokenizer_path),
                    &self.python_device,
                    self.system_prompt.as_deref(),
                    &self.prompt,
                    10,
                )?;
                let rust_report =
                    crate::llm::burn_text::collect_full_layer_outputs_burn_text_runtime(
                        &artifacts,
                        &prompt_token_ids,
                    )?;
                println!(
                    "Python reference tokenizer source: {}",
                    python_report.tokenizer_source
                );
                println!(
                    "Prompt token ids match Python reference: {}",
                    python_report.input_token_ids == prompt_token_ids
                );
                println!(
                    "Python reference top token ids: {:?}",
                    python_report.top_token_ids
                );
                println!(
                    "Python reference top token text: {:?}",
                    python_report.top_token_text
                );
                println!("Python reference top logits: {:?}", python_report.top_logits);
                println!("Rust Burn backend: {}", rust_report.backend);
                println!(
                    "Python layer count: {} / Rust layer count: {}",
                    python_report.layer_last_hidden_states.len(),
                    rust_report.layer_last_hidden_states.len()
                );
                let (final_norm_max_abs_diff, final_norm_mean_abs_diff) =
                    hidden_diff_summary(
                        &python_report.final_norm_last_hidden,
                        &rust_report.final_norm_last_hidden,
                    )?;
                println!(
                    "Rust vs Python final norm hidden diff: max_abs={} mean_abs={}",
                    final_norm_max_abs_diff,
                    final_norm_mean_abs_diff
                );
                for (layer_index, (python_hidden, rust_hidden)) in python_report
                    .layer_last_hidden_states
                    .iter()
                    .zip(rust_report.layer_last_hidden_states.iter())
                    .enumerate()
                {
                    let (max_abs_diff, mean_abs_diff) =
                        hidden_diff_summary(python_hidden, rust_hidden)?;
                    println!(
                        "Rust vs Python layer {} hidden diff: max_abs={} mean_abs={}",
                        layer_index,
                        max_abs_diff,
                        mean_abs_diff
                    );
                }
            }
            if self.compare_incremental {
                let comparison = crate::llm::burn_text::compare_with_incremental_burn_text_runtime(
                    &artifacts,
                    &prompt_token_ids,
                    &crate::llm::burn_text::BurnTextGenerationOptions {
                        max_new_tokens: self.max_new_tokens,
                        generation_timeout,
                    },
                )?;
                println!(
                    "Burn incremental comparison backend: {}",
                    comparison.backend
                );
                println!(
                    "Burn incremental token match: {}",
                    comparison.token_match
                );
                println!(
                    "Burn incremental first mismatch index: {:?}",
                    comparison.first_mismatch_index
                );
                println!(
                    "Burn incremental stable token ids: {:?}",
                    comparison.full_generated_token_ids
                );
                println!(
                    "Burn incremental experimental token ids: {:?}",
                    comparison.incremental_generated_token_ids
                );
                println!(
                    "Burn incremental stable text: {:?}",
                    comparison.full_generated_text
                );
                println!(
                    "Burn incremental experimental text: {:?}",
                    comparison.incremental_generated_text
                );
            }
            if self.compare_incremental_hidden {
                let diagnostics =
                    crate::llm::burn_text::diagnose_incremental_hidden_burn_text_runtime(
                        &artifacts,
                        &prompt_token_ids,
                    )?;
                println!(
                    "Burn incremental hidden diagnostics backend: {}",
                    diagnostics.backend
                );
                println!(
                    "Burn incremental first-prompt-token hidden diff: max_abs={} mean_abs={}",
                    diagnostics.first_prompt_token_hidden_diff.max_abs_diff,
                    diagnostics.first_prompt_token_hidden_diff.mean_abs_diff
                );
                println!(
                    "Burn incremental full-prompt hidden diff: max_abs={} mean_abs={}",
                    diagnostics.full_prompt_hidden_diff.max_abs_diff,
                    diagnostics.full_prompt_hidden_diff.mean_abs_diff
                );
            }
            if self.compare_incremental_layers {
                let diagnostics =
                    crate::llm::burn_text::diagnose_incremental_layers_burn_text_runtime(
                        &artifacts,
                        &prompt_token_ids,
                    )?;
                println!(
                    "Burn incremental layer diagnostics backend: {}",
                    diagnostics.backend
                );
                println!(
                    "Burn incremental first large diff layer index: {:?}",
                    diagnostics.first_large_diff_layer_index
                );
                for layer in diagnostics.layer_differences {
                    println!(
                        "Burn incremental layer {} ({}) hidden diff: max_abs={} mean_abs={}",
                        layer.layer_index,
                        layer.layer_type,
                        layer.hidden_diff.max_abs_diff,
                        layer.hidden_diff.mean_abs_diff
                    );
                }
            }
            return Ok(CliOutput::none());
        }

        let result = crate::llm::runtime::run_prompt(
            &artifacts,
            &crate::llm::runtime::LlmPromptRequest {
                system_prompt: self.system_prompt,
                user_prompt: self.prompt,
                max_new_tokens: self.max_new_tokens,
                generation_timeout,
            },
        )?;
        println!("Rendered prompt:\n{}", result.rendered_prompt);
        println!("\nRust Burn output:\n{}", result.output_text);
        Ok(CliOutput::none())
    }
}

fn parse_generation_timeout(value: &str) -> eyre::Result<Duration> {
    humantime::parse_duration(value).map_err(|error| {
        eyre::eyre!(
            "Failed to parse timeout {:?}: {}",
            value,
            error
        )
    })
}

fn resolve_timeout_arg(
    timeout: Option<&str>,
    generation_timeout: Option<&str>,
) -> eyre::Result<Option<Duration>> {
    match (timeout, generation_timeout) {
        (Some(_), Some(_)) => eyre::bail!(
            "Specify only one of `--timeout` or `--generation-timeout`; `--timeout` is the preferred flag."
        ),
        (Some(timeout), None) | (None, Some(timeout)) => {
            parse_generation_timeout(timeout).map(Some)
        }
        (None, None) => Ok(None),
    }
}

fn maybe_warn_about_dev_profile() {
    if !cfg!(debug_assertions) {
        return;
    }
    tracing::warn!(
        "Running `llm prompt` in the dev profile is substantially slower than release for Burn LLM inference. Prefer `cargo run --release -- llm prompt ...` for meaningful timing or throughput checks."
    );
    eprintln!(
        "warning: `llm prompt` is running in the dev profile; Burn LLM inference is much slower here. Prefer `cargo run --release -- llm prompt ...` for real performance measurements."
    );
}

fn is_timeout_supervision_child() -> bool {
    std::env::var_os(LLM_TIMEOUT_CHILD_ENV).is_some()
}

fn supervise_llm_prompt_timeout(timeout: Duration) -> eyre::Result<CliOutput> {
    let exe = std::env::current_exe()
        .map_err(|error| eyre::eyre!("Failed to resolve current executable for LLM timeout supervision: {}", error))?;
    let current_dir = std::env::current_dir()
        .map_err(|error| eyre::eyre!("Failed to resolve current directory for LLM timeout supervision: {}", error))?;
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let mut child = Command::new(&exe)
        .args(&args)
        .current_dir(current_dir)
        .env(LLM_TIMEOUT_CHILD_ENV, "1")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| eyre::eyre!("Failed to launch supervised LLM child process {}: {}", exe.display(), error))?;

    let deadline = std::time::Instant::now() + timeout + LLM_TIMEOUT_KILL_GRACE;
    loop {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(CliOutput::none());
            }
            return Err(eyre::eyre!(
                "supervised LLM child process exited with status {}",
                status
            ));
        }
        if std::time::Instant::now() >= deadline {
            tracing::warn!(
                pid = child.id(),
                timeout_secs = timeout.as_secs_f64(),
                grace_secs = LLM_TIMEOUT_KILL_GRACE.as_secs_f64(),
                "LLM child process exceeded graceful timeout window; forcing termination"
            );
            force_terminate_child(&mut child)?;
            return Err(eyre::eyre!(
                "LLM prompt exceeded timeout {} plus {} grace; child process was terminated",
                humantime::format_duration(timeout),
                humantime::format_duration(LLM_TIMEOUT_KILL_GRACE),
            ));
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn force_terminate_child(child: &mut std::process::Child) -> eyre::Result<()> {
    if let Err(error) = child.kill() {
        tracing::warn!(pid = child.id(), %error, "Child kill() failed during timeout termination");
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .arg("/PID")
            .arg(child.id().to_string())
            .arg("/T")
            .arg("/F")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.wait();
    Ok(())
}

fn hidden_diff_summary(left: &[f32], right: &[f32]) -> eyre::Result<(f32, f32)> {
    eyre::ensure!(
        left.len() == right.len(),
        "Hidden comparison length mismatch: {} vs {}",
        left.len(),
        right.len()
    );
    if left.is_empty() {
        return Ok((0.0, 0.0));
    }
    let mut max_abs_diff = 0.0_f32;
    let mut abs_diff_sum = 0.0_f64;
    for (left_value, right_value) in left.iter().zip(right.iter()) {
        let abs_diff = (left_value - right_value).abs();
        max_abs_diff = max_abs_diff.max(abs_diff);
        abs_diff_sum += f64::from(abs_diff);
    }
    Ok((max_abs_diff, (abs_diff_sum / left.len() as f64) as f32))
}
