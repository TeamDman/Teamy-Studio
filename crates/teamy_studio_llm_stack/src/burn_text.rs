use burn::{
    backend::{Cuda, NdArray, cuda::CudaDevice, ndarray::NdArrayDevice},
    tensor::{Tensor, TensorData, activation::{self, softmax}, backend::Backend},
};
use eyre::{WrapErr, bail, ensure};
use facet::Facet;
use half::{bf16, f16, slice::{HalfBitsSliceExt, HalfFloatSliceExt}};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    fmt,
    fs::File,
    io::{Read, Seek, SeekFrom},
    marker::PhantomData,
    path::{Path, PathBuf},
    sync::{Mutex, atomic::{AtomicBool, Ordering}},
    time::{Duration, Instant},
};

use crate::{
    model::{LlmModelArtifacts, load_tokenizer_config_summary},
    reference::{LlmReferenceBurnTextExportReport, export_llm_reference_burn_text_model},
};

pub const BURN_TEXT_DIR_NAME: &str = "burn-text";
pub const BURN_TEXT_MANIFEST_FILE_NAME: &str = "burn-text-manifest.json";
pub const DEFAULT_BURN_TEXT_EXPORT_DTYPE: &str = "float16";
const DEFAULT_LOGIT_CHUNK_ROWS: usize = 512;

pub type LlmCpuBackend = NdArray<f32>;
pub type LlmCudaBackend = Cuda<f16, i32>;

macro_rules! llm_tracy_zone {
    ($name:literal) => {
        #[cfg(feature = "tracing_subscriber_tracy")]
        let _tracy_zone = tracy_client::span!($name, 0);
    };
}

#[derive(Clone, Debug, PartialEq, Eq, Facet)]
pub struct BurnTextTensorSpec {
    pub path: String,
    pub shape: Vec<usize>,
    pub dtype: String,
    #[facet(default)]
    pub offset_bytes: Option<u64>,
    #[facet(default)]
    pub byte_len: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Facet)]
pub struct BurnTextManifest {
    pub format_version: u32,
    pub architecture: String,
    pub source_model_id: String,
    pub text_model_type: String,
    pub vocab_size: usize,
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub num_hidden_layers: usize,
    pub num_attention_heads: usize,
    pub num_key_value_heads: usize,
    pub head_dim: usize,
    pub rms_norm_eps: f64,
    pub hidden_act: String,
    pub partial_rotary_factor: f64,
    pub rope_theta: f64,
    pub linear_num_key_heads: usize,
    pub linear_num_value_heads: usize,
    pub linear_key_head_dim: usize,
    pub linear_value_head_dim: usize,
    pub linear_conv_kernel_dim: usize,
    pub layer_types: Vec<String>,
    pub tensors: BTreeMap<String, BurnTextTensorSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq, Facet)]
pub struct BurnTextRuntimeStatus {
    pub directory: String,
    pub manifest_path: String,
    pub exists: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Facet)]
pub struct BurnTextGenerationReport {
    pub generated_token_ids: Vec<usize>,
    pub generated_text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BurnTextGenerationOptions {
    pub max_new_tokens: usize,
    pub generation_timeout: Option<Duration>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BurnTextGenerationMode {
    Full,
    Incremental,
    Hybrid,
}

#[derive(Clone, Debug, PartialEq, Facet)]
pub struct BurnTextHiddenDifferenceSummary {
    pub max_abs_diff: f32,
    pub mean_abs_diff: f32,
}

#[derive(Clone, Debug, PartialEq, Facet)]
pub struct BurnTextIncrementalComparisonReport {
    pub backend: String,
    pub full_generated_token_ids: Vec<usize>,
    pub full_generated_text: String,
    pub incremental_generated_token_ids: Vec<usize>,
    pub incremental_generated_text: String,
    pub first_mismatch_index: Option<usize>,
    pub token_match: bool,
}

#[derive(Clone, Debug, PartialEq, Facet)]
pub struct BurnTextIncrementalHiddenDiagnosticsReport {
    pub backend: String,
    pub first_prompt_token_hidden_diff: BurnTextHiddenDifferenceSummary,
    pub full_prompt_hidden_diff: BurnTextHiddenDifferenceSummary,
}

#[derive(Clone, Debug, PartialEq, Facet)]
pub struct BurnTextLayerHiddenDifference {
    pub layer_index: usize,
    pub layer_type: String,
    pub hidden_diff: BurnTextHiddenDifferenceSummary,
}

#[derive(Clone, Debug, PartialEq, Facet)]
pub struct BurnTextIncrementalLayerDiagnosticsReport {
    pub backend: String,
    pub first_large_diff_layer_index: Option<usize>,
    pub layer_differences: Vec<BurnTextLayerHiddenDifference>,
}

#[derive(Clone, Debug, PartialEq, Facet)]
pub struct BurnTextLayerOutputsReport {
    pub backend: String,
    pub layer_last_hidden_states: Vec<Vec<f32>>,
    pub final_norm_last_hidden: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq)]
struct LoadedTensor {
    shape: Vec<usize>,
    values: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TokenMixerKind {
    FullAttention,
    LinearAttention,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HiddenActivationKind {
    Silu,
}

#[derive(Clone, Debug)]
struct GenerationDeadline {
    started_at: Instant,
    timeout: Option<Duration>,
}

#[derive(Debug)]
struct GenerationTimeoutError {
    elapsed: Duration,
    generated_token_count: usize,
    max_new_tokens: usize,
}

#[derive(Debug)]
struct Qwen35TextRuntime<B: Backend> {
    bundle_root: PathBuf,
    manifest: BurnTextManifest,
    device: B::Device,
    backend_marker: PhantomData<B>,
    packed_tensor_path: Option<PathBuf>,
    packed_tensor_file: Option<Mutex<File>>,
    loaded_tensor_cache: Mutex<HashMap<String, LoadedTensor>>,
    #[allow(dead_code)]
    tensor_1d_cache: Mutex<HashMap<String, Tensor<B, 1>>>,
    tensor_1d_values_cache: Mutex<HashMap<String, Vec<f32>>>,
    tensor_2d_cache: Mutex<HashMap<String, Tensor<B, 2>>>,
    lm_head_tensor_disabled: AtomicBool,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
struct Qwen35DecodeState {
    processed_token_count: usize,
    layer_states: Vec<DecoderLayerDecodeState>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
enum DecoderLayerDecodeState {
    Full(FullAttentionDecodeState),
    Linear(LinearAttentionDecodeState),
}

#[allow(dead_code)]
#[derive(Clone, Debug, Default, PartialEq)]
struct FullAttentionDecodeState {
    token_count: usize,
    repeated_key_cache: Vec<f32>,
    repeated_value_cache: Vec<f32>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
struct LinearAttentionDecodeState {
    state: Vec<f32>,
    conv_history: VecDeque<Vec<f32>>,
}

impl<B: Backend> Qwen35TextRuntime<B> {
    fn load(artifacts: &LlmModelArtifacts, device: B::Device) -> eyre::Result<Self> {
        let bundle_root = burn_text_dir(&artifacts.root);
        let manifest_path = burn_text_manifest_path(&artifacts.root);
        let manifest = load_burn_text_manifest(&manifest_path)?;
        ensure!(
            manifest.architecture == manifest.text_model_type,
            "Burn text manifest {} declared architecture `{}` but text model type `{}`",
            manifest_path.display(),
            manifest.architecture,
            manifest.text_model_type
        );
        ensure!(
            manifest.layer_types.len() == manifest.num_hidden_layers,
            "Burn text manifest {} declared {} layers but listed {} layer types",
            manifest_path.display(),
            manifest.num_hidden_layers,
            manifest.layer_types.len()
        );
        validate_manifest_contract(&manifest, &manifest_path)?;
        let packed_tensor_path = shared_packed_tensor_path(&bundle_root, &manifest);
        let packed_tensor_file = packed_tensor_path
            .as_ref()
            .map(File::open)
            .transpose()
            .wrap_err_with(|| {
                format!(
                    "Failed to open packed Burn text tensor file for {}",
                    manifest_path.display()
                )
            })?
            .map(Mutex::new);
        Ok(Self {
            bundle_root,
            manifest,
            device,
            backend_marker: PhantomData,
            packed_tensor_path,
            packed_tensor_file,
            loaded_tensor_cache: Mutex::new(HashMap::new()),
            tensor_1d_cache: Mutex::new(HashMap::new()),
            tensor_1d_values_cache: Mutex::new(HashMap::new()),
            tensor_2d_cache: Mutex::new(HashMap::new()),
            lm_head_tensor_disabled: AtomicBool::new(false),
        })
    }

    fn hidden_activation_kind(&self) -> eyre::Result<HiddenActivationKind> {
        hidden_activation_kind(&self.manifest.hidden_act)
    }

    #[allow(dead_code)]
    fn embedding_hidden_states(&self, token_ids: &[u32]) -> eyre::Result<Tensor<B, 3>> {
        let embedding = self.read_rows_f32("model.embed_tokens.weight", token_ids)?;
        Ok(tensor_3d(
            [1, token_ids.len(), self.manifest.hidden_size],
            embedding,
            &self.device,
        ))
    }

    fn read_rows_f32(&self, tensor_name: &str, row_ids: &[u32]) -> eyre::Result<Vec<f32>> {
        llm_tracy_zone!("llm_burn_read_rows_f32");
        #[cfg(feature = "extended_observability")]
        let _span = tracing::debug_span!(
            "llm_burn_read_rows_f32",
            tensor_name,
            row_count = row_ids.len()
        )
        .entered();
        let spec = self.tensor_spec(tensor_name)?;
        ensure!(
            spec.shape.len() == 2,
            "Expected 2D tensor for row lookup on `{tensor_name}` but found {:?}",
            spec.shape
        );
        let rows = spec.shape[0];
        let cols = spec.shape[1];
        let bytes_per_element = bytes_per_element(&spec.dtype)?;
        let row_bytes = cols
            .checked_mul(bytes_per_element)
            .ok_or_else(|| eyre::eyre!("row size overflow for tensor `{tensor_name}`"))?;
        let path = self.tensor_file_path(spec);
        let (offset_bytes, byte_len) = self.tensor_byte_range(spec, tensor_name)?;
        ensure!(
            byte_len == rows * row_bytes,
            "Burn text tensor `{tensor_name}` byte range {} did not match {}x{} rows",
            byte_len,
            rows,
            row_bytes
        );
        let mut values = Vec::with_capacity(
            row_ids
                .len()
                .checked_mul(cols)
                .ok_or_else(|| eyre::eyre!("embedding row capacity overflow"))?,
        );
        let mut buffer = vec![0_u8; row_bytes];
        if let Some(file) = &self.packed_tensor_file
            && self.packed_tensor_path.as_ref().is_some_and(|packed| packed == &path)
        {
            let mut file = file.lock().map_err(|_| {
                eyre::eyre!(
                    "Packed Burn text tensor file mutex was poisoned for {}",
                    path.display()
                )
            })?;
            for row_id in row_ids {
                let row_index = usize::try_from(*row_id)
                    .wrap_err_with(|| format!("Token id {row_id} exceeded usize range"))?;
                ensure!(
                    row_index < rows,
                    "Tensor `{tensor_name}` only contains {rows} rows, but token id {row_index} was requested"
                );
                let row_offset = row_index
                    .checked_mul(row_bytes)
                    .ok_or_else(|| eyre::eyre!("row offset overflow for tensor `{tensor_name}`"))?;
                let byte_offset = offset_bytes
                    .checked_add(u64::try_from(row_offset).unwrap_or(u64::MAX))
                    .ok_or_else(|| eyre::eyre!("byte offset overflow for tensor `{tensor_name}`"))?;
                file.seek(SeekFrom::Start(byte_offset)).wrap_err_with(|| {
                    format!(
                        "Failed to seek to row {row_index} in packed Burn text tensor {}",
                        path.display()
                    )
                })?;
                file.read_exact(&mut buffer).wrap_err_with(|| {
                    format!(
                        "Failed to read row {row_index} from packed Burn text tensor {}",
                        path.display()
                    )
                })?;
                decode_bytes_into_f32(&buffer, &spec.dtype, &mut values)?;
            }
            return Ok(values);
        }

        let mut file = File::open(&path)
            .wrap_err_with(|| format!("Failed to open Burn text tensor {}", path.display()))?;
        for row_id in row_ids {
            let row_index = usize::try_from(*row_id)
                .wrap_err_with(|| format!("Token id {row_id} exceeded usize range"))?;
            ensure!(
                row_index < rows,
                "Tensor `{tensor_name}` only contains {rows} rows, but token id {row_index} was requested"
            );
            let row_offset = row_index
                .checked_mul(row_bytes)
                .ok_or_else(|| eyre::eyre!("row offset overflow for tensor `{tensor_name}`"))?;
            let byte_offset = offset_bytes
                .checked_add(u64::try_from(row_offset).unwrap_or(u64::MAX))
                .ok_or_else(|| eyre::eyre!("byte offset overflow for tensor `{tensor_name}`"))?;
            file.seek(SeekFrom::Start(byte_offset)).wrap_err_with(|| {
                format!(
                    "Failed to seek to row {row_index} in Burn text tensor {}",
                    path.display()
                )
            })?;
            file.read_exact(&mut buffer).wrap_err_with(|| {
                format!(
                    "Failed to read row {row_index} from Burn text tensor {}",
                    path.display()
                )
            })?;
            decode_bytes_into_f32(&buffer, &spec.dtype, &mut values)?;
        }
        Ok(values)
    }

    fn tensor_spec(&self, tensor_name: &str) -> eyre::Result<&BurnTextTensorSpec> {
        self.manifest
            .tensors
            .get(tensor_name)
            .ok_or_else(|| eyre::eyre!("Burn text manifest is missing tensor `{tensor_name}`"))
    }

    fn tensor_file_path(&self, spec: &BurnTextTensorSpec) -> PathBuf {
        self.bundle_root.join(&spec.path)
    }

    fn tensor_byte_range(
        &self,
        spec: &BurnTextTensorSpec,
        tensor_name: &str,
    ) -> eyre::Result<(u64, usize)> {
        let element_count = spec
            .shape
            .iter()
            .copied()
            .try_fold(1_usize, |acc, dim| acc.checked_mul(dim))
            .ok_or_else(|| eyre::eyre!("element-count overflow for tensor `{tensor_name}`"))?;
        let expected_bytes = element_count
            .checked_mul(bytes_per_element(&spec.dtype)?)
            .ok_or_else(|| eyre::eyre!("byte-count overflow for tensor `{tensor_name}`"))?;
        let offset_bytes = spec.offset_bytes.unwrap_or(0);
        let byte_len = spec.byte_len.unwrap_or(expected_bytes);
        ensure!(
            byte_len == expected_bytes,
            "Burn text tensor `{tensor_name}` expected {} bytes from shape {:?} and dtype {}, but manifest declared {}",
            expected_bytes,
            spec.shape,
            spec.dtype,
            byte_len
        );
        Ok((offset_bytes, byte_len))
    }

    fn read_tensor_bytes(
        &self,
        spec: &BurnTextTensorSpec,
        tensor_name: &str,
    ) -> eyre::Result<Vec<u8>> {
        let (offset_bytes, byte_len) = self.tensor_byte_range(spec, tensor_name)?;
        let path = self.tensor_file_path(spec);
        let mut bytes = vec![0_u8; byte_len];
        if let Some(file) = &self.packed_tensor_file
            && self.packed_tensor_path.as_ref().is_some_and(|packed| packed == &path)
        {
            let mut file = file.lock().map_err(|_| {
                eyre::eyre!(
                    "Packed Burn text tensor file mutex was poisoned for {}",
                    path.display()
                )
            })?;
            file.seek(SeekFrom::Start(offset_bytes)).wrap_err_with(|| {
                format!(
                    "Failed to seek to offset {} in packed Burn text tensor {}",
                    offset_bytes,
                    path.display()
                )
            })?;
            file.read_exact(&mut bytes).wrap_err_with(|| {
                format!(
                    "Failed to read {} bytes from packed Burn text tensor {}",
                    byte_len,
                    path.display()
                )
            })?;
            return Ok(bytes);
        }
        let mut file = File::open(&path)
            .wrap_err_with(|| format!("Failed to open Burn text tensor {}", path.display()))?;
        file.seek(SeekFrom::Start(offset_bytes)).wrap_err_with(|| {
            format!(
                "Failed to seek to offset {} in Burn text tensor {}",
                offset_bytes,
                path.display()
            )
        })?;
        file.read_exact(&mut bytes).wrap_err_with(|| {
            format!(
                "Failed to read {} bytes from Burn text tensor {}",
                byte_len,
                path.display()
            )
        })?;
        Ok(bytes)
    }

    fn load_tensor(&self, tensor_name: &str) -> eyre::Result<LoadedTensor> {
        llm_tracy_zone!("llm_burn_load_tensor");
        #[cfg(feature = "extended_observability")]
        let _span = tracing::debug_span!("llm_burn_load_tensor", tensor_name).entered();
        if let Some(cached) = self
            .loaded_tensor_cache
            .lock()
            .map_err(|_| eyre::eyre!("Loaded tensor cache mutex was poisoned"))?
            .get(tensor_name)
            .cloned()
        {
            return Ok(cached);
        }
        let spec = self.tensor_spec(tensor_name)?;
        let element_count = spec
            .shape
            .iter()
            .copied()
            .try_fold(1_usize, |acc, dim| acc.checked_mul(dim))
            .ok_or_else(|| eyre::eyre!("element-count overflow for tensor `{tensor_name}`"))?;
        let mut values = Vec::with_capacity(element_count);
        let bytes = self.read_tensor_bytes(spec, tensor_name)?;
        decode_bytes_into_f32(&bytes, &spec.dtype, &mut values)?;
        let loaded = LoadedTensor {
            shape: spec.shape.clone(),
            values,
        };
        self.loaded_tensor_cache
            .lock()
            .map_err(|_| eyre::eyre!("Loaded tensor cache mutex was poisoned"))?
            .insert(tensor_name.to_owned(), loaded.clone());
        Ok(loaded)
    }

    #[allow(dead_code)]
    fn load_tensor_1d(&self, tensor_name: &str) -> eyre::Result<Tensor<B, 1>> {
        if let Some(cached) = self
            .tensor_1d_cache
            .lock()
            .map_err(|_| eyre::eyre!("1D tensor cache mutex was poisoned"))?
            .get(tensor_name)
            .cloned()
        {
            return Ok(cached);
        }
        let spec = self.tensor_spec(tensor_name)?;
        let [dim] = shape_array::<1>(&spec.shape, tensor_name)?;
        let bytes = self.read_tensor_bytes(spec, tensor_name)?;
        let tensor = tensor_1d_from_bytes(&bytes, &spec.dtype, dim, &self.device)?;
        self.tensor_1d_cache
            .lock()
            .map_err(|_| eyre::eyre!("1D tensor cache mutex was poisoned"))?
            .insert(tensor_name.to_owned(), tensor.clone());
        Ok(tensor)
    }

    fn load_tensor_2d(&self, tensor_name: &str) -> eyre::Result<Tensor<B, 2>> {
        if let Some(cached) = self
            .tensor_2d_cache
            .lock()
            .map_err(|_| eyre::eyre!("2D tensor cache mutex was poisoned"))?
            .get(tensor_name)
            .cloned()
        {
            return Ok(cached);
        }
        let spec = self.tensor_spec(tensor_name)?;
        let [dim0, dim1] = shape_array::<2>(&spec.shape, tensor_name)?;
        let bytes = self.read_tensor_bytes(spec, tensor_name)?;
        let tensor = tensor_2d_from_bytes(&bytes, &spec.dtype, [dim0, dim1], &self.device)?;
        self.tensor_2d_cache
            .lock()
            .map_err(|_| eyre::eyre!("2D tensor cache mutex was poisoned"))?
            .insert(tensor_name.to_owned(), tensor.clone());
        Ok(tensor)
    }

    #[allow(dead_code)]
    fn load_tensor_1d_values(&self, tensor_name: &str) -> eyre::Result<Vec<f32>> {
        if let Some(cached) = self
            .tensor_1d_values_cache
            .lock()
            .map_err(|_| eyre::eyre!("1D tensor values cache mutex was poisoned"))?
            .get(tensor_name)
            .cloned()
        {
            return Ok(cached);
        }
        let loaded = self.load_tensor(tensor_name)?;
        let [dim] = shape_array::<1>(&loaded.shape, tensor_name)?;
        ensure!(
            loaded.values.len() == dim,
            "Tensor `{tensor_name}` expected {} values but found {}",
            dim,
            loaded.values.len()
        );
        self.tensor_1d_values_cache
            .lock()
            .map_err(|_| eyre::eyre!("1D tensor values cache mutex was poisoned"))?
            .insert(tensor_name.to_owned(), loaded.values.clone());
        Ok(loaded.values)
    }

    #[allow(dead_code)]
    fn new_decode_state(&self) -> eyre::Result<Qwen35DecodeState> {
        let mut layer_states = Vec::with_capacity(self.manifest.num_hidden_layers);
        for layer_index in 0..self.manifest.num_hidden_layers {
            layer_states.push(match self.layer_token_mixer_kind(layer_index)? {
                TokenMixerKind::FullAttention => {
                    DecoderLayerDecodeState::Full(FullAttentionDecodeState::default())
                }
                TokenMixerKind::LinearAttention => {
                    let state_len = self
                        .manifest
                        .linear_num_value_heads
                        .checked_mul(self.manifest.linear_key_head_dim)
                        .and_then(|value| value.checked_mul(self.manifest.linear_value_head_dim))
                        .ok_or_else(|| {
                            eyre::eyre!("linear-attention decode-state size overflow")
                        })?;
                    DecoderLayerDecodeState::Linear(LinearAttentionDecodeState {
                        state: vec![0.0; state_len],
                        conv_history: VecDeque::with_capacity(
                            self.manifest.linear_conv_kernel_dim.saturating_sub(1),
                        ),
                    })
                }
            });
        }
        Ok(Qwen35DecodeState {
            processed_token_count: 0,
            layer_states,
        })
    }

    fn layer_token_mixer_kind(&self, layer_index: usize) -> eyre::Result<TokenMixerKind> {
        let layer_type = self
            .manifest
            .layer_types
            .get(layer_index)
            .ok_or_else(|| eyre::eyre!("Missing layer type for decoder layer {layer_index}"))?;
        match layer_type.as_str() {
            "full_attention" => Ok(TokenMixerKind::FullAttention),
            "linear_attention" => Ok(TokenMixerKind::LinearAttention),
            other => bail!("Unsupported Qwen3.5 layer type `{other}`"),
        }
    }

    #[allow(dead_code)]
    fn forward_hidden_states(
        &self,
        mut hidden_states: Tensor<B, 3>,
    ) -> eyre::Result<Tensor<B, 3>> {
        llm_tracy_zone!("llm_burn_forward_hidden_states");
        #[cfg(feature = "extended_observability")]
        let _span = tracing::info_span!(
            "llm_burn_forward_hidden_states",
            layer_count = self.manifest.num_hidden_layers
        )
        .entered();
        let [batch_size, seq_len, hidden_size] = hidden_states.dims();
        ensure!(
            batch_size == 1,
            "Qwen3.5 Burn text runtime currently expects batch size 1, but received {batch_size}"
        );
        ensure!(
            hidden_size == self.manifest.hidden_size,
            "Qwen3.5 Burn text runtime expected hidden size {}, but received {}",
            self.manifest.hidden_size,
            hidden_size
        );

        let position_embeddings = rotary_embeddings(seq_len, &self.manifest)
            .wrap_err("Failed to build rotary embeddings")?;
        let causal_mask = causal_mask::<B>(seq_len, &self.device);

        for layer_index in 0..self.manifest.num_hidden_layers {
            trace_burn_text_runtime(&format!(
                "decoder layer {}/{} ({})",
                layer_index + 1,
                self.manifest.num_hidden_layers,
                self.manifest.layer_types[layer_index]
            ));
            hidden_states = self.forward_decoder_layer(
                layer_index,
                hidden_states,
                &position_embeddings,
                &causal_mask,
            )?;
        }

        let norm_weight = tensor_to_vec_f32(&self.load_tensor_1d("model.norm.weight")?)?;
        let hidden = tensor_to_vec_f32(&hidden_states)?;
        let normalized = qwen_rms_norm(
            &hidden,
            batch_size,
            seq_len,
            self.manifest.hidden_size,
            &norm_weight,
            self.manifest.rms_norm_eps as f32,
        )?;
        Ok(tensor_3d(
            [batch_size, seq_len, self.manifest.hidden_size],
            normalized,
            &self.device,
        ))
    }

    #[allow(dead_code)]
    fn forward_hidden_states_with_decode_state(
        &self,
        mut hidden_states: Tensor<B, 3>,
    ) -> eyre::Result<(Tensor<B, 3>, Qwen35DecodeState)> {
        let [batch_size, seq_len, hidden_size] = hidden_states.dims();
        ensure!(
            batch_size == 1,
            "Qwen3.5 Burn text runtime currently expects batch size 1, but received {batch_size}"
        );
        ensure!(
            hidden_size == self.manifest.hidden_size,
            "Qwen3.5 Burn text runtime expected hidden size {}, but received {}",
            self.manifest.hidden_size,
            hidden_size
        );

        let position_embeddings = rotary_embeddings(seq_len, &self.manifest)
            .wrap_err("Failed to build rotary embeddings")?;
        let causal_mask = causal_mask::<B>(seq_len, &self.device);
        let mut decode_state = self.new_decode_state()?;

        for layer_index in 0..self.manifest.num_hidden_layers {
            hidden_states = self.forward_decoder_layer_with_decode_state(
                layer_index,
                hidden_states,
                &position_embeddings,
                &causal_mask,
                &mut decode_state.layer_states[layer_index],
            )?;
        }

        let norm_weight = tensor_to_vec_f32(&self.load_tensor_1d("model.norm.weight")?)?;
        let hidden = tensor_to_vec_f32(&hidden_states)?;
        let normalized = qwen_rms_norm(
            &hidden,
            batch_size,
            seq_len,
            self.manifest.hidden_size,
            &norm_weight,
            self.manifest.rms_norm_eps as f32,
        )?;
        decode_state.processed_token_count = seq_len;
        Ok((
            tensor_3d(
                [batch_size, seq_len, self.manifest.hidden_size],
                normalized,
                &self.device,
            ),
            decode_state,
        ))
    }

    #[allow(dead_code)]
    fn forward_token_hidden(
        &self,
        decode_state: &mut Qwen35DecodeState,
        token_id: u32,
    ) -> eyre::Result<Vec<f32>> {
        let mut hidden = self.read_rows_f32("model.embed_tokens.weight", &[token_id])?;
        for layer_index in 0..self.manifest.num_hidden_layers {
            hidden = self.forward_decoder_layer_single(
                layer_index,
                &hidden,
                decode_state.processed_token_count,
                &mut decode_state.layer_states[layer_index],
            )?;
        }
        let norm_weight = self.load_tensor_1d_values("model.norm.weight")?;
        let hidden = qwen_rms_norm(
            &hidden,
            1,
            1,
            self.manifest.hidden_size,
            &norm_weight,
            self.manifest.rms_norm_eps as f32,
        )?;
        decode_state.processed_token_count += 1;
        Ok(hidden)
    }

    #[allow(dead_code)]
    fn forward_token_hidden_layer_outputs(
        &self,
        decode_state: &mut Qwen35DecodeState,
        token_id: u32,
    ) -> eyre::Result<Vec<Vec<f32>>> {
        let mut hidden = self.read_rows_f32("model.embed_tokens.weight", &[token_id])?;
        let mut layer_outputs = Vec::with_capacity(self.manifest.num_hidden_layers);
        for layer_index in 0..self.manifest.num_hidden_layers {
            hidden = self.forward_decoder_layer_single(
                layer_index,
                &hidden,
                decode_state.processed_token_count,
                &mut decode_state.layer_states[layer_index],
            )?;
            layer_outputs.push(hidden.clone());
        }
        decode_state.processed_token_count += 1;
        Ok(layer_outputs)
    }

    #[allow(dead_code)]
    fn forward_decoder_layer_single(
        &self,
        layer_index: usize,
        hidden: &[f32],
        position: usize,
        layer_state: &mut DecoderLayerDecodeState,
    ) -> eyre::Result<Vec<f32>> {
        let hidden_size = self.manifest.hidden_size;
        ensure!(
            hidden.len() == hidden_size,
            "Qwen3.5 decode step expected hidden size {}, but found {}",
            hidden_size,
            hidden.len()
        );
        let prefix = format!("model.layers.{layer_index}");
        let input_layernorm_weight =
            self.load_tensor_1d_values(&format!("{prefix}.input_layernorm.weight"))?;
        let normalized = qwen_rms_norm(
            hidden,
            1,
            1,
            hidden_size,
            &input_layernorm_weight,
            self.manifest.rms_norm_eps as f32,
        )?;
        let mixed_delta = match layer_state {
            DecoderLayerDecodeState::Full(full_state) => {
                self.forward_full_attention_single(&prefix, &normalized, position, full_state)?
            }
            DecoderLayerDecodeState::Linear(linear_state) => {
                self.forward_linear_attention_single(&prefix, &normalized, linear_state)?
            }
        };
        let mixed = add_vectors(hidden, &mixed_delta)?;
        let post_attention_layernorm_weight =
            self.load_tensor_1d_values(&format!("{prefix}.post_attention_layernorm.weight"))?;
        let post_norm = qwen_rms_norm(
            &mixed,
            1,
            1,
            hidden_size,
            &post_attention_layernorm_weight,
            self.manifest.rms_norm_eps as f32,
        )?;
        let mlp = self.forward_mlp_single(&format!("{prefix}.mlp"), &post_norm)?;
        add_vectors(&mixed, &mlp)
    }

    #[allow(dead_code)]
    fn forward_mlp_single(&self, prefix: &str, hidden: &[f32]) -> eyre::Result<Vec<f32>> {
        let _activation = self.hidden_activation_kind()?;
        let hidden_states = tensor_3d([1, 1, self.manifest.hidden_size], hidden.to_vec(), &self.device);
        let gate_proj = linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.gate_proj.weight"))?,
            None,
        );
        let up_proj = linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.up_proj.weight"))?,
            None,
        );
        tensor_to_vec_f32(&linear_forward_3d(
            &activation::silu(gate_proj).mul(up_proj),
            self.load_tensor_2d(&format!("{prefix}.down_proj.weight"))?,
            None,
        ))
    }

    #[allow(dead_code)]
    fn forward_full_attention_single(
        &self,
        prefix: &str,
        hidden: &[f32],
        position: usize,
        decode_state: &mut FullAttentionDecodeState,
    ) -> eyre::Result<Vec<f32>> {
        let hidden_states = tensor_3d([1, 1, self.manifest.hidden_size], hidden.to_vec(), &self.device);
        let head_dim = self.manifest.head_dim;
        let query_projection = linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.self_attn.q_proj.weight"))?,
            None,
        );
        let query_projection_values = tensor_to_vec_f32(&query_projection)?;
        let gate_offset = self
            .manifest
            .num_attention_heads
            .checked_mul(head_dim)
            .ok_or_else(|| eyre::eyre!("gate offset overflow"))?;
        let query_and_gate_dim = gate_offset
            .checked_mul(2)
            .ok_or_else(|| eyre::eyre!("q_proj output size overflow"))?;
        ensure!(
            query_projection_values.len() == query_and_gate_dim,
            "Unexpected q_proj output size for Qwen3.5 full attention single-step decode"
        );
        let mut query_values = Vec::with_capacity(gate_offset);
        let mut gate_values = Vec::with_capacity(gate_offset);
        for head_chunk in query_projection_values.chunks_exact(head_dim * 2) {
            query_values.extend_from_slice(&head_chunk[..head_dim]);
            gate_values.extend_from_slice(&head_chunk[head_dim..]);
        }
        let gate_values = gate_values
            .into_iter()
            .map(sigmoid_scalar)
            .collect::<Vec<_>>();

        let query_norm_weight =
            self.load_tensor_1d_values(&format!("{prefix}.self_attn.q_norm.weight"))?;
        let key_norm_weight =
            self.load_tensor_1d_values(&format!("{prefix}.self_attn.k_norm.weight"))?;
        query_values = qwen_rms_norm_no_center(
            &query_values,
            1,
            self.manifest.num_attention_heads,
            head_dim,
            &query_norm_weight,
            self.manifest.rms_norm_eps as f32,
        )?;

        let key_proj = linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.self_attn.k_proj.weight"))?,
            None,
        );
        let mut key_values = qwen_rms_norm_no_center(
            &tensor_to_vec_f32(&key_proj)?,
            1,
            self.manifest.num_key_value_heads,
            head_dim,
            &key_norm_weight,
            self.manifest.rms_norm_eps as f32,
        )?;
        let value_proj = linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.self_attn.v_proj.weight"))?,
            None,
        );
        let value_values = tensor_to_vec_f32(&value_proj)?;

        apply_rotary_embedding_single_position(&mut query_values, position, &self.manifest, self.manifest.num_attention_heads)?;
        apply_rotary_embedding_single_position(&mut key_values, position, &self.manifest, self.manifest.num_key_value_heads)?;

        let repeated_key = repeat_key_value_heads(
            &key_values,
            1,
            self.manifest.num_key_value_heads,
            self.manifest.num_attention_heads / self.manifest.num_key_value_heads,
            head_dim,
        );
        let repeated_value = repeat_key_value_heads(
            &value_values,
            1,
            self.manifest.num_key_value_heads,
            self.manifest.num_attention_heads / self.manifest.num_key_value_heads,
            head_dim,
        );
        decode_state.repeated_key_cache.extend_from_slice(&repeated_key);
        decode_state.repeated_value_cache.extend_from_slice(&repeated_value);
        decode_state.token_count += 1;

        let scale = (head_dim as f32).powf(-0.5);
        let mut attention_output = vec![0.0_f32; self.manifest.hidden_size];
        for head in 0..self.manifest.num_attention_heads {
            let query_base = head * head_dim;
            let query_slice = &query_values[query_base..query_base + head_dim];
            let mut scores = Vec::with_capacity(decode_state.token_count);
            for token_index in 0..decode_state.token_count {
                let key_base = (token_index * self.manifest.num_attention_heads + head) * head_dim;
                let key_slice =
                    &decode_state.repeated_key_cache[key_base..key_base + head_dim];
                scores.push(dot_product(query_slice, key_slice) * scale);
            }
            let weights = softmax_scalars(&scores);
            let output_base = head * head_dim;
            for (token_index, weight) in weights.into_iter().enumerate() {
                let value_base =
                    (token_index * self.manifest.num_attention_heads + head) * head_dim;
                let value_slice =
                    &decode_state.repeated_value_cache[value_base..value_base + head_dim];
                for dim in 0..head_dim {
                    attention_output[output_base + dim] += value_slice[dim] * weight;
                }
            }
        }
        let gated = attention_output
            .into_iter()
            .zip(gate_values)
            .map(|(output, gate)| output * gate)
            .collect::<Vec<_>>();
        tensor_to_vec_f32(&linear_forward_3d(
            &tensor_3d([1, 1, self.manifest.hidden_size], gated, &self.device),
            self.load_tensor_2d(&format!("{prefix}.self_attn.o_proj.weight"))?,
            None,
        ))
    }

    #[allow(dead_code)]
    fn forward_linear_attention_single(
        &self,
        prefix: &str,
        hidden: &[f32],
        decode_state: &mut LinearAttentionDecodeState,
    ) -> eyre::Result<Vec<f32>> {
        let hidden_states = tensor_3d([1, 1, self.manifest.hidden_size], hidden.to_vec(), &self.device);
        let mixed_qkv = linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.linear_attn.in_proj_qkv.weight"))?,
            None,
        );
        let mixed_qkv_values = tensor_to_vec_f32(&mixed_qkv)?;
        let conv_weight = self.load_tensor(&format!("{prefix}.linear_attn.conv1d.weight"))?;
        let conv_mixed_qkv = depthwise_causal_conv1d_silu_step(
            &mixed_qkv_values,
            &decode_state.conv_history,
            self.manifest.linear_num_key_heads * self.manifest.linear_key_head_dim * 2
                + self.manifest.linear_num_value_heads * self.manifest.linear_value_head_dim,
            self.manifest.linear_conv_kernel_dim,
            &conv_weight.values,
            &conv_weight.shape,
        )?;
        decode_state.conv_history.push_back(mixed_qkv_values);
        while decode_state.conv_history.len() >= self.manifest.linear_conv_kernel_dim {
            decode_state.conv_history.pop_front();
        }

        let key_dim = self
            .manifest
            .linear_num_key_heads
            .checked_mul(self.manifest.linear_key_head_dim)
            .ok_or_else(|| eyre::eyre!("linear-attention key-dim overflow"))?;
        let value_dim = self
            .manifest
            .linear_num_value_heads
            .checked_mul(self.manifest.linear_value_head_dim)
            .ok_or_else(|| eyre::eyre!("linear-attention value-dim overflow"))?;
        let conv_dim = key_dim
            .checked_mul(2)
            .and_then(|value| value.checked_add(value_dim))
            .ok_or_else(|| eyre::eyre!("linear-attention conv-dim overflow"))?;
        ensure!(
            conv_mixed_qkv.len() == conv_dim,
            "Unexpected linear-attention conv output size for single-step decode"
        );
        let mut query = conv_mixed_qkv[..key_dim].to_vec();
        let mut key = conv_mixed_qkv[key_dim..key_dim * 2].to_vec();
        let value = conv_mixed_qkv[key_dim * 2..].to_vec();

        let z = tensor_to_vec_f32(&linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.linear_attn.in_proj_z.weight"))?,
            None,
        ))?;
        let beta = tensor_to_vec_f32(&linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.linear_attn.in_proj_b.weight"))?,
            None,
        ))?
        .into_iter()
        .map(sigmoid_scalar)
        .collect::<Vec<_>>();
        let a = tensor_to_vec_f32(&linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.linear_attn.in_proj_a.weight"))?,
            None,
        ))?;
        let dt_bias = self.load_tensor_1d_values(&format!("{prefix}.linear_attn.dt_bias"))?;
        let a_log = self.load_tensor_1d_values(&format!("{prefix}.linear_attn.A_log"))?;
        let mut g = Vec::with_capacity(a.len());
        for (index, value) in a.iter().copied().enumerate() {
            let head_index = index % self.manifest.linear_num_value_heads;
            let a_scale = -a_log[head_index].exp();
            g.push(a_scale * softplus_scalar(value + dt_bias[head_index]));
        }

        let repeat_factor = self.manifest.linear_num_value_heads / self.manifest.linear_num_key_heads;
        query = repeat_linear_attention_heads(
            &query,
            1,
            self.manifest.linear_num_key_heads,
            repeat_factor,
            self.manifest.linear_key_head_dim,
        );
        key = repeat_linear_attention_heads(
            &key,
            1,
            self.manifest.linear_num_key_heads,
            repeat_factor,
            self.manifest.linear_key_head_dim,
        );
        let query = l2_norm_heads(
            &query,
            1,
            self.manifest.linear_num_value_heads,
            self.manifest.linear_key_head_dim,
        )?;
        let key = l2_norm_heads(
            &key,
            1,
            self.manifest.linear_num_value_heads,
            self.manifest.linear_key_head_dim,
        )?;
        let core_attn_out = recurrent_gated_delta_step(
            &query,
            &key,
            &value,
            &g,
            &beta,
            &mut decode_state.state,
            self.manifest.linear_num_value_heads,
            self.manifest.linear_key_head_dim,
            self.manifest.linear_value_head_dim,
        )?;
        let norm_weight =
            self.load_tensor_1d_values(&format!("{prefix}.linear_attn.norm.weight"))?;
        let normalized = qwen_rms_norm_gated(
            &core_attn_out,
            1,
            self.manifest.linear_num_value_heads,
            self.manifest.linear_value_head_dim,
            &norm_weight,
            self.manifest.rms_norm_eps as f32,
            &z,
        )?;
        tensor_to_vec_f32(&linear_forward_3d(
            &tensor_3d([1, 1, value_dim], normalized, &self.device),
            self.load_tensor_2d(&format!("{prefix}.linear_attn.out_proj.weight"))?,
            None,
        ))
    }

    #[allow(dead_code)]
    fn forward_decoder_layer(
        &self,
        layer_index: usize,
        hidden_states: Tensor<B, 3>,
        position_embeddings: &RotaryEmbeddings,
        causal_mask: &Tensor<B, 4>,
    ) -> eyre::Result<Tensor<B, 3>> {
        llm_tracy_zone!("llm_burn_decoder_layer");
        #[cfg(feature = "extended_observability")]
        let _span = tracing::debug_span!(
            "llm_burn_decoder_layer",
            layer_index,
            layer_type = self.manifest.layer_types[layer_index].as_str()
        )
        .entered();
        let [batch_size, seq_len, hidden_size] = hidden_states.dims();
        let prefix = format!("model.layers.{layer_index}");

        let input_layernorm_weight = tensor_to_vec_f32(
            &self.load_tensor_1d(&format!("{prefix}.input_layernorm.weight"))?,
        )?;
        let input_values = tensor_to_vec_f32(&hidden_states)?;
        let normalized_values = qwen_rms_norm(
            &input_values,
            batch_size,
            seq_len,
            hidden_size,
            &input_layernorm_weight,
            self.manifest.rms_norm_eps as f32,
        )?;
        let normalized = tensor_3d(
            [batch_size, seq_len, hidden_size],
            normalized_values,
            &self.device,
        );

        let mixed = match self.layer_token_mixer_kind(layer_index)? {
            TokenMixerKind::FullAttention => self.forward_full_attention(
                &prefix,
                normalized,
                position_embeddings,
                causal_mask.clone(),
            )?,
            TokenMixerKind::LinearAttention => self.forward_linear_attention(&prefix, normalized)?,
        };

        let mixed = hidden_states + mixed;
        let post_attention_layernorm_weight = tensor_to_vec_f32(
            &self.load_tensor_1d(&format!("{prefix}.post_attention_layernorm.weight"))?,
        )?;
        let mixed_values = tensor_to_vec_f32(&mixed)?;
        let post_norm_values = qwen_rms_norm(
            &mixed_values,
            batch_size,
            seq_len,
            hidden_size,
            &post_attention_layernorm_weight,
            self.manifest.rms_norm_eps as f32,
        )?;
        let post_norm = tensor_3d(
            [batch_size, seq_len, hidden_size],
            post_norm_values,
            &self.device,
        );
        let mlp = self.forward_mlp(&format!("{prefix}.mlp"), post_norm)?;
        Ok(mixed + mlp)
    }

    #[allow(dead_code)]
    fn forward_decoder_layer_with_decode_state(
        &self,
        layer_index: usize,
        hidden_states: Tensor<B, 3>,
        position_embeddings: &RotaryEmbeddings,
        causal_mask: &Tensor<B, 4>,
        layer_state: &mut DecoderLayerDecodeState,
    ) -> eyre::Result<Tensor<B, 3>> {
        let [batch_size, seq_len, hidden_size] = hidden_states.dims();
        let prefix = format!("model.layers.{layer_index}");

        let input_layernorm_weight = tensor_to_vec_f32(
            &self.load_tensor_1d(&format!("{prefix}.input_layernorm.weight"))?,
        )?;
        let input_values = tensor_to_vec_f32(&hidden_states)?;
        let normalized_values = qwen_rms_norm(
            &input_values,
            batch_size,
            seq_len,
            hidden_size,
            &input_layernorm_weight,
            self.manifest.rms_norm_eps as f32,
        )?;
        let normalized = tensor_3d(
            [batch_size, seq_len, hidden_size],
            normalized_values,
            &self.device,
        );

        let mixed = match layer_state {
            DecoderLayerDecodeState::Full(full_state) => self.forward_full_attention_with_state(
                &prefix,
                normalized,
                position_embeddings,
                causal_mask.clone(),
                full_state,
            )?,
            DecoderLayerDecodeState::Linear(linear_state) => {
                self.forward_linear_attention_with_state(&prefix, normalized, linear_state)?
            }
        };

        let mixed = hidden_states + mixed;
        let post_attention_layernorm_weight = tensor_to_vec_f32(
            &self.load_tensor_1d(&format!("{prefix}.post_attention_layernorm.weight"))?,
        )?;
        let mixed_values = tensor_to_vec_f32(&mixed)?;
        let post_norm_values = qwen_rms_norm(
            &mixed_values,
            batch_size,
            seq_len,
            hidden_size,
            &post_attention_layernorm_weight,
            self.manifest.rms_norm_eps as f32,
        )?;
        let post_norm = tensor_3d(
            [batch_size, seq_len, hidden_size],
            post_norm_values,
            &self.device,
        );
        let mlp = self.forward_mlp(&format!("{prefix}.mlp"), post_norm)?;
        Ok(mixed + mlp)
    }

    #[allow(dead_code)]
    fn forward_mlp(
        &self,
        prefix: &str,
        hidden_states: Tensor<B, 3>,
    ) -> eyre::Result<Tensor<B, 3>> {
        llm_tracy_zone!("llm_burn_mlp");
        #[cfg(feature = "extended_observability")]
        let _span = tracing::debug_span!("llm_burn_mlp", prefix).entered();
        let _activation = self.hidden_activation_kind()?;
        let gate_proj = linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.gate_proj.weight"))?,
            None,
        );
        let up_proj = linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.up_proj.weight"))?,
            None,
        );
        Ok(linear_forward_3d(
            &activation::silu(gate_proj).mul(up_proj),
            self.load_tensor_2d(&format!("{prefix}.down_proj.weight"))?,
            None,
        ))
    }

    #[allow(dead_code)]
    fn forward_full_attention(
        &self,
        prefix: &str,
        hidden_states: Tensor<B, 3>,
        position_embeddings: &RotaryEmbeddings,
        causal_mask: Tensor<B, 4>,
    ) -> eyre::Result<Tensor<B, 3>> {
        llm_tracy_zone!("llm_burn_full_attention");
        #[cfg(feature = "extended_observability")]
        let _span = tracing::debug_span!("llm_burn_full_attention", prefix).entered();
        let [batch_size, seq_len, hidden_size] = hidden_states.dims();
        let head_dim = self.manifest.head_dim;
        let query_projection = linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.self_attn.q_proj.weight"))?,
            None,
        );
        let query_projection_values = tensor_to_vec_f32(&query_projection)?;
        let query_and_gate_dim = self
            .manifest
            .num_attention_heads
            .checked_mul(head_dim)
            .and_then(|value| value.checked_mul(2))
            .ok_or_else(|| eyre::eyre!("q_proj output size overflow"))?;
        ensure!(
            query_projection_values.len() == batch_size * seq_len * query_and_gate_dim,
            "Unexpected q_proj output size for Qwen3.5 full attention"
        );
        let query_width = self
            .manifest
            .num_attention_heads
            .checked_mul(head_dim)
            .ok_or_else(|| eyre::eyre!("query width overflow"))?;
        let mut query_values = Vec::with_capacity(batch_size * seq_len * query_width);
        let mut gate_values = Vec::with_capacity(batch_size * seq_len * query_width);
        for token_chunk in query_projection_values.chunks_exact(query_and_gate_dim) {
            for head_chunk in token_chunk.chunks_exact(head_dim * 2) {
                query_values.extend_from_slice(&head_chunk[..head_dim]);
                gate_values.extend_from_slice(&head_chunk[head_dim..]);
            }
        }
        let gate_values = gate_values
            .into_iter()
            .map(sigmoid_scalar)
            .collect::<Vec<_>>();

        let query_norm_weight =
            tensor_to_vec_f32(&self.load_tensor_1d(&format!("{prefix}.self_attn.q_norm.weight"))?)?;
        let key_norm_weight =
            tensor_to_vec_f32(&self.load_tensor_1d(&format!("{prefix}.self_attn.k_norm.weight"))?)?;
        let query_values = qwen_rms_norm_no_center(
            &query_values,
            batch_size * seq_len,
            self.manifest.num_attention_heads,
            head_dim,
            &query_norm_weight,
            self.manifest.rms_norm_eps as f32,
        )?;

        let key_proj = linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.self_attn.k_proj.weight"))?,
            None,
        );
        let key_values = qwen_rms_norm_no_center(
            &tensor_to_vec_f32(&key_proj)?,
            batch_size * seq_len,
            self.manifest.num_key_value_heads,
            head_dim,
            &key_norm_weight,
            self.manifest.rms_norm_eps as f32,
        )?;
        let value_proj = linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.self_attn.v_proj.weight"))?,
            None,
        );
        let value_values = tensor_to_vec_f32(&value_proj)?;

        let (query_values, key_values) = apply_rotary_embeddings(
            &query_values,
            &key_values,
            position_embeddings,
            &self.manifest,
            seq_len,
        )?;
        let query_values = transpose_seq_head_layout(
            &query_values,
            seq_len,
            self.manifest.num_attention_heads,
            head_dim,
        )?;

        let query = tensor_4d(
            [
                batch_size,
                self.manifest.num_attention_heads,
                seq_len,
                head_dim,
            ],
            query_values,
            &self.device,
        ) * (self.manifest.head_dim as f32).powf(-0.5);
        let key_repeated = repeat_key_value_heads(
            &key_values,
            seq_len,
            self.manifest.num_key_value_heads,
            self.manifest.num_attention_heads / self.manifest.num_key_value_heads,
            head_dim,
        );
        let key_repeated = transpose_seq_head_layout(
            &key_repeated,
            seq_len,
            self.manifest.num_attention_heads,
            head_dim,
        )?;
        let value_repeated = repeat_key_value_heads(
            &value_values,
            seq_len,
            self.manifest.num_key_value_heads,
            self.manifest.num_attention_heads / self.manifest.num_key_value_heads,
            head_dim,
        );
        let value_repeated = transpose_seq_head_layout(
            &value_repeated,
            seq_len,
            self.manifest.num_attention_heads,
            head_dim,
        )?;
        let key = tensor_4d(
            [
                batch_size,
                self.manifest.num_attention_heads,
                seq_len,
                head_dim,
            ],
            key_repeated,
            &self.device,
        );
        let value = tensor_4d(
            [
                batch_size,
                self.manifest.num_attention_heads,
                seq_len,
                head_dim,
            ],
            value_repeated,
            &self.device,
        );

        let attention = softmax(query.matmul(key.swap_dims(2, 3)) + causal_mask, 3);
        let output = attention
            .matmul(value)
            .swap_dims(1, 2)
            .reshape([batch_size, seq_len, hidden_size]);
        let gated = tensor_to_vec_f32(&output)?
            .into_iter()
            .zip(gate_values)
            .map(|(output, gate)| output * gate)
            .collect::<Vec<_>>();
        Ok(linear_forward_3d(
            &tensor_3d([batch_size, seq_len, hidden_size], gated, &self.device),
            self.load_tensor_2d(&format!("{prefix}.self_attn.o_proj.weight"))?,
            None,
        ))
    }

    #[allow(dead_code)]
    fn forward_full_attention_with_state(
        &self,
        prefix: &str,
        hidden_states: Tensor<B, 3>,
        position_embeddings: &RotaryEmbeddings,
        causal_mask: Tensor<B, 4>,
        decode_state: &mut FullAttentionDecodeState,
    ) -> eyre::Result<Tensor<B, 3>> {
        let [batch_size, seq_len, hidden_size] = hidden_states.dims();
        let head_dim = self.manifest.head_dim;
        let query_projection = linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.self_attn.q_proj.weight"))?,
            None,
        );
        let query_projection_values = tensor_to_vec_f32(&query_projection)?;
        let query_and_gate_dim = self
            .manifest
            .num_attention_heads
            .checked_mul(head_dim)
            .and_then(|value| value.checked_mul(2))
            .ok_or_else(|| eyre::eyre!("q_proj output size overflow"))?;
        ensure!(
            query_projection_values.len() == batch_size * seq_len * query_and_gate_dim,
            "Unexpected q_proj output size for Qwen3.5 full attention"
        );
        let query_width = self
            .manifest
            .num_attention_heads
            .checked_mul(head_dim)
            .ok_or_else(|| eyre::eyre!("query width overflow"))?;
        let mut query_values = Vec::with_capacity(batch_size * seq_len * query_width);
        let mut gate_values = Vec::with_capacity(batch_size * seq_len * query_width);
        for token_chunk in query_projection_values.chunks_exact(query_and_gate_dim) {
            for head_chunk in token_chunk.chunks_exact(head_dim * 2) {
                query_values.extend_from_slice(&head_chunk[..head_dim]);
                gate_values.extend_from_slice(&head_chunk[head_dim..]);
            }
        }
        let gate_values = gate_values
            .into_iter()
            .map(sigmoid_scalar)
            .collect::<Vec<_>>();

        let query_norm_weight =
            tensor_to_vec_f32(&self.load_tensor_1d(&format!("{prefix}.self_attn.q_norm.weight"))?)?;
        let key_norm_weight =
            tensor_to_vec_f32(&self.load_tensor_1d(&format!("{prefix}.self_attn.k_norm.weight"))?)?;
        let query_values = qwen_rms_norm_no_center(
            &query_values,
            batch_size * seq_len,
            self.manifest.num_attention_heads,
            head_dim,
            &query_norm_weight,
            self.manifest.rms_norm_eps as f32,
        )?;

        let key_proj = linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.self_attn.k_proj.weight"))?,
            None,
        );
        let key_values = qwen_rms_norm_no_center(
            &tensor_to_vec_f32(&key_proj)?,
            batch_size * seq_len,
            self.manifest.num_key_value_heads,
            head_dim,
            &key_norm_weight,
            self.manifest.rms_norm_eps as f32,
        )?;
        let value_proj = linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.self_attn.v_proj.weight"))?,
            None,
        );
        let value_values = tensor_to_vec_f32(&value_proj)?;

        let (query_values, key_values) = apply_rotary_embeddings(
            &query_values,
            &key_values,
            position_embeddings,
            &self.manifest,
            seq_len,
        )?;
        let query_values = transpose_seq_head_layout(
            &query_values,
            seq_len,
            self.manifest.num_attention_heads,
            head_dim,
        )?;

        let query = tensor_4d(
            [
                batch_size,
                self.manifest.num_attention_heads,
                seq_len,
                head_dim,
            ],
            query_values,
            &self.device,
        ) * (self.manifest.head_dim as f32).powf(-0.5);
        let key_repeated = repeat_key_value_heads(
            &key_values,
            seq_len,
            self.manifest.num_key_value_heads,
            self.manifest.num_attention_heads / self.manifest.num_key_value_heads,
            head_dim,
        );
        let value_repeated = repeat_key_value_heads(
            &value_values,
            seq_len,
            self.manifest.num_key_value_heads,
            self.manifest.num_attention_heads / self.manifest.num_key_value_heads,
            head_dim,
        );
        decode_state.token_count = seq_len;
        decode_state.repeated_key_cache = key_repeated.clone();
        decode_state.repeated_value_cache = value_repeated.clone();
        let key_repeated = transpose_seq_head_layout(
            &key_repeated,
            seq_len,
            self.manifest.num_attention_heads,
            head_dim,
        )?;
        let value_repeated = transpose_seq_head_layout(
            &value_repeated,
            seq_len,
            self.manifest.num_attention_heads,
            head_dim,
        )?;
        let key = tensor_4d(
            [
                batch_size,
                self.manifest.num_attention_heads,
                seq_len,
                head_dim,
            ],
            key_repeated,
            &self.device,
        );
        let value = tensor_4d(
            [
                batch_size,
                self.manifest.num_attention_heads,
                seq_len,
                head_dim,
            ],
            value_repeated,
            &self.device,
        );

        let attention = softmax(query.matmul(key.swap_dims(2, 3)) + causal_mask, 3);
        let output = attention
            .matmul(value)
            .swap_dims(1, 2)
            .reshape([batch_size, seq_len, hidden_size]);
        let gated = tensor_to_vec_f32(&output)?
            .into_iter()
            .zip(gate_values)
            .map(|(output, gate)| output * gate)
            .collect::<Vec<_>>();
        Ok(linear_forward_3d(
            &tensor_3d([batch_size, seq_len, hidden_size], gated, &self.device),
            self.load_tensor_2d(&format!("{prefix}.self_attn.o_proj.weight"))?,
            None,
        ))
    }

    #[allow(dead_code)]
    fn forward_linear_attention(
        &self,
        prefix: &str,
        hidden_states: Tensor<B, 3>,
    ) -> eyre::Result<Tensor<B, 3>> {
        llm_tracy_zone!("llm_burn_linear_attention");
        #[cfg(feature = "extended_observability")]
        let _span = tracing::debug_span!("llm_burn_linear_attention", prefix).entered();
        let [batch_size, seq_len, _hidden_size] = hidden_states.dims();
        ensure!(batch_size == 1, "Qwen3.5 linear attention currently expects batch size 1");
        let mixed_qkv = linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.linear_attn.in_proj_qkv.weight"))?,
            None,
        );
        let mixed_qkv_values = tensor_to_vec_f32(&mixed_qkv)?;
        let conv_weight = self.load_tensor(&format!("{prefix}.linear_attn.conv1d.weight"))?;
        let conv_mixed_qkv = depthwise_causal_conv1d_silu(
            &mixed_qkv_values,
            batch_size,
            seq_len,
            self.manifest.linear_num_key_heads * self.manifest.linear_key_head_dim * 2
                + self.manifest.linear_num_value_heads * self.manifest.linear_value_head_dim,
            self.manifest.linear_conv_kernel_dim,
            &conv_weight.values,
            &conv_weight.shape,
        )?;

        let key_dim = self
            .manifest
            .linear_num_key_heads
            .checked_mul(self.manifest.linear_key_head_dim)
            .ok_or_else(|| eyre::eyre!("linear-attention key-dim overflow"))?;
        let value_dim = self
            .manifest
            .linear_num_value_heads
            .checked_mul(self.manifest.linear_value_head_dim)
            .ok_or_else(|| eyre::eyre!("linear-attention value-dim overflow"))?;
        let conv_dim = key_dim
            .checked_mul(2)
            .and_then(|value| value.checked_add(value_dim))
            .ok_or_else(|| eyre::eyre!("linear-attention conv-dim overflow"))?;
        let mut query = Vec::with_capacity(seq_len * key_dim);
        let mut key = Vec::with_capacity(seq_len * key_dim);
        let mut value = Vec::with_capacity(seq_len * value_dim);
        for chunk in conv_mixed_qkv.chunks_exact(conv_dim) {
            query.extend_from_slice(&chunk[..key_dim]);
            key.extend_from_slice(&chunk[key_dim..key_dim * 2]);
            value.extend_from_slice(&chunk[key_dim * 2..]);
        }

        let z = tensor_to_vec_f32(&linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.linear_attn.in_proj_z.weight"))?,
            None,
        ))?;
        let beta = tensor_to_vec_f32(&linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.linear_attn.in_proj_b.weight"))?,
            None,
        ))?
        .into_iter()
        .map(sigmoid_scalar)
        .collect::<Vec<_>>();
        let a = tensor_to_vec_f32(&linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.linear_attn.in_proj_a.weight"))?,
            None,
        ))?;
        let dt_bias =
            tensor_to_vec_f32(&self.load_tensor_1d(&format!("{prefix}.linear_attn.dt_bias"))?)?;
        let a_log =
            tensor_to_vec_f32(&self.load_tensor_1d(&format!("{prefix}.linear_attn.A_log"))?)?;
        let mut g = Vec::with_capacity(a.len());
        for (index, value) in a.iter().copied().enumerate() {
            let head_index = index % self.manifest.linear_num_value_heads;
            let a_scale = -a_log[head_index].exp();
            g.push(a_scale * softplus_scalar(value + dt_bias[head_index]));
        }

        let repeat_factor = self.manifest.linear_num_value_heads / self.manifest.linear_num_key_heads;
        let query = repeat_linear_attention_heads(
            &query,
            seq_len,
            self.manifest.linear_num_key_heads,
            repeat_factor,
            self.manifest.linear_key_head_dim,
        );
        let key = repeat_linear_attention_heads(
            &key,
            seq_len,
            self.manifest.linear_num_key_heads,
            repeat_factor,
            self.manifest.linear_key_head_dim,
        );
        let query = l2_norm_heads(
            &query,
            seq_len,
            self.manifest.linear_num_value_heads,
            self.manifest.linear_key_head_dim,
        )?;
        let key = l2_norm_heads(
            &key,
            seq_len,
            self.manifest.linear_num_value_heads,
            self.manifest.linear_key_head_dim,
        )?;
        let core_attn_out = recurrent_gated_delta_rule(
            &query,
            &key,
            &value,
            &g,
            &beta,
            seq_len,
            self.manifest.linear_num_value_heads,
            self.manifest.linear_key_head_dim,
            self.manifest.linear_value_head_dim,
        )?;
        let norm_weight =
            tensor_to_vec_f32(&self.load_tensor_1d(&format!("{prefix}.linear_attn.norm.weight"))?)?;
        let normalized = qwen_rms_norm_gated(
            &core_attn_out,
            seq_len,
            self.manifest.linear_num_value_heads,
            self.manifest.linear_value_head_dim,
            &norm_weight,
            self.manifest.rms_norm_eps as f32,
            &z,
        )?;
        let normalized = tensor_3d([batch_size, seq_len, value_dim], normalized, &self.device);
        Ok(linear_forward_3d(
            &normalized,
            self.load_tensor_2d(&format!("{prefix}.linear_attn.out_proj.weight"))?,
            None,
        ))
    }

    #[allow(dead_code)]
    fn forward_linear_attention_with_state(
        &self,
        prefix: &str,
        hidden_states: Tensor<B, 3>,
        decode_state: &mut LinearAttentionDecodeState,
    ) -> eyre::Result<Tensor<B, 3>> {
        let [batch_size, seq_len, _hidden_size] = hidden_states.dims();
        ensure!(batch_size == 1, "Qwen3.5 linear attention currently expects batch size 1");
        let mixed_qkv = linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.linear_attn.in_proj_qkv.weight"))?,
            None,
        );
        let mixed_qkv_values = tensor_to_vec_f32(&mixed_qkv)?;
        let conv_weight = self.load_tensor(&format!("{prefix}.linear_attn.conv1d.weight"))?;
        let conv_mixed_qkv = depthwise_causal_conv1d_silu(
            &mixed_qkv_values,
            batch_size,
            seq_len,
            self.manifest.linear_num_key_heads * self.manifest.linear_key_head_dim * 2
                + self.manifest.linear_num_value_heads * self.manifest.linear_value_head_dim,
            self.manifest.linear_conv_kernel_dim,
            &conv_weight.values,
            &conv_weight.shape,
        )?;

        let key_dim = self
            .manifest
            .linear_num_key_heads
            .checked_mul(self.manifest.linear_key_head_dim)
            .ok_or_else(|| eyre::eyre!("linear-attention key-dim overflow"))?;
        let value_dim = self
            .manifest
            .linear_num_value_heads
            .checked_mul(self.manifest.linear_value_head_dim)
            .ok_or_else(|| eyre::eyre!("linear-attention value-dim overflow"))?;
        let conv_dim = key_dim
            .checked_mul(2)
            .and_then(|value| value.checked_add(value_dim))
            .ok_or_else(|| eyre::eyre!("linear-attention conv-dim overflow"))?;
        let mut query = Vec::with_capacity(seq_len * key_dim);
        let mut key = Vec::with_capacity(seq_len * key_dim);
        let mut value = Vec::with_capacity(seq_len * value_dim);
        for chunk in conv_mixed_qkv.chunks_exact(conv_dim) {
            query.extend_from_slice(&chunk[..key_dim]);
            key.extend_from_slice(&chunk[key_dim..key_dim * 2]);
            value.extend_from_slice(&chunk[key_dim * 2..]);
        }

        let z = tensor_to_vec_f32(&linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.linear_attn.in_proj_z.weight"))?,
            None,
        ))?;
        let beta = tensor_to_vec_f32(&linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.linear_attn.in_proj_b.weight"))?,
            None,
        ))?
        .into_iter()
        .map(sigmoid_scalar)
        .collect::<Vec<_>>();
        let a = tensor_to_vec_f32(&linear_forward_3d(
            &hidden_states,
            self.load_tensor_2d(&format!("{prefix}.linear_attn.in_proj_a.weight"))?,
            None,
        ))?;
        let dt_bias =
            tensor_to_vec_f32(&self.load_tensor_1d(&format!("{prefix}.linear_attn.dt_bias"))?)?;
        let a_log =
            tensor_to_vec_f32(&self.load_tensor_1d(&format!("{prefix}.linear_attn.A_log"))?)?;
        let mut g = Vec::with_capacity(a.len());
        for (index, value) in a.iter().copied().enumerate() {
            let head_index = index % self.manifest.linear_num_value_heads;
            let a_scale = -a_log[head_index].exp();
            g.push(a_scale * softplus_scalar(value + dt_bias[head_index]));
        }

        let repeat_factor = self.manifest.linear_num_value_heads / self.manifest.linear_num_key_heads;
        let query = repeat_linear_attention_heads(
            &query,
            seq_len,
            self.manifest.linear_num_key_heads,
            repeat_factor,
            self.manifest.linear_key_head_dim,
        );
        let key = repeat_linear_attention_heads(
            &key,
            seq_len,
            self.manifest.linear_num_key_heads,
            repeat_factor,
            self.manifest.linear_key_head_dim,
        );
        let query = l2_norm_heads(
            &query,
            seq_len,
            self.manifest.linear_num_value_heads,
            self.manifest.linear_key_head_dim,
        )?;
        let key = l2_norm_heads(
            &key,
            seq_len,
            self.manifest.linear_num_value_heads,
            self.manifest.linear_key_head_dim,
        )?;
        let (core_attn_out, final_state) = recurrent_gated_delta_rule_with_final_state(
            &query,
            &key,
            &value,
            &g,
            &beta,
            seq_len,
            self.manifest.linear_num_value_heads,
            self.manifest.linear_key_head_dim,
            self.manifest.linear_value_head_dim,
        )?;
        decode_state.state = final_state;
        let raw_mixed_dim = key_dim
            .checked_mul(2)
            .and_then(|value| value.checked_add(value_dim))
            .ok_or_else(|| eyre::eyre!("linear-attention raw mixed dim overflow"))?;
        decode_state.conv_history.clear();
        let keep_history = self.manifest.linear_conv_kernel_dim.saturating_sub(1);
        let total_tokens = mixed_qkv_values.len() / raw_mixed_dim;
        let history_start = total_tokens.saturating_sub(keep_history);
        for token_index in history_start..total_tokens {
            let start = token_index * raw_mixed_dim;
            let end = start + raw_mixed_dim;
            decode_state
                .conv_history
                .push_back(mixed_qkv_values[start..end].to_vec());
        }

        let norm_weight =
            tensor_to_vec_f32(&self.load_tensor_1d(&format!("{prefix}.linear_attn.norm.weight"))?)?;
        let normalized = qwen_rms_norm_gated(
            &core_attn_out,
            seq_len,
            self.manifest.linear_num_value_heads,
            self.manifest.linear_value_head_dim,
            &norm_weight,
            self.manifest.rms_norm_eps as f32,
            &z,
        )?;
        let normalized = tensor_3d([batch_size, seq_len, value_dim], normalized, &self.device);
        Ok(linear_forward_3d(
            &normalized,
            self.load_tensor_2d(&format!("{prefix}.linear_attn.out_proj.weight"))?,
            None,
        ))
    }

    #[allow(dead_code)]
    fn greedy_next_token(&self, hidden_states: Tensor<B, 3>) -> eyre::Result<usize> {
        llm_tracy_zone!("llm_burn_greedy_next_token");
        #[cfg(feature = "extended_observability")]
        let _span = tracing::debug_span!("llm_burn_greedy_next_token").entered();
        let [batch_size, seq_len, hidden_size] = hidden_states.dims();
        ensure!(batch_size == 1, "Qwen3.5 greedy decode expected batch size 1");
        ensure!(seq_len > 0, "Qwen3.5 greedy decode expected at least one timestep");
        let last_hidden = tensor_to_vec_f32(&hidden_states)?
            .chunks_exact(hidden_size)
            .last()
            .map(ToOwned::to_owned)
            .ok_or_else(|| eyre::eyre!("Missing final hidden state during greedy decode"))?;
        self.greedy_next_token_from_hidden(&last_hidden)
    }

    fn greedy_next_token_from_hidden(&self, hidden: &[f32]) -> eyre::Result<usize> {
        llm_tracy_zone!("llm_burn_lm_head_scan");
        #[cfg(feature = "extended_observability")]
        let _span = tracing::debug_span!("llm_burn_lm_head_scan", hidden_size = hidden.len()).entered();
        if !self.lm_head_tensor_disabled.load(Ordering::Relaxed) {
            match self.greedy_next_token_from_hidden_tensor(hidden) {
                Ok(token_id) => return Ok(token_id),
                Err(error) => {
                    self.lm_head_tensor_disabled.store(true, Ordering::Relaxed);
                    trace_burn_text_runtime(&format!(
                        "device lm_head path unavailable; falling back to row scan: {error}"
                    ));
                }
            }
        }
        let lm_head_name = if self.manifest.tensors.contains_key("lm_head.weight") {
            "lm_head.weight"
        } else {
            "model.embed_tokens.weight"
        };
        let spec = self.tensor_spec(lm_head_name)?;
        ensure!(
            spec.shape.len() == 2,
            "Expected a 2D lm_head tensor for `{lm_head_name}`"
        );
        let vocab_size = spec.shape[0];
        let hidden_size = spec.shape[1];
        ensure!(
            hidden.len() == hidden_size,
            "Qwen3.5 greedy decode expected hidden size {hidden_size}, but found {}",
            hidden.len()
        );
        let bytes_per_element = bytes_per_element(&spec.dtype)?;
        let row_bytes = hidden_size
            .checked_mul(bytes_per_element)
            .ok_or_else(|| eyre::eyre!("lm_head row-size overflow"))?;
        let max_chunk_rows = DEFAULT_LOGIT_CHUNK_ROWS.min(vocab_size.max(1));
        let chunk_bytes = max_chunk_rows
            .checked_mul(row_bytes)
            .ok_or_else(|| eyre::eyre!("lm_head chunk-size overflow"))?;
        let path = self.tensor_file_path(spec);
        let (offset_bytes, byte_len) = self.tensor_byte_range(spec, lm_head_name)?;
        ensure!(
            byte_len == vocab_size * row_bytes,
            "Burn text lm_head tensor `{lm_head_name}` byte range {} did not match {} rows of {} bytes",
            byte_len,
            vocab_size,
            row_bytes
        );
        let mut best_index = 0_usize;
        let mut best_value = f32::NEG_INFINITY;
        let mut row_start = 0_usize;
        let mut buffer = vec![0_u8; chunk_bytes];
        if let Some(file) = &self.packed_tensor_file
            && self.packed_tensor_path.as_ref().is_some_and(|packed| packed == &path)
        {
            let mut file = file.lock().map_err(|_| {
                eyre::eyre!(
                    "Packed Burn text tensor file mutex was poisoned for {}",
                    path.display()
                )
            })?;
            while row_start < vocab_size {
                let rows = (vocab_size - row_start).min(max_chunk_rows);
                let bytes = rows
                    .checked_mul(row_bytes)
                    .ok_or_else(|| eyre::eyre!("lm_head chunk read overflow"))?;
                let byte_offset = offset_bytes
                    .checked_add(
                        u64::try_from(row_start.saturating_mul(row_bytes))
                            .unwrap_or(u64::MAX),
                    )
                    .ok_or_else(|| eyre::eyre!("lm_head chunk offset overflow"))?;
                file.seek(SeekFrom::Start(byte_offset)).wrap_err_with(|| {
                    format!(
                        "Failed to seek to lm_head chunk starting at row {} in {}",
                        row_start,
                        path.display()
                    )
                })?;
                file.read_exact(&mut buffer[..bytes]).wrap_err_with(|| {
                    format!(
                        "Failed to read lm_head chunk starting at row {} from {}",
                        row_start,
                        path.display()
                    )
                })?;
                let mut values = Vec::with_capacity(rows * hidden_size);
                decode_bytes_into_f32(&buffer[..bytes], &spec.dtype, &mut values)?;
                for (row_offset, row) in values.chunks_exact(hidden_size).enumerate() {
                    let logit = dot_product(hidden, row);
                    if logit > best_value {
                        best_value = logit;
                        best_index = row_start + row_offset;
                    }
                }
                row_start += rows;
            }
            return Ok(best_index);
        }

        let mut file = File::open(&path)
            .wrap_err_with(|| format!("Failed to open lm_head tensor {}", path.display()))?;
        while row_start < vocab_size {
            let rows = (vocab_size - row_start).min(max_chunk_rows);
            let bytes = rows
                .checked_mul(row_bytes)
                .ok_or_else(|| eyre::eyre!("lm_head chunk read overflow"))?;
            let byte_offset = offset_bytes
                .checked_add(
                    u64::try_from(row_start.saturating_mul(row_bytes))
                        .unwrap_or(u64::MAX),
                )
                .ok_or_else(|| eyre::eyre!("lm_head chunk offset overflow"))?;
            file.seek(SeekFrom::Start(byte_offset)).wrap_err_with(|| {
                format!(
                    "Failed to seek to lm_head chunk starting at row {} in {}",
                    row_start,
                    path.display()
                )
            })?;
            file.read_exact(&mut buffer[..bytes]).wrap_err_with(|| {
                format!(
                    "Failed to read lm_head chunk starting at row {} from {}",
                    row_start,
                    path.display()
                )
            })?;
            let mut values = Vec::with_capacity(rows * hidden_size);
            decode_bytes_into_f32(&buffer[..bytes], &spec.dtype, &mut values)?;
            for (row_offset, row) in values.chunks_exact(hidden_size).enumerate() {
                let logit = dot_product(hidden, row);
                if logit > best_value {
                    best_value = logit;
                    best_index = row_start + row_offset;
                }
            }
            row_start += rows;
        }
        Ok(best_index)
    }

    fn greedy_next_token_from_hidden_tensor(&self, hidden: &[f32]) -> eyre::Result<usize> {
        let lm_head_name = if self.manifest.tensors.contains_key("lm_head.weight") {
            "lm_head.weight"
        } else {
            "model.embed_tokens.weight"
        };
        let logits = linear_forward_3d(
            &tensor_3d([1, 1, hidden.len()], hidden.to_vec(), &self.device),
            self.load_tensor_2d(lm_head_name)?,
            None,
        );
        let logits = tensor_to_vec_f32(&logits)?;
        let (best_token_id, _) = logits
            .into_iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| {
                left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal)
            })
            .ok_or_else(|| eyre::eyre!("lm_head logits were empty"))?;
        Ok(best_token_id)
    }
}

fn hidden_activation_kind(hidden_act: &str) -> eyre::Result<HiddenActivationKind> {
    match hidden_act.trim() {
        "silu" | "swish" => Ok(HiddenActivationKind::Silu),
        other => bail!(
            "Burn text runtime does not support hidden_act `{other}` yet; manifest/export must derive a supported activation from model config"
        ),
    }
}

impl fmt::Display for GenerationTimeoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Burn text generation timed out after {} while producing {} of {} requested tokens",
            humantime::format_duration(self.elapsed),
            self.generated_token_count,
            self.max_new_tokens
        )
    }
}

impl std::error::Error for GenerationTimeoutError {}

impl GenerationDeadline {
    fn new(timeout: Option<Duration>) -> Self {
        Self {
            started_at: Instant::now(),
            timeout,
        }
    }

    fn check(&self, generated_token_count: usize, max_new_tokens: usize) -> eyre::Result<()> {
        let Some(timeout) = self.timeout else {
            return Ok(());
        };
        let elapsed = self.started_at.elapsed();
        if elapsed <= timeout {
            return Ok(());
        }
        Err(eyre::Report::new(GenerationTimeoutError {
            elapsed,
            generated_token_count,
            max_new_tokens,
        }))
    }
}

fn is_generation_timeout_error(error: &eyre::Report) -> bool {
    error.downcast_ref::<GenerationTimeoutError>().is_some()
}

fn validate_manifest_contract(manifest: &BurnTextManifest, manifest_path: &Path) -> eyre::Result<()> {
    ensure!(
        manifest.format_version == 1,
        "Burn text manifest {} used unsupported format_version {}",
        manifest_path.display(),
        manifest.format_version
    );
    ensure!(
        manifest.text_model_type == "qwen3_5_text",
        "Burn text manifest {} used unsupported text model type `{}`",
        manifest_path.display(),
        manifest.text_model_type
    );
    let _ = hidden_activation_kind(&manifest.hidden_act)?;
    ensure!(
        manifest.hidden_size > 0
            && manifest.intermediate_size > 0
            && manifest.num_hidden_layers > 0
            && manifest.num_attention_heads > 0
            && manifest.num_key_value_heads > 0
            && manifest.head_dim > 0
            && manifest.vocab_size > 0,
        "Burn text manifest {} contained zero-valued core dimensions",
        manifest_path.display()
    );
    ensure!(
        manifest.hidden_size == manifest.num_attention_heads * manifest.head_dim,
        "Burn text manifest {} declared hidden_size {} but num_attention_heads {} x head_dim {} = {}",
        manifest_path.display(),
        manifest.hidden_size,
        manifest.num_attention_heads,
        manifest.head_dim,
        manifest.num_attention_heads * manifest.head_dim
    );
    ensure!(
        manifest.num_attention_heads.is_multiple_of(manifest.num_key_value_heads),
        "Burn text manifest {} declared {} attention heads and {} key/value heads, which is not an integer repeat factor",
        manifest_path.display(),
        manifest.num_attention_heads,
        manifest.num_key_value_heads
    );
    ensure!(
        manifest.linear_num_value_heads.is_multiple_of(manifest.linear_num_key_heads),
        "Burn text manifest {} declared {} linear value heads and {} linear key heads, which is not an integer repeat factor",
        manifest_path.display(),
        manifest.linear_num_value_heads,
        manifest.linear_num_key_heads
    );
    validate_manifest_tensor_shape(
        manifest,
        "model.embed_tokens.weight",
        &[manifest.vocab_size, manifest.hidden_size],
    )?;
    validate_manifest_tensor_shape(manifest, "model.norm.weight", &[manifest.hidden_size])?;
    if manifest.tensors.contains_key("lm_head.weight") {
        validate_manifest_tensor_shape(
            manifest,
            "lm_head.weight",
            &[manifest.vocab_size, manifest.hidden_size],
        )?;
    }
    for layer_index in 0..manifest.num_hidden_layers {
        let prefix = format!("model.layers.{layer_index}");
        validate_manifest_tensor_shape(
            manifest,
            &format!("{prefix}.input_layernorm.weight"),
            &[manifest.hidden_size],
        )?;
        validate_manifest_tensor_shape(
            manifest,
            &format!("{prefix}.post_attention_layernorm.weight"),
            &[manifest.hidden_size],
        )?;
        validate_manifest_tensor_shape(
            manifest,
            &format!("{prefix}.mlp.gate_proj.weight"),
            &[manifest.intermediate_size, manifest.hidden_size],
        )?;
        validate_manifest_tensor_shape(
            manifest,
            &format!("{prefix}.mlp.up_proj.weight"),
            &[manifest.intermediate_size, manifest.hidden_size],
        )?;
        validate_manifest_tensor_shape(
            manifest,
            &format!("{prefix}.mlp.down_proj.weight"),
            &[manifest.hidden_size, manifest.intermediate_size],
        )?;
        match manifest.layer_types[layer_index].as_str() {
            "full_attention" => {
                validate_manifest_tensor_shape(
                    manifest,
                    &format!("{prefix}.self_attn.q_proj.weight"),
                    &[manifest.num_attention_heads * manifest.head_dim * 2, manifest.hidden_size],
                )?;
                validate_manifest_tensor_shape(
                    manifest,
                    &format!("{prefix}.self_attn.k_proj.weight"),
                    &[manifest.num_key_value_heads * manifest.head_dim, manifest.hidden_size],
                )?;
                validate_manifest_tensor_shape(
                    manifest,
                    &format!("{prefix}.self_attn.v_proj.weight"),
                    &[manifest.num_key_value_heads * manifest.head_dim, manifest.hidden_size],
                )?;
                validate_manifest_tensor_shape(
                    manifest,
                    &format!("{prefix}.self_attn.o_proj.weight"),
                    &[manifest.hidden_size, manifest.hidden_size],
                )?;
                validate_manifest_tensor_shape(
                    manifest,
                    &format!("{prefix}.self_attn.q_norm.weight"),
                    &[manifest.head_dim],
                )?;
                validate_manifest_tensor_shape(
                    manifest,
                    &format!("{prefix}.self_attn.k_norm.weight"),
                    &[manifest.head_dim],
                )?;
            }
            "linear_attention" => {
                let linear_key_width = manifest.linear_num_key_heads * manifest.linear_key_head_dim;
                let linear_value_width =
                    manifest.linear_num_value_heads * manifest.linear_value_head_dim;
                let linear_qkv_width = linear_key_width * 2 + linear_value_width;
                validate_manifest_tensor_shape(
                    manifest,
                    &format!("{prefix}.linear_attn.conv1d.weight"),
                    &[linear_qkv_width, 1, manifest.linear_conv_kernel_dim],
                )?;
                validate_manifest_tensor_shape(
                    manifest,
                    &format!("{prefix}.linear_attn.dt_bias"),
                    &[manifest.linear_num_value_heads],
                )?;
                validate_manifest_tensor_shape(
                    manifest,
                    &format!("{prefix}.linear_attn.A_log"),
                    &[manifest.linear_num_value_heads],
                )?;
                validate_manifest_tensor_shape(
                    manifest,
                    &format!("{prefix}.linear_attn.norm.weight"),
                    &[manifest.linear_value_head_dim],
                )?;
                validate_manifest_tensor_shape(
                    manifest,
                    &format!("{prefix}.linear_attn.out_proj.weight"),
                    &[manifest.hidden_size, linear_value_width],
                )?;
                validate_manifest_tensor_shape(
                    manifest,
                    &format!("{prefix}.linear_attn.in_proj_qkv.weight"),
                    &[linear_qkv_width, manifest.hidden_size],
                )?;
                validate_manifest_tensor_shape(
                    manifest,
                    &format!("{prefix}.linear_attn.in_proj_z.weight"),
                    &[linear_value_width, manifest.hidden_size],
                )?;
                validate_manifest_tensor_shape(
                    manifest,
                    &format!("{prefix}.linear_attn.in_proj_b.weight"),
                    &[manifest.linear_num_value_heads, manifest.hidden_size],
                )?;
                validate_manifest_tensor_shape(
                    manifest,
                    &format!("{prefix}.linear_attn.in_proj_a.weight"),
                    &[manifest.linear_num_value_heads, manifest.hidden_size],
                )?;
            }
            other => bail!(
                "Burn text manifest {} used unsupported layer type `{other}` at layer {}",
                manifest_path.display(),
                layer_index
            ),
        }
    }
    Ok(())
}

fn validate_manifest_tensor_shape(
    manifest: &BurnTextManifest,
    tensor_name: &str,
    expected_shape: &[usize],
) -> eyre::Result<()> {
    let spec = manifest
        .tensors
        .get(tensor_name)
        .ok_or_else(|| eyre::eyre!("Burn text manifest is missing tensor `{tensor_name}`"))?;
    ensure!(
        spec.shape == expected_shape,
        "Burn text tensor `{tensor_name}` expected shape {:?} but manifest declared {:?}",
        expected_shape,
        spec.shape
    );
    Ok(())
}

#[must_use]
pub fn burn_text_dir(root: &Path) -> PathBuf {
    root.join(BURN_TEXT_DIR_NAME)
}

#[must_use]
pub fn burn_text_manifest_path(root: &Path) -> PathBuf {
    burn_text_dir(root).join(BURN_TEXT_MANIFEST_FILE_NAME)
}

fn shared_packed_tensor_path(root: &Path, manifest: &BurnTextManifest) -> Option<PathBuf> {
    let mut tensor_paths = manifest
        .tensors
        .values()
        .filter(|spec| spec.offset_bytes.is_some() && spec.byte_len.is_some())
        .map(|spec| spec.path.as_str());
    let first_path = tensor_paths.next()?;
    if tensor_paths.all(|path| path == first_path) {
        Some(root.join(first_path))
    } else {
        None
    }
}

#[must_use]
pub fn inspect_burn_text_runtime_status(root: &Path) -> BurnTextRuntimeStatus {
    let directory = burn_text_dir(root);
    let manifest_path = burn_text_manifest_path(root);
    BurnTextRuntimeStatus {
        directory: directory.display().to_string(),
        manifest_path: manifest_path.display().to_string(),
        exists: manifest_path.is_file(),
    }
}

/// # Errors
///
/// This function will return an error if the Burn text export cannot be written.
pub fn export_burn_text_weights(
    artifacts: &LlmModelArtifacts,
    overwrite: bool,
    dtype: &str,
) -> eyre::Result<LlmReferenceBurnTextExportReport> {
    let output_dir = burn_text_dir(&artifacts.root);
    let report = export_llm_reference_burn_text_model(
        &artifacts.metadata.source_repo_id,
        &output_dir,
        dtype,
        overwrite,
    )?;
    let manifest = load_burn_text_manifest(&burn_text_manifest_path(&artifacts.root))?;
    ensure!(
        report.tensor_count == manifest.tensors.len(),
        "Python Burn text export reported {} tensors but manifest {} contained {}",
        report.tensor_count,
        burn_text_manifest_path(&artifacts.root).display(),
        manifest.tensors.len()
    );
    Ok(report)
}

/// # Errors
///
/// This function will return an error if the Burn text manifest cannot be read or parsed.
pub fn load_burn_text_manifest(path: &Path) -> eyre::Result<BurnTextManifest> {
    let bytes = std::fs::read(path)
        .wrap_err_with(|| format!("Failed to read Burn text manifest {}", path.display()))?;
    facet_json::from_slice(&bytes)
        .wrap_err_with(|| format!("Failed to parse Burn text manifest {}", path.display()))
}

/// # Errors
///
/// This function will return an error if the Burn text runtime is missing or the prompt cannot be
/// generated.
pub fn generate_with_burn_text_runtime(
    artifacts: &LlmModelArtifacts,
    prompt_token_ids: &[u32],
    options: &BurnTextGenerationOptions,
) -> eyre::Result<BurnTextGenerationReport> {
    let generation_mode = burn_text_generation_mode();
    if burn_text_prefers_cuda_by_default()
        && let Some(report) =
            try_generate_with_cuda_backend(artifacts, prompt_token_ids, options, generation_mode)?
    {
        return Ok(report);
    }
    generate_with_selected_burn_text_runtime_on_device::<LlmCpuBackend>(
        artifacts,
        prompt_token_ids,
        options,
        generation_mode,
        NdArrayDevice::default(),
        "cpu-ndarray",
    )
}

/// # Errors
///
/// This function will return an error if either the stable or experimental incremental Burn path
/// cannot run for the provided prompt.
pub fn compare_with_incremental_burn_text_runtime(
    artifacts: &LlmModelArtifacts,
    prompt_token_ids: &[u32],
    options: &BurnTextGenerationOptions,
) -> eyre::Result<BurnTextIncrementalComparisonReport> {
    if burn_text_prefers_cuda_by_default()
        && let Some(report) = try_compare_with_incremental_cuda_backend(
            artifacts,
            prompt_token_ids,
            options,
        )?
    {
        return Ok(report);
    }
    compare_with_incremental_burn_text_runtime_on_device::<LlmCpuBackend>(
        artifacts,
        prompt_token_ids,
        options,
        NdArrayDevice::default(),
        "cpu-ndarray",
    )
}

/// # Errors
///
/// This function will return an error if either the stable or experimental incremental Burn path
/// cannot reproduce the final prompt hidden state for the provided prompt.
pub fn diagnose_incremental_hidden_burn_text_runtime(
    artifacts: &LlmModelArtifacts,
    prompt_token_ids: &[u32],
) -> eyre::Result<BurnTextIncrementalHiddenDiagnosticsReport> {
    if burn_text_prefers_cuda_by_default()
        && let Some(report) =
            try_diagnose_incremental_hidden_cuda_backend(artifacts, prompt_token_ids)?
    {
        return Ok(report);
    }
    diagnose_incremental_hidden_burn_text_runtime_on_device::<LlmCpuBackend>(
        artifacts,
        prompt_token_ids,
        NdArrayDevice::default(),
        "cpu-ndarray",
    )
}

/// # Errors
///
/// This function will return an error if either the stable or experimental incremental Burn path
/// cannot produce per-layer hidden diagnostics for the provided prompt.
pub fn diagnose_incremental_layers_burn_text_runtime(
    artifacts: &LlmModelArtifacts,
    prompt_token_ids: &[u32],
) -> eyre::Result<BurnTextIncrementalLayerDiagnosticsReport> {
    if burn_text_prefers_cuda_by_default()
        && let Some(report) =
            try_diagnose_incremental_layers_cuda_backend(artifacts, prompt_token_ids)?
    {
        return Ok(report);
    }
    diagnose_incremental_layers_burn_text_runtime_on_device::<LlmCpuBackend>(
        artifacts,
        prompt_token_ids,
        NdArrayDevice::default(),
        "cpu-ndarray",
    )
}

/// # Errors
///
/// This function will return an error if the Burn text runtime cannot collect final-token layer
/// outputs for the provided prompt.
pub fn collect_full_layer_outputs_burn_text_runtime(
    artifacts: &LlmModelArtifacts,
    prompt_token_ids: &[u32],
) -> eyre::Result<BurnTextLayerOutputsReport> {
    if burn_text_prefers_cuda_by_default()
        && let Some(report) = try_collect_full_layer_outputs_cuda_backend(artifacts, prompt_token_ids)?
    {
        return Ok(report);
    }
    collect_full_layer_outputs_burn_text_runtime_on_device::<LlmCpuBackend>(
        artifacts,
        prompt_token_ids,
        NdArrayDevice::default(),
        "cpu-ndarray",
    )
}

#[must_use]
pub fn llm_inference_cuda_device() -> CudaDevice {
    CudaDevice::default()
}

fn burn_text_prefers_cuda_by_default() -> bool {
    !matches!(
        std::env::var("TEAMY_STUDIO_LLM_BACKEND"),
        Ok(value) if value.trim().eq_ignore_ascii_case("cpu")
    )
}

fn burn_text_generation_mode() -> BurnTextGenerationMode {
    match std::env::var("TEAMY_STUDIO_LLM_BURN_GENERATION_MODE") {
        Ok(value) if value.trim().eq_ignore_ascii_case("full") => BurnTextGenerationMode::Full,
        Ok(value) if value.trim().eq_ignore_ascii_case("incremental") => {
            BurnTextGenerationMode::Incremental
        }
        _ => BurnTextGenerationMode::Hybrid,
    }
}

fn generate_with_selected_burn_text_runtime_on_device<B: Backend>(
    artifacts: &LlmModelArtifacts,
    prompt_token_ids: &[u32],
    options: &BurnTextGenerationOptions,
    generation_mode: BurnTextGenerationMode,
    device: B::Device,
    backend_label: &str,
) -> eyre::Result<BurnTextGenerationReport> {
    match generation_mode {
        BurnTextGenerationMode::Full => generate_with_burn_text_runtime_on_device::<B>(
            artifacts,
            prompt_token_ids,
            options,
            device,
            backend_label,
        ),
        BurnTextGenerationMode::Incremental => {
            generate_with_incremental_burn_text_runtime_on_device::<B>(
                artifacts,
                prompt_token_ids,
                options,
                device,
                backend_label,
            )
        }
        BurnTextGenerationMode::Hybrid => generate_with_hybrid_burn_text_runtime_on_device::<B>(
            artifacts,
            prompt_token_ids,
            options,
            device,
            backend_label,
        ),
    }
}

fn try_generate_with_cuda_backend(
    artifacts: &LlmModelArtifacts,
    prompt_token_ids: &[u32],
    options: &BurnTextGenerationOptions,
    generation_mode: BurnTextGenerationMode,
) -> eyre::Result<Option<BurnTextGenerationReport>> {
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        generate_with_selected_burn_text_runtime_on_device::<LlmCudaBackend>(
            artifacts,
            prompt_token_ids,
            options,
            generation_mode,
            llm_inference_cuda_device(),
            "cuda",
        )
    }));
    match attempt {
        Ok(Ok(report)) => Ok(Some(report)),
        Ok(Err(error)) => {
            if is_generation_timeout_error(&error) {
                return Err(error);
            }
            trace_burn_text_runtime(&format!(
                "CUDA backend unavailable for Burn text runtime; falling back to CPU: {error}"
            ));
            Ok(None)
        }
        Err(_) => {
            trace_burn_text_runtime(
                "CUDA backend panicked during Burn text runtime initialization; falling back to CPU",
            );
            Ok(None)
        }
    }
}

fn try_compare_with_incremental_cuda_backend(
    artifacts: &LlmModelArtifacts,
    prompt_token_ids: &[u32],
    options: &BurnTextGenerationOptions,
) -> eyre::Result<Option<BurnTextIncrementalComparisonReport>> {
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        compare_with_incremental_burn_text_runtime_on_device::<LlmCudaBackend>(
            artifacts,
            prompt_token_ids,
            options,
            llm_inference_cuda_device(),
            "cuda",
        )
    }));
    match attempt {
        Ok(Ok(report)) => Ok(Some(report)),
        Ok(Err(error)) => {
            if is_generation_timeout_error(&error) {
                return Err(error);
            }
            trace_burn_text_runtime(&format!(
                "CUDA backend unavailable for Burn incremental comparison; falling back to CPU: {error}"
            ));
            Ok(None)
        }
        Err(_) => {
            trace_burn_text_runtime(
                "CUDA backend panicked during Burn incremental comparison initialization; falling back to CPU",
            );
            Ok(None)
        }
    }
}

fn try_diagnose_incremental_hidden_cuda_backend(
    artifacts: &LlmModelArtifacts,
    prompt_token_ids: &[u32],
) -> eyre::Result<Option<BurnTextIncrementalHiddenDiagnosticsReport>> {
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        diagnose_incremental_hidden_burn_text_runtime_on_device::<LlmCudaBackend>(
            artifacts,
            prompt_token_ids,
            llm_inference_cuda_device(),
            "cuda",
        )
    }));
    match attempt {
        Ok(Ok(report)) => Ok(Some(report)),
        Ok(Err(error)) => {
            trace_burn_text_runtime(&format!(
                "CUDA backend unavailable for Burn incremental hidden diagnostics; falling back to CPU: {error}"
            ));
            Ok(None)
        }
        Err(_) => {
            trace_burn_text_runtime(
                "CUDA backend panicked during Burn incremental hidden diagnostics; falling back to CPU",
            );
            Ok(None)
        }
    }
}

fn try_diagnose_incremental_layers_cuda_backend(
    artifacts: &LlmModelArtifacts,
    prompt_token_ids: &[u32],
) -> eyre::Result<Option<BurnTextIncrementalLayerDiagnosticsReport>> {
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        diagnose_incremental_layers_burn_text_runtime_on_device::<LlmCudaBackend>(
            artifacts,
            prompt_token_ids,
            llm_inference_cuda_device(),
            "cuda",
        )
    }));
    match attempt {
        Ok(Ok(report)) => Ok(Some(report)),
        Ok(Err(error)) => {
            trace_burn_text_runtime(&format!(
                "CUDA backend unavailable for Burn incremental layer diagnostics; falling back to CPU: {error}"
            ));
            Ok(None)
        }
        Err(_) => {
            trace_burn_text_runtime(
                "CUDA backend panicked during Burn incremental layer diagnostics; falling back to CPU",
            );
            Ok(None)
        }
    }
}

fn try_collect_full_layer_outputs_cuda_backend(
    artifacts: &LlmModelArtifacts,
    prompt_token_ids: &[u32],
) -> eyre::Result<Option<BurnTextLayerOutputsReport>> {
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        collect_full_layer_outputs_burn_text_runtime_on_device::<LlmCudaBackend>(
            artifacts,
            prompt_token_ids,
            llm_inference_cuda_device(),
            "cuda",
        )
    }));
    match attempt {
        Ok(Ok(report)) => Ok(Some(report)),
        Ok(Err(error)) => {
            trace_burn_text_runtime(&format!(
                "CUDA backend unavailable for Burn full-layer outputs; falling back to CPU: {error}"
            ));
            Ok(None)
        }
        Err(_) => {
            trace_burn_text_runtime(
                "CUDA backend panicked during Burn full-layer outputs; falling back to CPU",
            );
            Ok(None)
        }
    }
}

fn compare_with_incremental_burn_text_runtime_on_device<B: Backend>(
    artifacts: &LlmModelArtifacts,
    prompt_token_ids: &[u32],
    options: &BurnTextGenerationOptions,
    device: B::Device,
    backend_label: &str,
) -> eyre::Result<BurnTextIncrementalComparisonReport> {
    llm_tracy_zone!("llm_burn_compare_incremental");
    #[cfg(feature = "extended_observability")]
    let _span = tracing::info_span!(
        "llm_burn_compare_incremental",
        backend = backend_label,
        prompt_token_count = prompt_token_ids.len(),
        max_new_tokens = options.max_new_tokens
    )
    .entered();
    ensure!(
        !prompt_token_ids.is_empty(),
        "Burn incremental comparison expected at least one prompt token"
    );
    let full = generate_with_burn_text_runtime_on_device::<B>(
        artifacts,
        prompt_token_ids,
        options,
        device.clone(),
        backend_label,
    )?;
    let incremental = generate_with_incremental_burn_text_runtime_on_device::<B>(
        artifacts,
        prompt_token_ids,
        options,
        device,
        backend_label,
    )?;
    let first_mismatch_index =
        first_mismatch_index(&full.generated_token_ids, &incremental.generated_token_ids);
    Ok(BurnTextIncrementalComparisonReport {
        backend: backend_label.to_owned(),
        token_match: first_mismatch_index.is_none(),
        first_mismatch_index,
        full_generated_token_ids: full.generated_token_ids,
        full_generated_text: full.generated_text,
        incremental_generated_token_ids: incremental.generated_token_ids,
        incremental_generated_text: incremental.generated_text,
    })
}

fn diagnose_incremental_hidden_burn_text_runtime_on_device<B: Backend>(
    artifacts: &LlmModelArtifacts,
    prompt_token_ids: &[u32],
    device: B::Device,
    backend_label: &str,
) -> eyre::Result<BurnTextIncrementalHiddenDiagnosticsReport> {
    llm_tracy_zone!("llm_burn_diagnose_incremental_hidden");
    #[cfg(feature = "extended_observability")]
    let _span = tracing::info_span!(
        "llm_burn_diagnose_incremental_hidden",
        backend = backend_label,
        prompt_token_count = prompt_token_ids.len()
    )
    .entered();
    ensure!(
        !prompt_token_ids.is_empty(),
        "Burn incremental hidden diagnostics expected at least one prompt token"
    );
    let runtime = Qwen35TextRuntime::<B>::load(artifacts, device)?;
    let first_prompt_token_hidden_diff = compare_hidden_slices(
        &full_last_hidden_for_prompt(&runtime, &prompt_token_ids[..1])?,
        &incremental_last_hidden_for_prompt(&runtime, &prompt_token_ids[..1])?,
    )?;
    let full_prompt_hidden_diff = compare_hidden_slices(
        &full_last_hidden_for_prompt(&runtime, prompt_token_ids)?,
        &incremental_last_hidden_for_prompt(&runtime, prompt_token_ids)?,
    )?;
    Ok(BurnTextIncrementalHiddenDiagnosticsReport {
        backend: backend_label.to_owned(),
        first_prompt_token_hidden_diff,
        full_prompt_hidden_diff,
    })
}

fn diagnose_incremental_layers_burn_text_runtime_on_device<B: Backend>(
    artifacts: &LlmModelArtifacts,
    prompt_token_ids: &[u32],
    device: B::Device,
    backend_label: &str,
) -> eyre::Result<BurnTextIncrementalLayerDiagnosticsReport> {
    llm_tracy_zone!("llm_burn_diagnose_incremental_layers");
    #[cfg(feature = "extended_observability")]
    let _span = tracing::info_span!(
        "llm_burn_diagnose_incremental_layers",
        backend = backend_label,
        prompt_token_count = prompt_token_ids.len()
    )
    .entered();
    ensure!(
        !prompt_token_ids.is_empty(),
        "Burn incremental layer diagnostics expected at least one prompt token"
    );
    let runtime = Qwen35TextRuntime::<B>::load(artifacts, device)?;
    let full_outputs = full_layer_outputs_for_prompt(&runtime, prompt_token_ids)?;
    let incremental_outputs = incremental_layer_outputs_for_prompt(&runtime, prompt_token_ids)?;
    ensure!(
        full_outputs.len() == incremental_outputs.len(),
        "Per-layer diagnostic output count mismatch: {} vs {}",
        full_outputs.len(),
        incremental_outputs.len()
    );
    let mut layer_differences = Vec::with_capacity(full_outputs.len());
    for (layer_index, (full_hidden, incremental_hidden)) in
        full_outputs.iter().zip(incremental_outputs.iter()).enumerate()
    {
        layer_differences.push(BurnTextLayerHiddenDifference {
            layer_index,
            layer_type: runtime.manifest.layer_types[layer_index].clone(),
            hidden_diff: compare_hidden_slices(full_hidden, incremental_hidden)?,
        });
    }
    let first_large_diff_layer_index = layer_differences
        .iter()
        .find(|layer| layer.hidden_diff.max_abs_diff > 0.1)
        .map(|layer| layer.layer_index);
    Ok(BurnTextIncrementalLayerDiagnosticsReport {
        backend: backend_label.to_owned(),
        first_large_diff_layer_index,
        layer_differences,
    })
}

fn collect_full_layer_outputs_burn_text_runtime_on_device<B: Backend>(
    artifacts: &LlmModelArtifacts,
    prompt_token_ids: &[u32],
    device: B::Device,
    backend_label: &str,
) -> eyre::Result<BurnTextLayerOutputsReport> {
    llm_tracy_zone!("llm_burn_collect_full_layer_outputs");
    #[cfg(feature = "extended_observability")]
    let _span = tracing::info_span!(
        "llm_burn_collect_full_layer_outputs",
        backend = backend_label,
        prompt_token_count = prompt_token_ids.len()
    )
    .entered();
    ensure!(
        !prompt_token_ids.is_empty(),
        "Burn full-layer output collection expected at least one prompt token"
    );
    let runtime = Qwen35TextRuntime::<B>::load(artifacts, device)?;
    let layer_last_hidden_states = full_layer_outputs_for_prompt(&runtime, prompt_token_ids)?;
    let final_norm_last_hidden = full_last_hidden_for_prompt(&runtime, prompt_token_ids)?;
    Ok(BurnTextLayerOutputsReport {
        backend: backend_label.to_owned(),
        layer_last_hidden_states,
        final_norm_last_hidden,
    })
}

fn generate_with_burn_text_runtime_on_device<B: Backend>(
    artifacts: &LlmModelArtifacts,
    prompt_token_ids: &[u32],
    options: &BurnTextGenerationOptions,
    device: B::Device,
    backend_label: &str,
) -> eyre::Result<BurnTextGenerationReport> {
    llm_tracy_zone!("llm_burn_generate");
    #[cfg(feature = "extended_observability")]
    let _span = tracing::info_span!(
        "llm_burn_generate",
        backend = backend_label,
        prompt_token_count = prompt_token_ids.len(),
        max_new_tokens = options.max_new_tokens
    )
    .entered();
    trace_burn_text_runtime(&format!(
        "using {backend_label} backend for Burn text runtime"
    ));
    let runtime = Qwen35TextRuntime::<B>::load(artifacts, device)?;
    let tokenizer = tokenizers::Tokenizer::from_file(&artifacts.tokenizer_path).map_err(|error| {
        eyre::eyre!(
            "Failed to load tokenizer from {}: {}",
            artifacts.tokenizer_path.display(),
            error
        )
    })?;
    let eos_token_id = load_tokenizer_config_summary(&artifacts.tokenizer_config_path)?
        .eos_token
        .as_deref()
        .and_then(|token| tokenizer.token_to_id(token))
        .and_then(|token_id| usize::try_from(token_id).ok());
    let mut all_token_ids = prompt_token_ids
        .iter()
        .copied()
        .map(|token_id| usize::try_from(token_id).unwrap_or(usize::MAX))
        .collect::<Vec<_>>();
    let mut generated_token_ids = Vec::new();
    let deadline = GenerationDeadline::new(options.generation_timeout);

    for token_index in 0..options.max_new_tokens {
        deadline.check(generated_token_ids.len(), options.max_new_tokens)?;
        trace_burn_text_runtime(&format!(
            "generating token {} with prompt length {}",
            token_index + 1,
            all_token_ids.len()
        ));
        let input_token_ids = all_token_ids
            .iter()
            .copied()
            .map(|token_id| {
                u32::try_from(token_id)
                    .wrap_err_with(|| format!("Token id {token_id} exceeded u32 range"))
            })
            .collect::<eyre::Result<Vec<_>>>()?;
        let hidden_states =
            runtime.forward_hidden_states(runtime.embedding_hidden_states(&input_token_ids)?)?;
        trace_burn_text_runtime("decoder stack complete; scanning lm_head");
        let next_token_id = runtime.greedy_next_token(hidden_states)?;
        trace_burn_text_runtime(&format!("selected token id {next_token_id}"));
        generated_token_ids.push(next_token_id);
        all_token_ids.push(next_token_id);
        deadline.check(generated_token_ids.len(), options.max_new_tokens)?;
        if eos_token_id.is_some_and(|eos_token_id| eos_token_id == next_token_id) {
            break;
        }
    }

    let generated_text = decode_token_ids_with_tokenizer(&tokenizer, &generated_token_ids, false)?;
    Ok(BurnTextGenerationReport {
        generated_token_ids,
        generated_text,
    })
}

fn generate_with_incremental_burn_text_runtime_on_device<B: Backend>(
    artifacts: &LlmModelArtifacts,
    prompt_token_ids: &[u32],
    options: &BurnTextGenerationOptions,
    device: B::Device,
    backend_label: &str,
) -> eyre::Result<BurnTextGenerationReport> {
    llm_tracy_zone!("llm_burn_generate_incremental");
    #[cfg(feature = "extended_observability")]
    let _span = tracing::info_span!(
        "llm_burn_generate_incremental",
        backend = backend_label,
        prompt_token_count = prompt_token_ids.len(),
        max_new_tokens = options.max_new_tokens
    )
    .entered();
    trace_burn_text_runtime(&format!(
        "using {backend_label} backend for Burn text incremental runtime"
    ));
    ensure!(
        !prompt_token_ids.is_empty(),
        "Burn text incremental runtime expected at least one prompt token"
    );
    let runtime = Qwen35TextRuntime::<B>::load(artifacts, device)?;
    let tokenizer = tokenizers::Tokenizer::from_file(&artifacts.tokenizer_path).map_err(|error| {
        eyre::eyre!(
            "Failed to load tokenizer from {}: {}",
            artifacts.tokenizer_path.display(),
            error
        )
    })?;
    let eos_token_id = load_tokenizer_config_summary(&artifacts.tokenizer_config_path)?
        .eos_token
        .as_deref()
        .and_then(|token| tokenizer.token_to_id(token))
        .and_then(|token_id| usize::try_from(token_id).ok());

    let deadline = GenerationDeadline::new(options.generation_timeout);
    let mut decode_state = runtime.new_decode_state()?;
    let mut last_hidden = Vec::new();
    for (prompt_index, token_id) in prompt_token_ids.iter().enumerate() {
        deadline.check(0, options.max_new_tokens)?;
        trace_burn_text_runtime(&format!(
            "incremental processing prompt token {} of {}",
            prompt_index + 1,
            prompt_token_ids.len()
        ));
        last_hidden = runtime.forward_token_hidden(&mut decode_state, *token_id)?;
    }

    let mut generated_token_ids = Vec::new();
    for token_index in 0..options.max_new_tokens {
        deadline.check(generated_token_ids.len(), options.max_new_tokens)?;
        trace_burn_text_runtime(&format!(
            "incremental generating token {} after {} processed tokens",
            token_index + 1,
            decode_state.processed_token_count
        ));
        let next_token_id = runtime.greedy_next_token_from_hidden(&last_hidden)?;
        trace_burn_text_runtime(&format!(
            "incremental selected token id {next_token_id}"
        ));
        generated_token_ids.push(next_token_id);
        deadline.check(generated_token_ids.len(), options.max_new_tokens)?;
        if eos_token_id.is_some_and(|eos_token_id| eos_token_id == next_token_id) {
            break;
        }
        last_hidden = runtime.forward_token_hidden(
            &mut decode_state,
            u32::try_from(next_token_id)
                .wrap_err_with(|| format!("Token id {next_token_id} exceeded u32 range"))?,
        )?;
    }

    let generated_text = decode_token_ids_with_tokenizer(&tokenizer, &generated_token_ids, false)?;
    Ok(BurnTextGenerationReport {
        generated_token_ids,
        generated_text,
    })
}

fn generate_with_hybrid_burn_text_runtime_on_device<B: Backend>(
    artifacts: &LlmModelArtifacts,
    prompt_token_ids: &[u32],
    options: &BurnTextGenerationOptions,
    device: B::Device,
    backend_label: &str,
) -> eyre::Result<BurnTextGenerationReport> {
    llm_tracy_zone!("llm_burn_generate_hybrid");
    #[cfg(feature = "extended_observability")]
    let _span = tracing::info_span!(
        "llm_burn_generate_hybrid",
        backend = backend_label,
        prompt_token_count = prompt_token_ids.len(),
        max_new_tokens = options.max_new_tokens
    )
    .entered();
    trace_burn_text_runtime(&format!(
        "using {backend_label} backend for Burn text hybrid runtime"
    ));
    if options.max_new_tokens == 0 {
        return Ok(BurnTextGenerationReport {
            generated_token_ids: Vec::new(),
            generated_text: String::new(),
        });
    }
    if prompt_token_ids.is_empty() {
        return generate_with_burn_text_runtime_on_device::<B>(
            artifacts,
            prompt_token_ids,
            options,
            device,
            backend_label,
        );
    }

    let runtime = Qwen35TextRuntime::<B>::load(artifacts, device)?;
    let tokenizer = tokenizers::Tokenizer::from_file(&artifacts.tokenizer_path).map_err(|error| {
        eyre::eyre!(
            "Failed to load tokenizer from {}: {}",
            artifacts.tokenizer_path.display(),
            error
        )
    })?;
    let eos_token_id = load_tokenizer_config_summary(&artifacts.tokenizer_config_path)?
        .eos_token
        .as_deref()
        .and_then(|token| tokenizer.token_to_id(token))
        .and_then(|token_id| usize::try_from(token_id).ok());
    let deadline = GenerationDeadline::new(options.generation_timeout);

    deadline.check(0, options.max_new_tokens)?;
    trace_burn_text_runtime(&format!(
        "hybrid generating token 1 with prompt length {}",
        prompt_token_ids.len()
    ));
    let (hidden_states, mut decode_state) = runtime
        .forward_hidden_states_with_decode_state(runtime.embedding_hidden_states(prompt_token_ids)?)?;
    trace_burn_text_runtime("hybrid decoder stack complete; scanning lm_head");
    let first_token_id = runtime.greedy_next_token(hidden_states)?;
    trace_burn_text_runtime(&format!("hybrid selected token id {first_token_id}"));
    let mut generated_token_ids = vec![first_token_id];
    deadline.check(generated_token_ids.len(), options.max_new_tokens)?;
    if eos_token_id.is_some_and(|eos_token_id| eos_token_id == first_token_id)
        || options.max_new_tokens == 1
    {
        let generated_text =
            decode_token_ids_with_tokenizer(&tokenizer, &generated_token_ids, false)?;
        return Ok(BurnTextGenerationReport {
            generated_token_ids,
            generated_text,
        });
    }

    deadline.check(generated_token_ids.len(), options.max_new_tokens)?;
    trace_burn_text_runtime("hybrid handing off first generated token to cached decode state");
    let mut last_hidden = runtime.forward_token_hidden(
        &mut decode_state,
        u32::try_from(first_token_id)
            .wrap_err_with(|| format!("Token id {first_token_id} exceeded u32 range"))?,
    )?;

    for token_index in 1..options.max_new_tokens {
        deadline.check(generated_token_ids.len(), options.max_new_tokens)?;
        trace_burn_text_runtime(&format!(
            "hybrid generating token {} after {} processed tokens",
            token_index + 1,
            decode_state.processed_token_count
        ));
        let next_token_id = runtime.greedy_next_token_from_hidden(&last_hidden)?;
        trace_burn_text_runtime(&format!(
            "hybrid selected token id {next_token_id}"
        ));
        generated_token_ids.push(next_token_id);
        deadline.check(generated_token_ids.len(), options.max_new_tokens)?;
        if eos_token_id.is_some_and(|eos_token_id| eos_token_id == next_token_id) {
            break;
        }
        last_hidden = runtime.forward_token_hidden(
            &mut decode_state,
            u32::try_from(next_token_id)
                .wrap_err_with(|| format!("Token id {next_token_id} exceeded u32 range"))?,
        )?;
    }

    let generated_text = decode_token_ids_with_tokenizer(&tokenizer, &generated_token_ids, false)?;
    Ok(BurnTextGenerationReport {
        generated_token_ids,
        generated_text,
    })
}

fn decode_token_ids_with_tokenizer(
    tokenizer: &tokenizers::Tokenizer,
    token_ids: &[usize],
    skip_special_tokens: bool,
) -> eyre::Result<String> {
    let token_ids = token_ids
        .iter()
        .copied()
        .map(u32::try_from)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| eyre::eyre!("Generated token id exceeded u32 range"))?;
    tokenizer
        .decode(&token_ids, skip_special_tokens)
        .map_err(|error| eyre::eyre!("Failed to decode generated token ids: {}", error))
}

fn trace_burn_text_runtime(message: &str) {
    if std::env::var_os("TEAMY_STUDIO_LLM_TRACE").is_some() {
        eprintln!("[teamy-llm-trace] {message}");
    }
}

fn full_last_hidden_for_prompt<B: Backend>(
    runtime: &Qwen35TextRuntime<B>,
    prompt_token_ids: &[u32],
) -> eyre::Result<Vec<f32>> {
    ensure!(
        !prompt_token_ids.is_empty(),
        "Burn text hidden comparison expected at least one prompt token"
    );
    let hidden_states =
        runtime.forward_hidden_states(runtime.embedding_hidden_states(prompt_token_ids)?)?;
    last_hidden_from_hidden_states(hidden_states)
}

fn full_layer_outputs_for_prompt<B: Backend>(
    runtime: &Qwen35TextRuntime<B>,
    prompt_token_ids: &[u32],
) -> eyre::Result<Vec<Vec<f32>>> {
    ensure!(
        !prompt_token_ids.is_empty(),
        "Burn text layer diagnostics expected at least one prompt token"
    );
    let seq_len = prompt_token_ids.len();
    let mut hidden_states = runtime.embedding_hidden_states(prompt_token_ids)?;
    let position_embeddings = rotary_embeddings(seq_len, &runtime.manifest)
        .wrap_err("Failed to build rotary embeddings for layer diagnostics")?;
    let causal_mask = causal_mask::<B>(seq_len, &runtime.device);
    let mut outputs = Vec::with_capacity(runtime.manifest.num_hidden_layers);
    for layer_index in 0..runtime.manifest.num_hidden_layers {
        hidden_states = runtime.forward_decoder_layer(
            layer_index,
            hidden_states,
            &position_embeddings,
            &causal_mask,
        )?;
        outputs.push(last_hidden_from_hidden_states(hidden_states.clone())?);
    }
    Ok(outputs)
}

fn incremental_last_hidden_for_prompt<B: Backend>(
    runtime: &Qwen35TextRuntime<B>,
    prompt_token_ids: &[u32],
) -> eyre::Result<Vec<f32>> {
    ensure!(
        !prompt_token_ids.is_empty(),
        "Burn text incremental hidden comparison expected at least one prompt token"
    );
    let mut decode_state = runtime.new_decode_state()?;
    let mut last_hidden = Vec::new();
    for token_id in prompt_token_ids {
        last_hidden = runtime.forward_token_hidden(&mut decode_state, *token_id)?;
    }
    Ok(last_hidden)
}

fn incremental_layer_outputs_for_prompt<B: Backend>(
    runtime: &Qwen35TextRuntime<B>,
    prompt_token_ids: &[u32],
) -> eyre::Result<Vec<Vec<f32>>> {
    ensure!(
        !prompt_token_ids.is_empty(),
        "Burn text incremental layer diagnostics expected at least one prompt token"
    );
    let mut decode_state = runtime.new_decode_state()?;
    let mut last_outputs = Vec::new();
    for token_id in prompt_token_ids {
        last_outputs = runtime.forward_token_hidden_layer_outputs(&mut decode_state, *token_id)?;
    }
    Ok(last_outputs)
}

fn last_hidden_from_hidden_states<B: Backend>(hidden_states: Tensor<B, 3>) -> eyre::Result<Vec<f32>> {
    let [batch_size, seq_len, hidden_size] = hidden_states.dims();
    ensure!(batch_size == 1, "Qwen3.5 hidden comparison expected batch size 1");
    ensure!(seq_len > 0, "Qwen3.5 hidden comparison expected at least one timestep");
    tensor_to_vec_f32(&hidden_states)?
        .chunks_exact(hidden_size)
        .last()
        .map(ToOwned::to_owned)
        .ok_or_else(|| eyre::eyre!("Missing final hidden state during hidden comparison"))
}

fn compare_hidden_slices(
    left: &[f32],
    right: &[f32],
) -> eyre::Result<BurnTextHiddenDifferenceSummary> {
    ensure!(
        left.len() == right.len(),
        "Hidden comparison length mismatch: {} vs {}",
        left.len(),
        right.len()
    );
    if left.is_empty() {
        return Ok(BurnTextHiddenDifferenceSummary {
            max_abs_diff: 0.0,
            mean_abs_diff: 0.0,
        });
    }
    let mut max_abs_diff = 0.0_f32;
    let mut abs_diff_sum = 0.0_f64;
    for (left_value, right_value) in left.iter().zip(right.iter()) {
        let abs_diff = (left_value - right_value).abs();
        max_abs_diff = max_abs_diff.max(abs_diff);
        abs_diff_sum += f64::from(abs_diff);
    }
    Ok(BurnTextHiddenDifferenceSummary {
        max_abs_diff,
        mean_abs_diff: (abs_diff_sum / left.len() as f64) as f32,
    })
}

fn first_mismatch_index<T: PartialEq>(left: &[T], right: &[T]) -> Option<usize> {
    left.iter()
        .zip(right.iter())
        .position(|(left, right)| left != right)
        .or_else(|| (left.len() != right.len()).then_some(left.len().min(right.len())))
}

fn tensor_3d<B: Backend>(shape: [usize; 3], values: Vec<f32>, device: &B::Device) -> Tensor<B, 3> {
    Tensor::from_data(TensorData::new(values, shape), device)
}

#[allow(dead_code)]
fn tensor_4d<B: Backend>(shape: [usize; 4], values: Vec<f32>, device: &B::Device) -> Tensor<B, 4> {
    Tensor::from_data(TensorData::new(values, shape), device)
}

fn tensor_1d_from_bytes<B: Backend>(
    bytes: &[u8],
    dtype: &str,
    dim: usize,
    device: &B::Device,
) -> eyre::Result<Tensor<B, 1>> {
    match dtype.trim().to_ascii_lowercase().as_str() {
        "float16" | "f16" => {
            ensure!(
                bytes.len().is_multiple_of(2),
                "float16 tensor bytes length {} was not divisible by 2",
                bytes.len()
            );
            let words = bytes
                .chunks_exact(2)
                .map(|chunk| u16::from_ne_bytes([chunk[0], chunk[1]]))
                .collect::<Vec<_>>();
            let values: &[f16] = words.reinterpret_cast();
            Ok(Tensor::from_data(TensorData::new(values.to_vec(), [dim]), device))
        }
        "bfloat16" | "bf16" => {
            ensure!(
                bytes.len().is_multiple_of(2),
                "bfloat16 tensor bytes length {} was not divisible by 2",
                bytes.len()
            );
            let words = bytes
                .chunks_exact(2)
                .map(|chunk| u16::from_ne_bytes([chunk[0], chunk[1]]))
                .collect::<Vec<_>>();
            let values: &[bf16] = words.reinterpret_cast();
            Ok(Tensor::from_data(TensorData::new(values.to_vec(), [dim]), device))
        }
        "float32" | "f32" => {
            ensure!(
                bytes.len().is_multiple_of(4),
                "float32 tensor bytes length {} was not divisible by 4",
                bytes.len()
            );
            let values = bytes
                .chunks_exact(4)
                .map(|chunk| f32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect::<Vec<_>>();
            Ok(Tensor::from_data(TensorData::new(values.to_vec(), [dim]), device))
        }
        other => bail!("Unsupported Burn text tensor dtype `{other}`"),
    }
}

fn tensor_2d_from_bytes<B: Backend>(
    bytes: &[u8],
    dtype: &str,
    shape: [usize; 2],
    device: &B::Device,
) -> eyre::Result<Tensor<B, 2>> {
    match dtype.trim().to_ascii_lowercase().as_str() {
        "float16" | "f16" => {
            ensure!(
                bytes.len().is_multiple_of(2),
                "float16 tensor bytes length {} was not divisible by 2",
                bytes.len()
            );
            let words = bytes
                .chunks_exact(2)
                .map(|chunk| u16::from_ne_bytes([chunk[0], chunk[1]]))
                .collect::<Vec<_>>();
            let values: &[f16] = words.reinterpret_cast();
            Ok(Tensor::from_data(TensorData::new(values.to_vec(), shape), device))
        }
        "bfloat16" | "bf16" => {
            ensure!(
                bytes.len().is_multiple_of(2),
                "bfloat16 tensor bytes length {} was not divisible by 2",
                bytes.len()
            );
            let words = bytes
                .chunks_exact(2)
                .map(|chunk| u16::from_ne_bytes([chunk[0], chunk[1]]))
                .collect::<Vec<_>>();
            let values: &[bf16] = words.reinterpret_cast();
            Ok(Tensor::from_data(TensorData::new(values.to_vec(), shape), device))
        }
        "float32" | "f32" => {
            ensure!(
                bytes.len().is_multiple_of(4),
                "float32 tensor bytes length {} was not divisible by 4",
                bytes.len()
            );
            let values = bytes
                .chunks_exact(4)
                .map(|chunk| f32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect::<Vec<_>>();
            Ok(Tensor::from_data(TensorData::new(values.to_vec(), shape), device))
        }
        other => bail!("Unsupported Burn text tensor dtype `{other}`"),
    }
}

fn linear_forward_3d<B: Backend>(
    input: &Tensor<B, 3>,
    weight: Tensor<B, 2>,
    bias: Option<Tensor<B, 1>>,
) -> Tensor<B, 3> {
    llm_tracy_zone!("llm_burn_linear_forward_3d");
    #[cfg(feature = "extended_observability")]
    let _span = tracing::debug_span!("llm_burn_linear_forward_3d").entered();
    let output = input.clone().matmul(weight.transpose().unsqueeze::<3>());
    if let Some(bias) = bias {
        output + bias.unsqueeze::<3>()
    } else {
        output
    }
}

fn shape_array<const D: usize>(shape: &[usize], tensor_name: &str) -> eyre::Result<[usize; D]> {
    ensure!(
        shape.len() == D,
        "Expected tensor `{tensor_name}` to have rank {D}, but found shape {:?}",
        shape
    );
    let mut out = [0_usize; D];
    out.copy_from_slice(shape);
    Ok(out)
}

fn tensor_to_vec_f32<B: Backend, const D: usize>(tensor: &Tensor<B, D>) -> eyre::Result<Vec<f32>> {
    llm_tracy_zone!("llm_burn_tensor_to_vec_f32");
    #[cfg(feature = "extended_observability")]
    let _span = tracing::debug_span!("llm_burn_tensor_to_vec_f32", rank = D).entered();
    tensor
        .to_data()
        .convert::<f32>()
        .to_vec::<f32>()
        .map_err(|error| eyre::eyre!("Failed to extract Burn tensor values: {:?}", error))
}

fn bytes_per_element(dtype: &str) -> eyre::Result<usize> {
    match dtype.trim().to_ascii_lowercase().as_str() {
        "float16" | "f16" | "bfloat16" | "bf16" => Ok(2),
        "float32" | "f32" => Ok(4),
        other => bail!("Unsupported Burn text tensor dtype `{other}`"),
    }
}

fn decode_bytes_into_f32(bytes: &[u8], dtype: &str, out: &mut Vec<f32>) -> eyre::Result<()> {
    match dtype.trim().to_ascii_lowercase().as_str() {
        "float32" | "f32" => {
            ensure!(
                bytes.len().is_multiple_of(4),
                "float32 tensor bytes length {} was not divisible by 4",
                bytes.len()
            );
            for chunk in bytes.chunks_exact(4) {
                out.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
            }
        }
        "float16" | "f16" => {
            ensure!(
                bytes.len().is_multiple_of(2),
                "float16 tensor bytes length {} was not divisible by 2",
                bytes.len()
            );
            decode_f16_bytes_into_f32(bytes, out)?;
        }
        "bfloat16" | "bf16" => {
            ensure!(
                bytes.len().is_multiple_of(2),
                "bfloat16 tensor bytes length {} was not divisible by 2",
                bytes.len()
            );
            decode_bf16_bytes_into_f32(bytes, out)?;
        }
        other => bail!("Unsupported Burn text tensor dtype `{other}`"),
    }
    Ok(())
}

fn decode_f16_bytes_into_f32(bytes: &[u8], out: &mut Vec<f32>) -> eyre::Result<()> {
    if cfg!(target_endian = "little") {
        // Reuse the packed little-endian bytes directly when the slice alignment permits it.
        let (prefix, words, suffix) = unsafe { bytes.align_to::<u16>() };
        if prefix.is_empty() && suffix.is_empty() {
            let values: &[f16] = words.reinterpret_cast();
            let start = out.len();
            out.resize(
                start
                    .checked_add(values.len())
                    .ok_or_else(|| eyre::eyre!("decoded value count overflow for float16"))?,
                0.0,
            );
            values.convert_to_f32_slice(&mut out[start..]);
            return Ok(());
        }
    }

    let mut words = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        words.push(u16::from_le_bytes([chunk[0], chunk[1]]));
    }
    let values: &[f16] = words.reinterpret_cast();
    let start = out.len();
    out.resize(
        start
            .checked_add(values.len())
            .ok_or_else(|| eyre::eyre!("decoded value count overflow for float16"))?,
        0.0,
    );
    values.convert_to_f32_slice(&mut out[start..]);
    Ok(())
}

fn decode_bf16_bytes_into_f32(bytes: &[u8], out: &mut Vec<f32>) -> eyre::Result<()> {
    if cfg!(target_endian = "little") {
        let (prefix, words, suffix) = unsafe { bytes.align_to::<u16>() };
        if prefix.is_empty() && suffix.is_empty() {
            let values: &[bf16] = words.reinterpret_cast();
            let start = out.len();
            out.resize(
                start
                    .checked_add(values.len())
                    .ok_or_else(|| eyre::eyre!("decoded value count overflow for bfloat16"))?,
                0.0,
            );
            values.convert_to_f32_slice(&mut out[start..]);
            return Ok(());
        }
    }

    let mut words = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        words.push(u16::from_le_bytes([chunk[0], chunk[1]]));
    }
    let values: &[bf16] = words.reinterpret_cast();
    let start = out.len();
    out.resize(
        start
            .checked_add(values.len())
            .ok_or_else(|| eyre::eyre!("decoded value count overflow for bfloat16"))?,
        0.0,
    );
    values.convert_to_f32_slice(&mut out[start..]);
    Ok(())
}

#[cfg(test)]
fn f16_bits_to_f32(bits: u16) -> f32 {
    let sign = u32::from(bits & 0x8000) << 16;
    let exponent = (bits & 0x7C00) >> 10;
    let fraction = u32::from(bits & 0x03FF);
    let f32_bits = if exponent == 0 {
        if fraction == 0 {
            sign
        } else {
            let mut exponent = -14_i32;
            let mut fraction = fraction;
            while (fraction & 0x0400) == 0 {
                fraction <<= 1;
                exponent -= 1;
            }
            fraction &= 0x03FF;
            let exponent_bits = u32::try_from(exponent + 127).unwrap_or(0) << 23;
            sign | exponent_bits | (fraction << 13)
        }
    } else if exponent == 0x1F {
        sign | 0x7F80_0000 | (fraction << 13)
    } else {
        let exponent_bits = u32::from(exponent + 112) << 23;
        sign | exponent_bits | (fraction << 13)
    };
    f32::from_bits(f32_bits)
}

fn dot_product(left: &[f32], right: &[f32]) -> f32 {
    left.iter()
        .copied()
        .zip(right.iter().copied())
        .map(|(left, right)| left * right)
        .sum()
}

#[allow(dead_code)]
fn add_vectors(left: &[f32], right: &[f32]) -> eyre::Result<Vec<f32>> {
    ensure!(
        left.len() == right.len(),
        "Vector add length mismatch: {} vs {}",
        left.len(),
        right.len()
    );
    Ok(left
        .iter()
        .copied()
        .zip(right.iter().copied())
        .map(|(left, right)| left + right)
        .collect())
}

#[allow(dead_code)]
fn softmax_scalars(values: &[f32]) -> Vec<f32> {
    if values.is_empty() {
        return Vec::new();
    }
    let max_value = values
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    let mut exp_values = values
        .iter()
        .copied()
        .map(|value| (value - max_value).exp())
        .collect::<Vec<_>>();
    let sum = exp_values.iter().copied().sum::<f32>();
    if sum <= 0.0 {
        return vec![0.0; values.len()];
    }
    for value in &mut exp_values {
        *value /= sum;
    }
    exp_values
}

fn qwen_rms_norm(
    values: &[f32],
    batch_size: usize,
    seq_len: usize,
    hidden_size: usize,
    weight: &[f32],
    eps: f32,
) -> eyre::Result<Vec<f32>> {
    llm_tracy_zone!("llm_burn_qwen_rms_norm");
    #[cfg(feature = "extended_observability")]
    let _span = tracing::debug_span!(
        "llm_burn_qwen_rms_norm",
        batch_size,
        seq_len,
        hidden_size
    )
    .entered();
    ensure!(
        weight.len() == hidden_size,
        "Qwen RMSNorm expected {} weights but found {}",
        hidden_size,
        weight.len()
    );
    let chunk_len = hidden_size;
    ensure!(
        values.len() == batch_size * seq_len * chunk_len,
        "Qwen RMSNorm input length {} did not match {}x{}x{}",
        values.len(),
        batch_size,
        seq_len,
        chunk_len
    );
    let mut output = Vec::with_capacity(values.len());
    for chunk in values.chunks_exact(chunk_len) {
        let variance = chunk.iter().copied().map(|value| value * value).sum::<f32>()
            / chunk_len as f32;
        let scale = (variance + eps).sqrt().recip();
        for (index, value) in chunk.iter().copied().enumerate() {
            output.push((value * scale) * (1.0 + weight[index]));
        }
    }
    Ok(output)
}

fn qwen_rms_norm_no_center(
    values: &[f32],
    row_count: usize,
    head_count: usize,
    head_dim: usize,
    weight: &[f32],
    eps: f32,
) -> eyre::Result<Vec<f32>> {
    llm_tracy_zone!("llm_burn_qwen_rms_norm_no_center");
    #[cfg(feature = "extended_observability")]
    let _span = tracing::debug_span!(
        "llm_burn_qwen_rms_norm_no_center",
        row_count,
        head_count,
        head_dim
    )
    .entered();
    ensure!(
        weight.len() == head_dim,
        "Qwen head RMSNorm expected {} weights but found {}",
        head_dim,
        weight.len()
    );
    ensure!(
        values.len() == row_count * head_count * head_dim,
        "Qwen head RMSNorm input length {} did not match {}x{}x{}",
        values.len(),
        row_count,
        head_count,
        head_dim
    );
    let mut output = Vec::with_capacity(values.len());
    for chunk in values.chunks_exact(head_dim) {
        let variance = chunk.iter().copied().map(|value| value * value).sum::<f32>()
            / head_dim as f32;
        let scale = (variance + eps).sqrt().recip();
        for (index, value) in chunk.iter().copied().enumerate() {
            output.push(value * scale * (1.0 + weight[index]));
        }
    }
    Ok(output)
}

fn qwen_rms_norm_gated(
    values: &[f32],
    seq_len: usize,
    head_count: usize,
    head_dim: usize,
    weight: &[f32],
    eps: f32,
    gate: &[f32],
) -> eyre::Result<Vec<f32>> {
    llm_tracy_zone!("llm_burn_qwen_rms_norm_gated");
    #[cfg(feature = "extended_observability")]
    let _span = tracing::debug_span!(
        "llm_burn_qwen_rms_norm_gated",
        seq_len,
        head_count,
        head_dim
    )
    .entered();
    ensure!(
        weight.len() == head_dim,
        "Qwen gated RMSNorm expected {} weights but found {}",
        head_dim,
        weight.len()
    );
    ensure!(
        values.len() == seq_len * head_count * head_dim,
        "Qwen gated RMSNorm input length {} did not match {}x{}x{}",
        values.len(),
        seq_len,
        head_count,
        head_dim
    );
    ensure!(
        gate.len() == values.len(),
        "Qwen gated RMSNorm gate length {} did not match hidden length {}",
        gate.len(),
        values.len()
    );
    let mut output = Vec::with_capacity(values.len());
    for (value_chunk, gate_chunk) in values
        .chunks_exact(head_dim)
        .zip(gate.chunks_exact(head_dim))
    {
        let variance = value_chunk
            .iter()
            .copied()
            .map(|value| value * value)
            .sum::<f32>()
            / head_dim as f32;
        let scale = (variance + eps).sqrt().recip();
        for index in 0..head_dim {
            output.push(value_chunk[index] * scale * weight[index] * silu_scalar(gate_chunk[index]));
        }
    }
    Ok(output)
}

#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
struct RotaryEmbeddings {
    cos: Vec<f32>,
    sin: Vec<f32>,
    rotary_dim: usize,
}

#[allow(dead_code)]
fn rotary_embeddings(seq_len: usize, manifest: &BurnTextManifest) -> eyre::Result<RotaryEmbeddings> {
    let rotary_dim = (manifest.head_dim as f64 * manifest.partial_rotary_factor).round() as usize;
    ensure!(
        rotary_dim > 0 && rotary_dim <= manifest.head_dim && rotary_dim.is_multiple_of(2),
        "Qwen3.5 rotary dim {} was invalid for head dim {} and partial rotary factor {}",
        rotary_dim,
        manifest.head_dim,
        manifest.partial_rotary_factor
    );
    let half_dim = rotary_dim / 2;
    let rope_theta = manifest.rope_theta;
    let mut inv_freq = Vec::with_capacity(half_dim);
    for index in 0..half_dim {
        let numerator = (index * 2) as f64;
        inv_freq.push((1.0 / rope_theta.powf(numerator / rotary_dim as f64)) as f32);
    }
    let mut cos = Vec::with_capacity(seq_len * rotary_dim);
    let mut sin = Vec::with_capacity(seq_len * rotary_dim);
    for position in 0..seq_len {
        let mut freqs = Vec::with_capacity(half_dim);
        for inv_freq in &inv_freq {
            freqs.push(position as f32 * *inv_freq);
        }
        for value in &freqs {
            cos.push(value.cos());
        }
        for value in &freqs {
            cos.push(value.cos());
        }
        for value in &freqs {
            sin.push(value.sin());
        }
        for value in &freqs {
            sin.push(value.sin());
        }
    }
    Ok(RotaryEmbeddings {
        cos,
        sin,
        rotary_dim,
    })
}

#[allow(dead_code)]
fn apply_rotary_embeddings(
    query: &[f32],
    key: &[f32],
    embeddings: &RotaryEmbeddings,
    manifest: &BurnTextManifest,
    seq_len: usize,
) -> eyre::Result<(Vec<f32>, Vec<f32>)> {
    llm_tracy_zone!("llm_burn_apply_rotary_embeddings");
    #[cfg(feature = "extended_observability")]
    let _span = tracing::debug_span!(
        "llm_burn_apply_rotary_embeddings",
        seq_len,
        attention_heads = manifest.num_attention_heads,
        kv_heads = manifest.num_key_value_heads,
        head_dim = manifest.head_dim
    )
    .entered();
    let query_expected = seq_len
        .checked_mul(manifest.num_attention_heads)
        .and_then(|value| value.checked_mul(manifest.head_dim))
        .ok_or_else(|| eyre::eyre!("query shape overflow during rotary application"))?;
    let key_expected = seq_len
        .checked_mul(manifest.num_key_value_heads)
        .and_then(|value| value.checked_mul(manifest.head_dim))
        .ok_or_else(|| eyre::eyre!("key shape overflow during rotary application"))?;
    ensure!(
        query.len() == query_expected,
        "Rotary query length {} did not match {}",
        query.len(),
        query_expected
    );
    ensure!(
        key.len() == key_expected,
        "Rotary key length {} did not match {}",
        key.len(),
        key_expected
    );
    let mut query_out = query.to_vec();
    let mut key_out = key.to_vec();
    let rotary_dim = embeddings.rotary_dim;
    let half_dim = rotary_dim / 2;
    for position in 0..seq_len {
        let cos = &embeddings.cos[position * rotary_dim..(position + 1) * rotary_dim];
        let sin = &embeddings.sin[position * rotary_dim..(position + 1) * rotary_dim];
        for head in 0..manifest.num_attention_heads {
            let base = (position * manifest.num_attention_heads + head) * manifest.head_dim;
            apply_rotary_slice(&mut query_out[base..base + manifest.head_dim], cos, sin, half_dim);
        }
        for head in 0..manifest.num_key_value_heads {
            let base = (position * manifest.num_key_value_heads + head) * manifest.head_dim;
            apply_rotary_slice(&mut key_out[base..base + manifest.head_dim], cos, sin, half_dim);
        }
    }
    Ok((query_out, key_out))
}

#[allow(dead_code)]
fn apply_rotary_embedding_single_position(
    values: &mut [f32],
    position: usize,
    manifest: &BurnTextManifest,
    head_count: usize,
) -> eyre::Result<()> {
    let rotary_dim = (manifest.head_dim as f64 * manifest.partial_rotary_factor).round() as usize;
    ensure!(
        rotary_dim > 0 && rotary_dim <= manifest.head_dim && rotary_dim.is_multiple_of(2),
        "Qwen3.5 rotary dim {} was invalid for head dim {} and partial rotary factor {}",
        rotary_dim,
        manifest.head_dim,
        manifest.partial_rotary_factor
    );
    ensure!(
        values.len() == head_count * manifest.head_dim,
        "Single-position rotary values length {} did not match {}x{}",
        values.len(),
        head_count,
        manifest.head_dim
    );
    let half_dim = rotary_dim / 2;
    let rope_theta = manifest.rope_theta;
    let mut cos = Vec::with_capacity(rotary_dim);
    let mut sin = Vec::with_capacity(rotary_dim);
    for index in 0..half_dim {
        let numerator = (index * 2) as f64;
        let inv_freq = 1.0 / rope_theta.powf(numerator / rotary_dim as f64);
        let angle = position as f32 * inv_freq as f32;
        cos.push(angle.cos());
    }
    for index in 0..half_dim {
        cos.push(cos[index]);
    }
    for index in 0..half_dim {
        let numerator = (index * 2) as f64;
        let inv_freq = 1.0 / rope_theta.powf(numerator / rotary_dim as f64);
        let angle = position as f32 * inv_freq as f32;
        sin.push(angle.sin());
    }
    for index in 0..half_dim {
        sin.push(sin[index]);
    }
    for head in 0..head_count {
        let base = head * manifest.head_dim;
        apply_rotary_slice(&mut values[base..base + manifest.head_dim], &cos, &sin, half_dim);
    }
    Ok(())
}

fn apply_rotary_slice(values: &mut [f32], cos: &[f32], sin: &[f32], half_dim: usize) {
    let rotary_dim = half_dim * 2;
    let original = values[..rotary_dim].to_vec();
    for index in 0..half_dim {
        let left = original[index];
        let right = original[index + half_dim];
        values[index] = left * cos[index] + (-right) * sin[index];
        values[index + half_dim] = right * cos[index + half_dim] + left * sin[index + half_dim];
    }
}

fn transpose_seq_head_layout(
    values: &[f32],
    seq_len: usize,
    num_heads: usize,
    head_dim: usize,
) -> eyre::Result<Vec<f32>> {
    ensure!(
        values.len() == seq_len * num_heads * head_dim,
        "Seq/head transpose expected {} values but found {}",
        seq_len * num_heads * head_dim,
        values.len()
    );
    let mut output = vec![0.0_f32; values.len()];
    for position in 0..seq_len {
        for head in 0..num_heads {
            let source_base = (position * num_heads + head) * head_dim;
            let target_base = (head * seq_len + position) * head_dim;
            output[target_base..target_base + head_dim]
                .copy_from_slice(&values[source_base..source_base + head_dim]);
        }
    }
    Ok(output)
}

fn repeat_key_value_heads(
    values: &[f32],
    seq_len: usize,
    num_heads: usize,
    repeat_factor: usize,
    head_dim: usize,
) -> Vec<f32> {
    let mut output = Vec::with_capacity(seq_len * num_heads * repeat_factor * head_dim);
    for position in 0..seq_len {
        for head in 0..num_heads {
            let start = (position * num_heads + head) * head_dim;
            let end = start + head_dim;
            for _ in 0..repeat_factor {
                output.extend_from_slice(&values[start..end]);
            }
        }
    }
    output
}

fn repeat_linear_attention_heads(
    values: &[f32],
    seq_len: usize,
    num_heads: usize,
    repeat_factor: usize,
    head_dim: usize,
) -> Vec<f32> {
    repeat_key_value_heads(values, seq_len, num_heads, repeat_factor, head_dim)
}

fn l2_norm_heads(
    values: &[f32],
    seq_len: usize,
    num_heads: usize,
    head_dim: usize,
) -> eyre::Result<Vec<f32>> {
    llm_tracy_zone!("llm_burn_l2_norm_heads");
    #[cfg(feature = "extended_observability")]
    let _span = tracing::debug_span!("llm_burn_l2_norm_heads", seq_len, num_heads, head_dim).entered();
    ensure!(
        values.len() == seq_len * num_heads * head_dim,
        "L2 head norm input length {} did not match {}x{}x{}",
        values.len(),
        seq_len,
        num_heads,
        head_dim
    );
    let mut output = Vec::with_capacity(values.len());
    for chunk in values.chunks_exact(head_dim) {
        let norm = chunk.iter().copied().map(|value| value * value).sum::<f32>() + 1e-6;
        let inv_norm = norm.sqrt().recip();
        for value in chunk {
            output.push(*value * inv_norm);
        }
    }
    Ok(output)
}

#[expect(
    clippy::too_many_arguments,
    reason = "the recurrent gated-delta rule keeps the tensor contract explicit to mirror the upstream Qwen3.5 recurrence"
)]
#[allow(dead_code)]
fn recurrent_gated_delta_rule(
    query: &[f32],
    key: &[f32],
    value: &[f32],
    g: &[f32],
    beta: &[f32],
    seq_len: usize,
    num_heads: usize,
    key_dim: usize,
    value_dim: usize,
) -> eyre::Result<Vec<f32>> {
    llm_tracy_zone!("llm_burn_recurrent_gated_delta_rule");
    #[cfg(feature = "extended_observability")]
    let _span = tracing::debug_span!(
        "llm_burn_recurrent_gated_delta_rule",
        seq_len,
        num_heads,
        key_dim,
        value_dim
    )
    .entered();
    ensure!(
        query.len() == seq_len * num_heads * key_dim,
        "Gated-delta query length {} did not match {}x{}x{}",
        query.len(),
        seq_len,
        num_heads,
        key_dim
    );
    ensure!(
        key.len() == query.len(),
        "Gated-delta key length {} did not match query length {}",
        key.len(),
        query.len()
    );
    ensure!(
        value.len() == seq_len * num_heads * value_dim,
        "Gated-delta value length {} did not match {}x{}x{}",
        value.len(),
        seq_len,
        num_heads,
        value_dim
    );
    ensure!(
        g.len() == seq_len * num_heads,
        "Gated-delta g length {} did not match {}x{}",
        g.len(),
        seq_len,
        num_heads
    );
    ensure!(
        beta.len() == seq_len * num_heads,
        "Gated-delta beta length {} did not match {}x{}",
        beta.len(),
        seq_len,
        num_heads
    );
    let scale = (key_dim as f32).sqrt().recip();
    let mut state = vec![0_f32; num_heads * key_dim * value_dim];
    let mut output = vec![0_f32; seq_len * num_heads * value_dim];
    for position in 0..seq_len {
        for head in 0..num_heads {
            let state_base = head * key_dim * value_dim;
            let q_base = (position * num_heads + head) * key_dim;
            let v_base = (position * num_heads + head) * value_dim;
            let decay = g[position * num_heads + head].exp();
            let beta_t = beta[position * num_heads + head];
            for state_index in 0..key_dim * value_dim {
                state[state_base + state_index] *= decay;
            }

            let mut kv_mem = vec![0_f32; value_dim];
            for key_index in 0..key_dim {
                let key_value = key[q_base + key_index];
                let state_row_base = state_base + key_index * value_dim;
                for value_index in 0..value_dim {
                    kv_mem[value_index] += state[state_row_base + value_index] * key_value;
                }
            }

            let mut delta = vec![0_f32; value_dim];
            for value_index in 0..value_dim {
                delta[value_index] = (value[v_base + value_index] - kv_mem[value_index]) * beta_t;
            }
            for key_index in 0..key_dim {
                let key_value = key[q_base + key_index];
                let state_row_base = state_base + key_index * value_dim;
                for value_index in 0..value_dim {
                    state[state_row_base + value_index] += key_value * delta[value_index];
                }
            }

            let output_base = (position * num_heads + head) * value_dim;
            for value_index in 0..value_dim {
                let mut sum = 0_f32;
                for key_index in 0..key_dim {
                    sum += state[state_base + key_index * value_dim + value_index]
                        * (query[q_base + key_index] * scale);
                }
                output[output_base + value_index] = sum;
            }
        }
    }
    Ok(output)
}

#[allow(dead_code)]
#[expect(
    clippy::too_many_arguments,
    reason = "the recurrent gated-delta prefill capture keeps the tensor contract explicit to mirror the upstream Qwen3.5 recurrence"
)]
fn recurrent_gated_delta_rule_with_final_state(
    query: &[f32],
    key: &[f32],
    value: &[f32],
    g: &[f32],
    beta: &[f32],
    seq_len: usize,
    num_heads: usize,
    key_dim: usize,
    value_dim: usize,
) -> eyre::Result<(Vec<f32>, Vec<f32>)> {
    llm_tracy_zone!("llm_burn_recurrent_gated_delta_rule_with_final_state");
    ensure!(
        query.len() == seq_len * num_heads * key_dim,
        "Gated-delta query length {} did not match {}x{}x{}",
        query.len(),
        seq_len,
        num_heads,
        key_dim
    );
    ensure!(
        key.len() == query.len(),
        "Gated-delta key length {} did not match query length {}",
        key.len(),
        query.len()
    );
    ensure!(
        value.len() == seq_len * num_heads * value_dim,
        "Gated-delta value length {} did not match {}x{}x{}",
        value.len(),
        seq_len,
        num_heads,
        value_dim
    );
    ensure!(
        g.len() == seq_len * num_heads,
        "Gated-delta g length {} did not match {}x{}",
        g.len(),
        seq_len,
        num_heads
    );
    ensure!(
        beta.len() == seq_len * num_heads,
        "Gated-delta beta length {} did not match {}x{}",
        beta.len(),
        seq_len,
        num_heads
    );
    let scale = (key_dim as f32).sqrt().recip();
    let mut state = vec![0_f32; num_heads * key_dim * value_dim];
    let mut output = vec![0_f32; seq_len * num_heads * value_dim];
    for position in 0..seq_len {
        for head in 0..num_heads {
            let state_base = head * key_dim * value_dim;
            let q_base = (position * num_heads + head) * key_dim;
            let v_base = (position * num_heads + head) * value_dim;
            let decay = g[position * num_heads + head].exp();
            let beta_t = beta[position * num_heads + head];
            for state_index in 0..key_dim * value_dim {
                state[state_base + state_index] *= decay;
            }

            let mut kv_mem = vec![0_f32; value_dim];
            for key_index in 0..key_dim {
                let key_value = key[q_base + key_index];
                let state_row_base = state_base + key_index * value_dim;
                for value_index in 0..value_dim {
                    kv_mem[value_index] += state[state_row_base + value_index] * key_value;
                }
            }

            let mut delta = vec![0_f32; value_dim];
            for value_index in 0..value_dim {
                delta[value_index] = (value[v_base + value_index] - kv_mem[value_index]) * beta_t;
            }
            for key_index in 0..key_dim {
                let key_value = key[q_base + key_index];
                let state_row_base = state_base + key_index * value_dim;
                for value_index in 0..value_dim {
                    state[state_row_base + value_index] += key_value * delta[value_index];
                }
            }

            let output_base = (position * num_heads + head) * value_dim;
            for value_index in 0..value_dim {
                let mut sum = 0_f32;
                for key_index in 0..key_dim {
                    sum += state[state_base + key_index * value_dim + value_index]
                        * (query[q_base + key_index] * scale);
                }
                output[output_base + value_index] = sum;
            }
        }
    }
    Ok((output, state))
}

#[allow(dead_code)]
#[expect(
    clippy::too_many_arguments,
    reason = "the incremental gated-delta step keeps the tensor contract explicit to mirror the upstream recurrence"
)]
fn recurrent_gated_delta_step(
    query: &[f32],
    key: &[f32],
    value: &[f32],
    g: &[f32],
    beta: &[f32],
    state: &mut [f32],
    num_heads: usize,
    key_dim: usize,
    value_dim: usize,
) -> eyre::Result<Vec<f32>> {
    ensure!(
        query.len() == num_heads * key_dim,
        "Gated-delta step query length {} did not match {}x{}",
        query.len(),
        num_heads,
        key_dim
    );
    ensure!(
        key.len() == query.len(),
        "Gated-delta step key length {} did not match query length {}",
        key.len(),
        query.len()
    );
    ensure!(
        value.len() == num_heads * value_dim,
        "Gated-delta step value length {} did not match {}x{}",
        value.len(),
        num_heads,
        value_dim
    );
    ensure!(
        g.len() == num_heads,
        "Gated-delta step g length {} did not match {} heads",
        g.len(),
        num_heads
    );
    ensure!(
        beta.len() == num_heads,
        "Gated-delta step beta length {} did not match {} heads",
        beta.len(),
        num_heads
    );
    ensure!(
        state.len() == num_heads * key_dim * value_dim,
        "Gated-delta step state length {} did not match {}x{}x{}",
        state.len(),
        num_heads,
        key_dim,
        value_dim
    );
    let scale = (key_dim as f32).sqrt().recip();
    let mut output = vec![0.0_f32; num_heads * value_dim];
    for head in 0..num_heads {
        let state_base = head * key_dim * value_dim;
        let q_base = head * key_dim;
        let v_base = head * value_dim;
        let decay = g[head].exp();
        let beta_t = beta[head];
        for state_index in 0..key_dim * value_dim {
            state[state_base + state_index] *= decay;
        }
        let mut kv_mem = vec![0.0_f32; value_dim];
        for key_index in 0..key_dim {
            let key_value = key[q_base + key_index];
            let state_row_base = state_base + key_index * value_dim;
            for value_index in 0..value_dim {
                kv_mem[value_index] += state[state_row_base + value_index] * key_value;
            }
        }
        let mut delta = vec![0.0_f32; value_dim];
        for value_index in 0..value_dim {
            delta[value_index] = (value[v_base + value_index] - kv_mem[value_index]) * beta_t;
        }
        for key_index in 0..key_dim {
            let key_value = key[q_base + key_index];
            let state_row_base = state_base + key_index * value_dim;
            for value_index in 0..value_dim {
                state[state_row_base + value_index] += key_value * delta[value_index];
            }
        }
        for value_index in 0..value_dim {
            let mut sum = 0.0_f32;
            for key_index in 0..key_dim {
                sum +=
                    state[state_base + key_index * value_dim + value_index]
                        * (query[q_base + key_index] * scale);
            }
            output[v_base + value_index] = sum;
        }
    }
    Ok(output)
}

#[allow(dead_code)]
fn depthwise_causal_conv1d_silu(
    values: &[f32],
    batch_size: usize,
    seq_len: usize,
    channels: usize,
    kernel_size: usize,
    weight_values: &[f32],
    weight_shape: &[usize],
) -> eyre::Result<Vec<f32>> {
    llm_tracy_zone!("llm_burn_depthwise_causal_conv1d_silu");
    #[cfg(feature = "extended_observability")]
    let _span = tracing::debug_span!(
        "llm_burn_depthwise_causal_conv1d_silu",
        batch_size,
        seq_len,
        channels,
        kernel_size
    )
    .entered();
    ensure!(
        batch_size == 1,
        "Depthwise causal conv currently expects batch size 1, but found {batch_size}"
    );
    let [weight_channels, grouped_channels, kernel] = shape_array::<3>(weight_shape, "conv1d.weight")?;
    ensure!(
        weight_channels == channels && grouped_channels == 1 && kernel == kernel_size,
        "Depthwise causal conv expected weight shape [{channels}, 1, {kernel_size}] but found {:?}",
        weight_shape
    );
    ensure!(
        values.len() == batch_size * seq_len * channels,
        "Depthwise causal conv input length {} did not match {}x{}x{}",
        values.len(),
        batch_size,
        seq_len,
        channels
    );
    let mut output = vec![0_f32; values.len()];
    for position in 0..seq_len {
        for channel in 0..channels {
            let mut sum = 0_f32;
            for kernel_index in 0..kernel_size {
                if kernel_index > position {
                    continue;
                }
                let input_position = position - kernel_index;
                let input_index = input_position * channels + channel;
                let weight_index = (channel * kernel_size) + (kernel_size - 1 - kernel_index);
                sum += values[input_index] * weight_values[weight_index];
            }
            output[position * channels + channel] = silu_scalar(sum);
        }
    }
    Ok(output)
}

#[allow(dead_code)]
fn depthwise_causal_conv1d_silu_step(
    current: &[f32],
    history: &VecDeque<Vec<f32>>,
    channels: usize,
    kernel_size: usize,
    weight_values: &[f32],
    weight_shape: &[usize],
) -> eyre::Result<Vec<f32>> {
    ensure!(
        current.len() == channels,
        "Depthwise causal conv step input length {} did not match channel count {}",
        current.len(),
        channels
    );
    ensure!(
        weight_shape == [channels, 1, kernel_size],
        "Depthwise conv expected weight shape [{channels}, 1, {kernel_size}], found {:?}",
        weight_shape
    );
    ensure!(
        weight_values.len() == channels * kernel_size,
        "Depthwise conv expected {} weight values but found {}",
        channels * kernel_size,
        weight_values.len()
    );
    let mut output = vec![0.0_f32; channels];
    for channel in 0..channels {
        let mut sum = 0.0_f32;
        for kernel_index in 0..kernel_size {
            let input_value = if kernel_index == 0 {
                current[channel]
            } else if history.len() >= kernel_index {
                history[history.len() - kernel_index][channel]
            } else {
                continue;
            };
            let weight_index = (channel * kernel_size) + (kernel_size - 1 - kernel_index);
            sum += input_value * weight_values[weight_index];
        }
        output[channel] = silu_scalar(sum);
    }
    Ok(output)
}

#[allow(dead_code)]
fn causal_mask<B: Backend>(seq_len: usize, device: &B::Device) -> Tensor<B, 4> {
    let mut values = vec![0_f32; seq_len * seq_len];
    for row in 0..seq_len {
        for col in row + 1..seq_len {
            values[row * seq_len + col] = f32::NEG_INFINITY;
        }
    }
    Tensor::from_data(TensorData::new(values, [1, 1, seq_len, seq_len]), device)
}

fn sigmoid_scalar(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

fn silu_scalar(value: f32) -> f32 {
    value * sigmoid_scalar(value)
}

fn softplus_scalar(value: f32) -> f32 {
    if value > 20.0 {
        value
    } else {
        (1.0 + value.exp()).ln()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BURN_TEXT_DIR_NAME, BURN_TEXT_MANIFEST_FILE_NAME, BurnTextManifest, BurnTextTensorSpec,
        LlmCpuBackend, apply_rotary_slice, bytes_per_element, causal_mask,
        decode_bytes_into_f32, dot_product, f16_bits_to_f32, first_mismatch_index,
        inspect_burn_text_runtime_status, rotary_embeddings, sigmoid_scalar,
    };
    use std::collections::BTreeMap;

    #[test]
    fn burn_text_status_reports_manifest_presence() {
        let temp = tempfile::tempdir().expect("temp dir");
        let status = inspect_burn_text_runtime_status(temp.path());
        assert!(!status.exists);
        let burn_dir = temp.path().join(BURN_TEXT_DIR_NAME);
        std::fs::create_dir_all(&burn_dir).expect("burn dir");
        std::fs::write(burn_dir.join(BURN_TEXT_MANIFEST_FILE_NAME), b"{}").expect("manifest");
        let status = inspect_burn_text_runtime_status(temp.path());
        assert!(status.exists);
    }

    #[test]
    fn float16_decode_matches_known_values() {
        let values = vec![0x3C00_u16, 0xC000_u16, 0x3800_u16];
        let bytes = values
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let mut out = Vec::new();
        decode_bytes_into_f32(&bytes, "float16", &mut out).expect("decode");
        assert_eq!(out, vec![1.0, -2.0, 0.5]);
        assert_eq!(f16_bits_to_f32(0x3555), f16_bits_to_f32(0x3555));
    }

    #[test]
    fn bytes_per_element_supports_expected_dtypes() {
        assert_eq!(bytes_per_element("float16").expect("f16"), 2);
        assert_eq!(bytes_per_element("bf16").expect("bf16"), 2);
        assert_eq!(bytes_per_element("f32").expect("f32"), 4);
    }

    #[test]
    fn causal_mask_blocks_future_positions() {
        let mask = causal_mask::<LlmCpuBackend>(3, &Default::default());
        let values = mask.to_data().to_vec::<f32>().expect("mask data");
        assert_eq!(values[0], 0.0);
        assert!(values[1].is_infinite() && values[1].is_sign_negative());
        assert!(values[2].is_infinite() && values[2].is_sign_negative());
        assert_eq!(values[4], 0.0);
        assert_eq!(values[8], 0.0);
    }

    #[test]
    fn rotary_slice_preserves_zero_vector() {
        let mut values = vec![0.0_f32; 8];
        let cos = vec![1.0_f32; 4];
        let sin = vec![0.0_f32; 4];
        apply_rotary_slice(&mut values, &cos, &sin, 2);
        assert_eq!(values, vec![0.0; 8]);
    }

    #[test]
    fn rotary_embeddings_uses_partial_factor() {
        let manifest = BurnTextManifest {
            format_version: 1,
            architecture: "qwen3_5_text".to_owned(),
            source_model_id: "fixture".to_owned(),
            text_model_type: "qwen3_5_text".to_owned(),
            vocab_size: 16,
            hidden_size: 8,
            intermediate_size: 16,
            num_hidden_layers: 1,
            num_attention_heads: 2,
            num_key_value_heads: 1,
            head_dim: 4,
            rms_norm_eps: 1e-6,
            hidden_act: "silu".to_owned(),
            partial_rotary_factor: 0.5,
            rope_theta: 10_000_000.0,
            linear_num_key_heads: 1,
            linear_num_value_heads: 2,
            linear_key_head_dim: 2,
            linear_value_head_dim: 2,
            linear_conv_kernel_dim: 4,
            layer_types: vec!["full_attention".to_owned()],
            tensors: BTreeMap::<String, BurnTextTensorSpec>::new(),
        };
        let embeddings = rotary_embeddings(3, &manifest).expect("rotary embeddings");
        assert_eq!(embeddings.rotary_dim, 2);
        assert_eq!(embeddings.cos.len(), 6);
        assert_eq!(embeddings.sin.len(), 6);
    }

    #[test]
    fn helper_math_is_stable() {
        assert!((sigmoid_scalar(0.0) - 0.5).abs() < 1e-6);
        assert_eq!(dot_product(&[1.0, 2.0], &[3.0, 4.0]), 11.0);
    }

    #[test]
    fn first_mismatch_index_reports_value_mismatches_and_length_mismatches() {
        assert_eq!(first_mismatch_index::<usize>(&[], &[]), None);
        assert_eq!(first_mismatch_index(&[1, 2, 3], &[1, 4, 3]), Some(1));
        assert_eq!(first_mismatch_index(&[1, 2], &[1, 2, 3]), Some(2));
        assert_eq!(first_mismatch_index(&[1, 2, 3], &[1, 2]), Some(2));
        assert_eq!(first_mismatch_index(&[1, 2, 3], &[1, 2, 3]), None);
    }
}
