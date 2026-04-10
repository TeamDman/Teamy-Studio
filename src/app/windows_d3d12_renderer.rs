#![expect(
    clippy::borrow_as_ptr,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_ptr_alignment,
    clippy::cast_sign_loss,
    clippy::collapsible_if,
    clippy::float_cmp,
    clippy::items_after_statements,
    clippy::multiple_unsafe_ops_per_block,
    clippy::ptr_as_ptr,
    clippy::semicolon_if_nothing_returned,
    clippy::semicolon_outside_block,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::undocumented_unsafe_blocks,
    clippy::unnecessary_cast,
    clippy::unused_self,
    clippy::vec_init_then_push,
    clippy::wildcard_imports
)]
use std::collections::{BTreeSet, HashMap};
use std::ops::Range;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread;
use std::time::Instant;

// os[impl os.windows.rendering.direct3d12]
use eyre::Context;
use fontdb::{Database, Family, Query, Source};
use image::{ImageBuffer, Rgba};
#[cfg(feature = "tracy")]
use tracing::debug_span;
use tracing::{info, info_span, instrument, warn};
use ttf_parser::{Face, GlyphId, OutlineBuilder};
use windows::Win32::Foundation::{E_FAIL, HANDLE, HWND, RECT, TRUE};
use windows::Win32::Graphics::Direct3D::Fxc::{
    D3DCOMPILE_DEBUG, D3DCOMPILE_SKIP_OPTIMIZATION, D3DCompileFromFile,
};
use windows::Win32::Graphics::Direct3D::{
    D3D_FEATURE_LEVEL_11_0, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, ID3DBlob,
};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::Threading::{CreateEventW, INFINITE, WaitForSingleObjectEx};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows::core::{Error, HSTRING, Interface, Owned, PCSTR, s};

use super::spatial::{ClientRect, TerminalCellPoint};
use super::windows_terminal::{
    SharedTerminalDisplayState, TerminalDisplayCursor, TerminalDisplayCursorStyle,
    TerminalDisplayRow, TerminalDisplayScrollbar, TerminalLayout,
};

const FRAME_COUNT: usize = 2;
const MAX_PANEL_COUNT: usize = 8_192;
const MAX_GLYPH_COUNT: usize = 8_192;
const MAX_VERTEX_COUNT: usize = (MAX_PANEL_COUNT + MAX_GLYPH_COUNT) * 6;
const FALLBACK_GLYPH: char = '?';
const MAX_CURVE_FLOAT4_COUNT: usize = 65_536;
const MAX_BAND_UINT_COUNT: usize = 262_144;
const TERMINAL_FONT_FAMILY: &str = "CaskaydiaCove Nerd Font Mono";
const SLUG_GLYPH_DILATION_PX: f32 = 0.5;
const SLUG_BAND_SIZE_FONT_UNITS: f32 = 64.0;
const TEAMY_D3D12_GPU_VALIDATION_ENV: &str = "TEAMY_D3D12_GPU_VALIDATION";

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct Vertex {
    position: [f32; 3],
    color: [f32; 4],
    uv: [f32; 2],
    effect: f32,
    glyph: f32,
    glyph_data: [f32; 4],
    banding: [f32; 4],
    normal: [f32; 2],
    jacobian: [f32; 4],
    _padding: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct ShaderParams {
    slug_matrix: [[f32; 4]; 4],
    slug_viewport: [f32; 4],
    scene_time: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
struct SlugGlyph {
    curve_start: u32,
    curve_count: u32,
    band_start: u32,
    band_count_x: u32,
    band_count_y: u32,
    band_transform: [f32; 4],
    x_min: f32,
    y_min: f32,
    x_max: f32,
    y_max: f32,
    advance: f32,
}

#[derive(Debug)]
struct LoadedTerminalFont {
    font_bytes: Vec<u8>,
    face_index: u32,
    units_per_em: f32,
    ascender: f32,
    descender: f32,
    cell_advance: f32,
}

impl SlugGlyph {
    fn empty(font: &LoadedTerminalFont) -> Self {
        Self {
            curve_start: 0,
            curve_count: 0,
            band_start: 0,
            band_count_x: 1,
            band_count_y: 1,
            band_transform: [0.0; 4],
            x_min: 0.0,
            y_min: font.descender,
            x_max: font.cell_advance,
            y_max: font.ascender,
            advance: font.cell_advance,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct QuadraticCurve {
    p0: [f32; 2],
    p1: [f32; 2],
    p2: [f32; 2],
}

#[derive(Clone, Copy, Debug)]
struct CurveExtents {
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
}

#[derive(Clone, Copy, Debug)]
pub enum PanelEffect {
    BlueBackground = 0,
    Sidecar = 1,
    DragHandle = 2,
    CodePanel = 3,
    ResultPanel = 4,
    PlayButton = 5,
    StopButton = 6,
    PlusButton = 7,
    TerminalFill = 8,
    TerminalCursor = 9,
    TerminalScrollbarTrack = 10,
    TerminalScrollbarThumb = 11,
    Text = 12,
}

#[derive(Clone, Copy, Debug)]
pub struct PanelRect {
    pub rect: RECT,
    pub color: [f32; 4],
    pub effect: PanelEffect,
}

#[derive(Clone, Copy, Debug)]
pub struct GlyphQuad {
    pub rect: RECT,
    pub color: [f32; 4],
    pub character: char,
}

#[derive(Clone, Debug)]
pub struct RenderScene {
    pub panels: Vec<PanelRect>,
    pub glyphs: Vec<GlyphQuad>,
    pub overlay_panels: Vec<PanelRect>,
}

#[derive(Debug)]
pub struct D3d12PanelRenderer {
    _dxgi_factory: IDXGIFactory4,
    dxgi_info_queue: Option<IDXGIInfoQueue>,
    device: ID3D12Device,
    command_queue: ID3D12CommandQueue,
    swap_chain: IDXGISwapChain3,
    render_targets: [Option<ID3D12Resource>; FRAME_COUNT],
    rtv_heap: ID3D12DescriptorHeap,
    rtv_descriptor_size: u32,
    command_allocators: [ID3D12CommandAllocator; FRAME_COUNT],
    command_list: ID3D12GraphicsCommandList,
    fence: ID3D12Fence,
    next_fence_value: u64,
    frame_fence_values: [u64; FRAME_COUNT],
    fence_event: Owned<HANDLE>,
    frame_latency_waitable_object: Owned<HANDLE>,
    root_signature: ID3D12RootSignature,
    pipeline_state: ID3D12PipelineState,
    vertex_buffer: ID3D12Resource,
    vertex_buffer_view: D3D12_VERTEX_BUFFER_VIEW,
    shader_param_buffer: ID3D12Resource,
    srv_heap: ID3D12DescriptorHeap,
    curve_buffer: ID3D12Resource,
    band_buffer: ID3D12Resource,
    font: LoadedTerminalFont,
    glyph_cache: HashMap<char, SlugGlyph>,
    cached_chars: Vec<char>,
    glyph_cache_generation: u64,
    viewport: D3D12_VIEWPORT,
    scissor_rect: RECT,
    width: u32,
    height: u32,
    animation_start: Instant,
}

#[derive(Debug)]
struct RenderThreadShared {
    pending_resize: Option<(u32, u32)>,
    pending_frame: Option<QueuedRenderFrame>,
    next_submission_id: u64,
    completed_submission_id: u64,
    shutdown: bool,
    error: Option<String>,
}

#[derive(Clone, Debug)]
struct QueuedRenderFrame {
    submission_id: u64,
    frame: RenderFrameModel,
    force_redraw: bool,
}

pub struct RenderThreadProxy {
    shared: Arc<(Mutex<RenderThreadShared>, Condvar)>,
    worker: Option<thread::JoinHandle<()>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RendererTerminalVisualState {
    pub track_hovered: bool,
    pub thumb_hovered: bool,
    pub thumb_grabbed: bool,
}

#[derive(Clone, Debug)]
pub struct RenderFrameModel {
    pub layout: TerminalLayout,
    pub cell_number: usize,
    pub output_text: String,
    pub output_cell_width: i32,
    pub output_cell_height: i32,
    pub terminal_cell_width: i32,
    pub terminal_cell_height: i32,
    pub terminal_display: SharedTerminalDisplayState,
    pub terminal_visual_state: RendererTerminalVisualState,
}

impl PartialEq for RenderFrameModel {
    fn eq(&self, other: &Self) -> bool {
        self.layout == other.layout
            && self.cell_number == other.cell_number
            && self.output_text == other.output_text
            && self.output_cell_width == other.output_cell_width
            && self.output_cell_height == other.output_cell_height
            && self.terminal_cell_width == other.terminal_cell_width
            && self.terminal_cell_height == other.terminal_cell_height
            && (Arc::ptr_eq(&self.terminal_display, &other.terminal_display)
                || self.terminal_display == other.terminal_display)
            && self.terminal_visual_state == other.terminal_visual_state
    }
}

#[derive(Clone, Debug)]
struct CachedChromeScene {
    layout: TerminalLayout,
    cell_number: usize,
    scene: Arc<RenderScene>,
}

#[derive(Clone, Debug)]
struct CachedOutputScene {
    layout: TerminalLayout,
    output_text: String,
    output_cell_width: i32,
    output_cell_height: i32,
    scene: Arc<RenderScene>,
}

#[derive(Clone, Debug)]
struct CachedTerminalRowScene {
    row: TerminalDisplayRow,
    scene: Arc<RenderScene>,
}

#[derive(Clone, Debug)]
struct CachedTerminalScene {
    layout: TerminalLayout,
    rows: Vec<CachedTerminalRowScene>,
    cursor: Option<TerminalDisplayCursor>,
    cursor_scene: Option<Arc<RenderScene>>,
    scrollbar: Option<TerminalDisplayScrollbar>,
    visual_state: RendererTerminalVisualState,
    scrollbar_scene: Arc<RenderScene>,
}

#[derive(Default)]
struct RenderThreadSceneCache {
    last_frame: Option<RenderFrameModel>,
    chrome: Option<CachedChromeScene>,
    chrome_vertices: Option<CachedSceneVertices>,
    output: Option<CachedOutputScene>,
    output_vertices: Option<CachedSceneVertices>,
    terminal: Option<CachedTerminalScene>,
    terminal_vertices: Vec<Option<CachedSceneVertices>>,
    composited_vertices: Option<CachedCompositedVertices>,
}

#[derive(Clone, Debug)]
struct CachedSceneVertices {
    glyph_cache_generation: u64,
    vertices: Vec<Vertex>,
}

#[derive(Clone, Debug, Default)]
struct CachedCompositedVertices {
    fragment_ranges: Vec<Range<usize>>,
    vertices: Vec<Vertex>,
}

impl RenderThreadProxy {
    #[instrument(level = "info", skip_all)]
    pub fn new(hwnd: HWND) -> eyre::Result<Self> {
        let shared = Arc::new((
            Mutex::new(RenderThreadShared {
                pending_resize: None,
                pending_frame: None,
                next_submission_id: 0,
                completed_submission_id: 0,
                shutdown: false,
                error: None,
            }),
            Condvar::new(),
        ));
        let shared_for_worker = Arc::clone(&shared);
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let raw_hwnd = hwnd.0 as isize;

        let worker = thread::Builder::new()
            .name("teamy-d3d12-renderer".to_owned())
            .spawn(move || {
                let startup_result =
                    D3d12PanelRenderer::new(HWND(raw_hwnd as *mut core::ffi::c_void));
                match startup_result {
                    Ok(mut renderer) => {
                        let _ = startup_tx.send(Ok(()));
                        render_thread_main_loop(&shared_for_worker, &mut renderer);
                    }
                    Err(error) => {
                        let message = format!("failed to create D3D12 renderer thread: {error}");
                        if let Ok(mut state) = shared_for_worker.0.lock() {
                            state.error = Some(message.clone());
                        }
                        let _ = startup_tx.send(Err(eyre::eyre!(message)));
                    }
                }
            })
            .map_err(|error| eyre::eyre!("failed to spawn D3D12 renderer thread: {error}"))?;

        startup_rx
            .recv()
            .map_err(|error| eyre::eyre!("renderer thread failed to report startup: {error}"))??;

        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }

    pub fn resize(&self, width: u32, height: u32) -> eyre::Result<()> {
        self.check_error()?;
        let (state_lock, wake) = &*self.shared;
        let mut state = state_lock
            .lock()
            .map_err(|error| eyre::eyre!("failed to lock renderer thread state: {error}"))?;
        state.pending_resize = Some((width, height));
        wake.notify_one();
        Ok(())
    }

    pub fn render_frame_model(&self, frame: RenderFrameModel) -> eyre::Result<()> {
        let _ = self.submit_render_frame_model(frame, false)?;
        Ok(())
    }

    pub fn render_frame_model_force_redraw(&self, frame: RenderFrameModel) -> eyre::Result<()> {
        let _ = self.submit_render_frame_model(frame, true)?;
        Ok(())
    }

    pub fn render_frame_model_blocking(&self, frame: RenderFrameModel) -> eyre::Result<()> {
        let submission_id = self.submit_render_frame_model(frame, false)?;
        let (state_lock, wake) = &*self.shared;
        let mut state = state_lock
            .lock()
            .map_err(|error| eyre::eyre!("failed to lock renderer thread state: {error}"))?;

        while state.completed_submission_id < submission_id {
            if let Some(error) = state.error.as_ref() {
                eyre::bail!(error.clone());
            }

            state = wake.wait(state).map_err(|error| {
                eyre::eyre!("failed to wait for renderer thread completion: {error}")
            })?;
        }

        if let Some(error) = state.error.as_ref() {
            eyre::bail!(error.clone());
        }

        Ok(())
    }

    fn submit_render_frame_model(
        &self,
        frame: RenderFrameModel,
        force_redraw: bool,
    ) -> eyre::Result<u64> {
        self.check_error()?;
        let (state_lock, wake) = &*self.shared;
        let mut state = state_lock
            .lock()
            .map_err(|error| eyre::eyre!("failed to lock renderer thread state: {error}"))?;
        state.next_submission_id += 1;
        let submission_id = state.next_submission_id;
        state.pending_frame = Some(QueuedRenderFrame {
            submission_id,
            frame,
            force_redraw,
        });
        wake.notify_one();
        Ok(submission_id)
    }

    fn check_error(&self) -> eyre::Result<()> {
        let state = self
            .shared
            .0
            .lock()
            .map_err(|error| eyre::eyre!("failed to lock renderer thread state: {error}"))?;
        if let Some(error) = state.error.as_ref() {
            eyre::bail!(error.clone());
        }
        Ok(())
    }
}

impl Drop for RenderThreadProxy {
    fn drop(&mut self) {
        let (state_lock, wake) = &*self.shared;
        if let Ok(mut state) = state_lock.lock() {
            state.shutdown = true;
            wake.notify_one();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn render_thread_main_loop(
    shared: &Arc<(Mutex<RenderThreadShared>, Condvar)>,
    renderer: &mut D3d12PanelRenderer,
) {
    let mut scene_cache = RenderThreadSceneCache::default();
    loop {
        let (pending_resize, pending_frame) = {
            let (state_lock, wake) = &**shared;
            let Ok(mut state) = state_lock.lock() else {
                return;
            };

            while !state.shutdown
                && state.pending_resize.is_none()
                && state.pending_frame.is_none()
                && state.error.is_none()
            {
                state = match wake.wait(state) {
                    Ok(state) => state,
                    Err(_) => return,
                };
            }

            if state.shutdown || state.error.is_some() {
                return;
            }

            (state.pending_resize.take(), state.pending_frame.take())
        };

        let result = (|| -> eyre::Result<()> {
            if let Some((width, height)) = pending_resize {
                #[cfg(feature = "tracy")]
                let _span = debug_span!("render_thread_resize_swap_chain").entered();
                renderer.resize(width, height)?;
            }

            if let Some(queued_frame) = pending_frame.as_ref() {
                #[cfg(feature = "tracy")]
                let _span = debug_span!("render_thread_render_frame").entered();
                renderer.render_frame_model(
                    &queued_frame.frame,
                    queued_frame.force_redraw,
                    pending_resize.is_some(),
                    &mut scene_cache,
                )?;
            }

            Ok(())
        })();

        if let Err(error) = result {
            if let Ok(mut state) = shared.0.lock() {
                state.error = Some(error.to_string());
                shared.1.notify_all();
            }
            return;
        }

        if let Some(queued_frame) = pending_frame
            && let Ok(mut state) = shared.0.lock()
        {
            state.completed_submission_id = state
                .completed_submission_id
                .max(queued_frame.submission_id);
            shared.1.notify_all();
        }
    }
}

impl D3d12PanelRenderer {
    #[instrument(level = "info", skip_all)]
    pub fn new(hwnd: HWND) -> eyre::Result<Self> {
        let (dxgi_factory, device, dxgi_info_queue) =
            info_span!("create_d3d12_device").in_scope(create_device)?;
        let command_queue =
            info_span!("create_d3d12_command_queue").in_scope(|| create_command_queue(&device))?;
        let (width, height) =
            info_span!("query_renderer_client_size").in_scope(|| client_size(hwnd))?;
        let swap_chain = info_span!("create_swap_chain", width, height)
            .in_scope(|| create_swap_chain(&dxgi_factory, &command_queue, hwnd, width, height))?;
        unsafe { dxgi_factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER)? };
        unsafe { swap_chain.SetMaximumFrameLatency(1)? };
        let frame_latency_waitable_object =
            unsafe { Owned::new(swap_chain.GetFrameLatencyWaitableObject()) };

        let (rtv_heap, rtv_descriptor_size, render_targets) =
            info_span!("create_render_targets")
                .in_scope(|| create_render_targets(&device, &swap_chain))?;
        let command_allocators = info_span!("create_command_allocators")
            .in_scope(|| create_command_allocators(&device))?;
        let (srv_heap, curve_buffer, band_buffer) = info_span!("create_slug_buffers_and_srv")
            .in_scope(|| create_slug_buffers_and_srv(&device))?;
        let font = info_span!("load_terminal_font").in_scope(load_terminal_font)?;
        let root_signature =
            info_span!("create_root_signature").in_scope(|| create_root_signature(&device))?;
        let pipeline_state = info_span!("create_pipeline_state")
            .in_scope(|| create_pipeline_state(&device, &root_signature))?;
        let command_list: ID3D12GraphicsCommandList = unsafe {
            device.CreateCommandList(
                0,
                D3D12_COMMAND_LIST_TYPE_DIRECT,
                &command_allocators[0],
                &pipeline_state,
            )
        }?;
        unsafe { command_list.Close()? };

        let (vertex_buffer, vertex_buffer_view) =
            info_span!("create_vertex_buffer").in_scope(|| create_vertex_buffer(&device))?;
        let shader_param_buffer = info_span!("create_shader_param_buffer")
            .in_scope(|| create_shader_param_buffer(&device))?;
        let fence: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }?;
        let fence_event = unsafe { Owned::new(CreateEventW(None, false, false, None)?) };

        let viewport = D3D12_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: width as f32,
            Height: height as f32,
            MinDepth: D3D12_MIN_DEPTH,
            MaxDepth: D3D12_MAX_DEPTH,
        };
        let scissor_rect = RECT {
            left: 0,
            top: 0,
            right: width as i32,
            bottom: height as i32,
        };

        Ok(Self {
            _dxgi_factory: dxgi_factory,
            dxgi_info_queue,
            device,
            command_queue,
            swap_chain,
            render_targets: render_targets.map(Some),
            rtv_heap,
            rtv_descriptor_size,
            command_allocators,
            command_list,
            fence,
            next_fence_value: 1,
            frame_fence_values: [0; FRAME_COUNT],
            fence_event,
            frame_latency_waitable_object,
            root_signature,
            pipeline_state,
            vertex_buffer,
            vertex_buffer_view,
            shader_param_buffer,
            srv_heap,
            curve_buffer,
            band_buffer,
            font,
            glyph_cache: HashMap::new(),
            cached_chars: Vec::new(),
            glyph_cache_generation: 0,
            viewport,
            scissor_rect,
            width,
            height,
            animation_start: Instant::now(),
        })
    }

    #[instrument(level = "info", skip_all, fields(width, height))]
    pub fn resize(&mut self, width: u32, height: u32) -> eyre::Result<()> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        if width == self.width && height == self.height {
            return Ok(());
        }

        self.wait_for_gpu()?;
        unsafe {
            self.command_list.Reset(&self.command_allocators[0], None)?;
            self.command_list.ClearState(None);
            self.command_list.Close()?;
        }
        let command_allocators = create_command_allocators(&self.device)?;
        let command_list =
            create_closed_command_list(&self.device, &command_allocators[0], &self.pipeline_state)?;
        self.command_allocators = command_allocators;
        self.command_list = command_list;
        let old_render_targets =
            std::mem::replace(&mut self.render_targets, std::array::from_fn(|_| None));
        drop(old_render_targets);
        let old_rtv_heap =
            std::mem::replace(&mut self.rtv_heap, create_empty_rtv_heap(&self.device)?);
        drop(old_rtv_heap);
        self.rtv_descriptor_size = unsafe {
            self.device
                .GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV)
        };
        self.frame_latency_waitable_object = Owned::default();
        if let Err(error) = unsafe {
            self.swap_chain.ResizeBuffers(
                FRAME_COUNT as u32,
                width,
                height,
                DXGI_FORMAT_B8G8R8A8_UNORM,
                DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT,
            )
        } {
            self.log_dxgi_debug_messages("ResizeBuffers");
            self.log_dxgi_live_objects("ResizeBuffers");
            return Err(error).wrap_err("failed to resize swap chain buffers");
        }
        self.frame_latency_waitable_object =
            unsafe { Owned::new(self.swap_chain.GetFrameLatencyWaitableObject()) };

        let (rtv_heap, rtv_descriptor_size, render_targets) =
            create_render_targets(&self.device, &self.swap_chain)?;
        self.rtv_heap = rtv_heap;
        self.rtv_descriptor_size = rtv_descriptor_size;
        self.render_targets = render_targets.map(Some);
        self.frame_fence_values = [0; FRAME_COUNT];
        self.width = width;
        self.height = height;
        self.viewport.Width = width as f32;
        self.viewport.Height = height as f32;
        self.scissor_rect = RECT {
            left: 0,
            top: 0,
            right: width as i32,
            bottom: height as i32,
        };
        Ok(())
    }

    fn log_dxgi_debug_messages(&self, context: &str) {
        let Some(queue) = &self.dxgi_info_queue else {
            return;
        };

        let count = unsafe { queue.GetNumStoredMessages(DXGI_DEBUG_ALL) };
        if count == 0 {
            return;
        }

        warn!(context, count, "DXGI debug messages");
        for index in 0..count {
            let mut message_size = 0;
            if unsafe { queue.GetMessage(DXGI_DEBUG_ALL, index, None, &mut message_size) }.is_err()
            {
                warn!(context, index, "failed to query DXGI debug message size");
                continue;
            }

            let mut message_buffer = vec![0_u8; message_size];
            let message_ptr = message_buffer.as_mut_ptr() as *mut DXGI_INFO_QUEUE_MESSAGE;
            if unsafe {
                queue.GetMessage(DXGI_DEBUG_ALL, index, Some(message_ptr), &mut message_size)
            }
            .is_err()
            {
                warn!(context, index, "failed to read DXGI debug message");
                continue;
            }

            let (severity, description) = unsafe {
                let description_slice = std::slice::from_raw_parts(
                    (*message_ptr).pDescription as *const u8,
                    (*message_ptr).DescriptionByteLength,
                );
                let severity = match (*message_ptr).Severity {
                    DXGI_INFO_QUEUE_MESSAGE_SEVERITY_CORRUPTION => "CORRUPTION",
                    DXGI_INFO_QUEUE_MESSAGE_SEVERITY_ERROR => "ERROR",
                    DXGI_INFO_QUEUE_MESSAGE_SEVERITY_WARNING => "WARNING",
                    DXGI_INFO_QUEUE_MESSAGE_SEVERITY_INFO => "INFO",
                    DXGI_INFO_QUEUE_MESSAGE_SEVERITY_MESSAGE => "MESSAGE",
                    _ => "UNKNOWN",
                };
                let description = String::from_utf8_lossy(description_slice)
                    .trim_matches(char::from(0))
                    .trim()
                    .to_string();
                (severity, description)
            };

            warn!(context, index, severity, %description, "DXGI debug message");
        }
        unsafe { queue.ClearStoredMessages(DXGI_DEBUG_ALL) };
    }

    fn log_dxgi_live_objects(&self, context: &str) {
        let debug = unsafe { DXGIGetDebugInterface1::<IDXGIDebug1>(0) };
        match debug {
            Ok(debug) => {
                if let Err(error) = unsafe {
                    debug.ReportLiveObjects(
                        DXGI_DEBUG_ALL,
                        DXGI_DEBUG_RLO_FLAGS(
                            DXGI_DEBUG_RLO_DETAIL.0 | DXGI_DEBUG_RLO_IGNORE_INTERNAL.0,
                        ),
                    )
                } {
                    warn!(context, ?error, "failed to report DXGI live objects");
                }
            }
            Err(error) => {
                warn!(context, ?error, "failed to acquire DXGI debug reporter");
            }
        }
    }

    #[expect(
        dead_code,
        reason = "compatibility wrapper while callers migrate to fragment-based rendering"
    )]
    #[cfg_attr(feature = "tracy", instrument(level = "debug", skip_all))]
    pub fn render(&mut self, scene: &RenderScene) -> eyre::Result<()> {
        self.render_fragments(&[scene])
    }

    #[cfg_attr(feature = "tracy", instrument(level = "debug", skip_all))]
    pub fn render_fragments(&mut self, scenes: &[&RenderScene]) -> eyre::Result<()> {
        {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("update_slug_curves").entered();
            let _ = self.update_slug_curves_for_fragments(scenes)?;
        }
        let vertex_count = {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("update_scene_vertices").entered();
            self.update_scene_vertices_for_fragments(scenes)?
        };
        self.execute_prepared_frame(vertex_count)
    }

    fn render_frame_model(
        &mut self,
        frame: &RenderFrameModel,
        force_redraw: bool,
        resized: bool,
        scene_cache: &mut RenderThreadSceneCache,
    ) -> eyre::Result<()> {
        if !force_redraw && !resized && scene_cache.last_frame.as_ref() == Some(frame) {
            return Ok(());
        }

        let (chrome_scene, chrome_reused) =
            chrome_scene_fragment(&mut scene_cache.chrome, frame.layout, frame.cell_number);
        let (terminal_scenes, terminal_reused) = terminal_scene_fragments(
            &mut scene_cache.terminal,
            frame.layout,
            &frame.terminal_display,
            frame.terminal_visual_state,
            frame.terminal_cell_width,
            frame.terminal_cell_height,
        );
        let (output_scene, output_reused) = output_scene_fragment(
            &mut scene_cache.output,
            frame.layout,
            &frame.output_text,
            frame.output_cell_width,
            frame.output_cell_height,
        );

        let glyph_cache_changed = {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("update_slug_curves").entered();
            let mut scenes = Vec::with_capacity(terminal_scenes.len() + 2);
            scenes.push(chrome_scene.as_ref());
            scenes.extend(terminal_scenes.iter().map(Arc::as_ref));
            scenes.push(output_scene.as_ref());
            self.update_slug_curves_for_fragments(&scenes)?
        };

        let chrome_vertices = {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("update_chrome_vertices").entered();
            self.cached_fragment_vertices(
                chrome_scene.as_ref(),
                chrome_reused && !glyph_cache_changed,
                &mut scene_cache.chrome_vertices,
            )
        };
        let terminal_vertices = {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("update_terminal_vertices").entered();
            scene_cache
                .terminal_vertices
                .resize_with(terminal_scenes.len(), || None);
            let mut vertices = Vec::with_capacity(terminal_scenes.len());
            for (scene, cached_vertices) in terminal_scenes
                .iter()
                .zip(scene_cache.terminal_vertices.iter_mut())
            {
                vertices.push(self.cached_fragment_vertices(
                    scene.as_ref(),
                    terminal_reused[vertices.len()] && !glyph_cache_changed,
                    cached_vertices,
                ));
            }
            vertices
        };
        let output_vertices = {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("update_output_vertices").entered();
            self.cached_fragment_vertices(
                output_scene.as_ref(),
                output_reused && !glyph_cache_changed,
                &mut scene_cache.output_vertices,
            )
        };

        let vertex_count = {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("update_scene_vertices").entered();
            let mut fragments = Vec::with_capacity(terminal_vertices.len() + 2);
            let mut fragment_reused = Vec::with_capacity(terminal_vertices.len() + 2);
            fragments.push(chrome_vertices);
            fragment_reused.push(chrome_reused && !glyph_cache_changed);
            fragments.extend(terminal_vertices);
            fragment_reused.extend(
                terminal_reused
                    .iter()
                    .map(|reused| *reused && !glyph_cache_changed),
            );
            fragments.push(output_vertices);
            fragment_reused.push(output_reused && !glyph_cache_changed);
            self.upload_composited_fragment_vertices(
                &fragments,
                &fragment_reused,
                &mut scene_cache.composited_vertices,
            )?
        };

        scene_cache.last_frame = Some(frame.clone());

        self.execute_prepared_frame(vertex_count)
    }

    fn update_scene_vertices_for_fragments(&self, scenes: &[&RenderScene]) -> eyre::Result<usize> {
        let built_fragments = scenes
            .iter()
            .map(|scene| self.build_scene_vertices(scene))
            .collect::<Vec<_>>();
        let fragment_slices = built_fragments
            .iter()
            .map(Vec::as_slice)
            .collect::<Vec<_>>();
        self.upload_cached_fragment_vertices(&fragment_slices)
    }

    fn cached_fragment_vertices<'a>(
        &self,
        scene: &RenderScene,
        reused: bool,
        cached_vertices: &'a mut Option<CachedSceneVertices>,
    ) -> &'a [Vertex] {
        let can_reuse = can_reuse_cached_scene_vertices(
            reused,
            cached_vertices.as_ref(),
            self.glyph_cache_generation,
        );

        if !can_reuse {
            *cached_vertices = Some(CachedSceneVertices {
                glyph_cache_generation: self.glyph_cache_generation,
                vertices: self.build_scene_vertices(scene),
            });
        }

        cached_vertices
            .as_ref()
            .map_or(&[], |cached| cached.vertices.as_slice())
    }

    fn build_scene_vertices(&self, scene: &RenderScene) -> Vec<Vertex> {
        let mut vertices = Vec::with_capacity(
            (scene.panels.len() + scene.glyphs.len() + scene.overlay_panels.len()) * 6,
        );
        for panel in &scene.panels {
            append_rect(
                &mut vertices,
                panel.rect,
                panel.color,
                panel.effect as u32,
                0,
            );
        }
        for glyph in &scene.glyphs {
            let slug_glyph = self
                .glyph_cache
                .get(&glyph.character)
                .or_else(|| self.glyph_cache.get(&FALLBACK_GLYPH))
                .copied()
                .unwrap_or_else(|| SlugGlyph::empty(&self.font));
            append_text_rect(
                &mut vertices,
                glyph.rect,
                glyph.color,
                slug_glyph,
                &self.font,
            );
        }
        for panel in &scene.overlay_panels {
            append_rect(
                &mut vertices,
                panel.rect,
                panel.color,
                panel.effect as u32,
                0,
            );
        }
        vertices
    }

    fn upload_cached_fragment_vertices(&self, fragments: &[&[Vertex]]) -> eyre::Result<usize> {
        let vertex_count = fragments
            .iter()
            .map(|fragment| fragment.len())
            .sum::<usize>();

        unsafe {
            let mut mapped = std::ptr::null_mut();
            self.vertex_buffer.Map(0, None, Some(&mut mapped))?;
            let mut write_ptr = mapped as *mut Vertex;
            for fragment in fragments {
                std::ptr::copy_nonoverlapping(fragment.as_ptr(), write_ptr, fragment.len());
                write_ptr = write_ptr.add(fragment.len());
            }
            self.vertex_buffer.Unmap(0, None);
        }

        Ok(vertex_count)
    }

    fn upload_composited_fragment_vertices(
        &self,
        fragments: &[&[Vertex]],
        fragment_reused: &[bool],
        cached_vertices: &mut Option<CachedCompositedVertices>,
    ) -> eyre::Result<usize> {
        debug_assert_eq!(fragments.len(), fragment_reused.len());

        let fragment_ranges = fragment_vertex_ranges(fragments);
        let vertex_count = fragment_ranges.last().map_or(0, |range| range.end);

        if cached_vertices
            .as_ref()
            .is_none_or(|cached| !fragment_ranges_match(&cached.fragment_ranges, &fragment_ranges))
        {
            let mut vertices = Vec::with_capacity(vertex_count);
            for fragment in fragments {
                vertices.extend_from_slice(fragment);
            }
            self.upload_vertex_ranges(&vertices, &[0..vertex_count])?;
            *cached_vertices = Some(CachedCompositedVertices {
                fragment_ranges,
                vertices,
            });
            return Ok(vertex_count);
        }

        let Some(cached_vertices) = cached_vertices.as_mut() else {
            return Ok(vertex_count);
        };

        let dirty_ranges = dirty_fragment_ranges(
            &cached_vertices.fragment_ranges,
            fragments,
            fragment_reused,
            &mut cached_vertices.vertices,
        );
        self.upload_vertex_ranges(&cached_vertices.vertices, &dirty_ranges)?;
        Ok(vertex_count)
    }

    fn upload_vertex_ranges(
        &self,
        vertices: &[Vertex],
        ranges: &[Range<usize>],
    ) -> eyre::Result<()> {
        if ranges.is_empty() {
            return Ok(());
        }

        unsafe {
            let mut mapped = std::ptr::null_mut();
            self.vertex_buffer.Map(0, None, Some(&mut mapped))?;
            let base_ptr = mapped as *mut Vertex;
            for range in ranges {
                if range.is_empty() {
                    continue;
                }

                std::ptr::copy_nonoverlapping(
                    vertices[range.clone()].as_ptr(),
                    base_ptr.add(range.start),
                    range.len(),
                );
            }
            self.vertex_buffer.Unmap(0, None);
        }

        Ok(())
    }

    fn execute_prepared_frame(&mut self, vertex_count: usize) -> eyre::Result<()> {
        {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("wait_for_frame_sync").entered();
            self.wait_for_frame_latency()?;
        }
        let frame_index = unsafe { self.swap_chain.GetCurrentBackBufferIndex() as usize };
        {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("wait_for_frame_fence").entered();
            self.wait_for_frame(frame_index)?;
        }

        let current_target = self.render_targets[frame_index]
            .as_ref()
            .ok_or_else(|| eyre::eyre!("render target was missing for current frame"))?;
        let command_allocator = &self.command_allocators[frame_index];

        {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("record_render_commands").entered();
            unsafe {
                command_allocator.Reset()?;
                self.command_list
                    .Reset(command_allocator, &self.pipeline_state)?;

                self.update_shader_params()?;

                self.command_list
                    .SetDescriptorHeaps(&[Some(self.srv_heap.clone())]);
                self.command_list
                    .SetGraphicsRootSignature(&self.root_signature);
                self.command_list.SetGraphicsRootConstantBufferView(
                    0,
                    self.shader_param_buffer.GetGPUVirtualAddress(),
                );
                self.command_list.SetGraphicsRootDescriptorTable(
                    1,
                    self.srv_heap.GetGPUDescriptorHandleForHeapStart(),
                );
                self.command_list.RSSetViewports(&[self.viewport]);
                self.command_list.RSSetScissorRects(&[self.scissor_rect]);

                issue_transition_barrier(
                    &self.command_list,
                    current_target,
                    D3D12_RESOURCE_STATE_PRESENT,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                );

                let rtv_handle = D3D12_CPU_DESCRIPTOR_HANDLE {
                    ptr: self.rtv_heap.GetCPUDescriptorHandleForHeapStart().ptr
                        + frame_index * self.rtv_descriptor_size as usize,
                };
                self.command_list
                    .OMSetRenderTargets(1, Some(&rtv_handle), false, None);

                let clear_color = [0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32];
                self.command_list
                    .ClearRenderTargetView(rtv_handle, &clear_color, None);
                self.command_list
                    .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                self.command_list
                    .IASetVertexBuffers(0, Some(&[self.vertex_buffer_view]));
                self.command_list
                    .DrawInstanced(vertex_count as u32, 1, 0, 0);

                issue_transition_barrier(
                    &self.command_list,
                    current_target,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PRESENT,
                );
                self.command_list.Close()?;
            }
        }

        let command_lists = [Some(self.command_list.cast::<ID3D12CommandList>()?)];
        {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("submit_and_present_frame").entered();
            unsafe {
                self.command_queue.ExecuteCommandLists(&command_lists);
                self.swap_chain.Present(0, DXGI_PRESENT(0)).ok()?;
            }
        }

        self.signal_frame(frame_index)?;
        #[cfg(feature = "tracy")]
        info!(message = "finished frame", tracy.frame_mark = true,);
        Ok(())
    }

    #[expect(
        dead_code,
        reason = "compatibility wrapper while callers migrate to fragment-based rendering"
    )]
    fn update_scene_vertices(&self, scene: &RenderScene) -> eyre::Result<usize> {
        self.update_scene_vertices_for_fragments(&[scene])
    }

    fn update_slug_curves_for_fragments(&mut self, scenes: &[&RenderScene]) -> eyre::Result<bool> {
        let scene_chars = collect_scene_chars_from_fragments(scenes);
        if scene_chars == self.cached_chars {
            return Ok(false);
        }

        let (curve_data, band_data, glyph_cache) =
            build_slug_curve_buffer(&self.font, &scene_chars)?;
        unsafe {
            let mut mapped = std::ptr::null_mut();
            self.curve_buffer.Map(0, None, Some(&mut mapped))?;
            std::ptr::write_bytes(
                mapped,
                0,
                MAX_CURVE_FLOAT4_COUNT * std::mem::size_of::<[f32; 4]>(),
            );
            std::ptr::copy_nonoverlapping(
                curve_data.as_ptr(),
                mapped as *mut [f32; 4],
                curve_data.len(),
            );
            self.curve_buffer.Unmap(0, None);

            let mut band_mapped = std::ptr::null_mut();
            self.band_buffer.Map(0, None, Some(&mut band_mapped))?;
            std::ptr::write_bytes(
                band_mapped,
                0,
                MAX_BAND_UINT_COUNT * std::mem::size_of::<u32>(),
            );
            std::ptr::copy_nonoverlapping(
                band_data.as_ptr(),
                band_mapped as *mut u32,
                band_data.len(),
            );
            self.band_buffer.Unmap(0, None);
        }

        self.glyph_cache = glyph_cache;
        self.cached_chars = scene_chars;
        self.glyph_cache_generation += 1;
        Ok(true)
    }

    fn update_shader_params(&self) -> eyre::Result<()> {
        let elapsed_seconds = self.animation_start.elapsed().as_secs_f32();
        let params = build_shader_params(self.width as f32, self.height as f32, elapsed_seconds);
        unsafe {
            let mut mapped = std::ptr::null_mut();
            self.shader_param_buffer.Map(0, None, Some(&mut mapped))?;
            std::ptr::copy_nonoverlapping(&params, mapped as *mut ShaderParams, 1);
            self.shader_param_buffer.Unmap(0, None);
        }
        Ok(())
    }

    fn wait_for_frame_latency(&self) -> eyre::Result<()> {
        if self.frame_latency_waitable_object.0.is_null() {
            return Err(eyre::eyre!(
                "swap chain did not provide a frame latency waitable object"
            ));
        }

        unsafe {
            WaitForSingleObjectEx(*self.frame_latency_waitable_object, INFINITE, false);
        }

        Ok(())
    }

    #[expect(
        dead_code,
        reason = "compatibility wrapper while callers migrate to fragment-based rendering"
    )]
    fn update_slug_curves(&mut self, scene: &RenderScene) -> eyre::Result<()> {
        let _ = self.update_slug_curves_for_fragments(&[scene])?;
        Ok(())
    }

    fn wait_for_frame(&self, frame_index: usize) -> eyre::Result<()> {
        let fence_value = self.frame_fence_values[frame_index];
        if fence_value == 0 {
            return Ok(());
        }

        unsafe {
            if self.fence.GetCompletedValue() < fence_value {
                self.fence
                    .SetEventOnCompletion(fence_value, *self.fence_event)?;
                WaitForSingleObjectEx(*self.fence_event, INFINITE, false);
            }
        }

        Ok(())
    }

    fn signal_frame(&mut self, frame_index: usize) -> eyre::Result<()> {
        let fence_value = self.next_fence_value;
        unsafe {
            self.command_queue.Signal(&self.fence, fence_value)?;
        }
        self.frame_fence_values[frame_index] = fence_value;
        self.next_fence_value += 1;
        Ok(())
    }

    fn wait_for_gpu(&mut self) -> eyre::Result<()> {
        let fence_value = self.next_fence_value;
        unsafe {
            self.command_queue.Signal(&self.fence, fence_value)?;
            if self.fence.GetCompletedValue() < fence_value {
                self.fence
                    .SetEventOnCompletion(fence_value, *self.fence_event)?;
                WaitForSingleObjectEx(*self.fence_event, INFINITE, false);
            }
        }
        self.next_fence_value += 1;
        Ok(())
    }
}

fn can_reuse_cached_scene_vertices(
    reused: bool,
    cached_vertices: Option<&CachedSceneVertices>,
    glyph_cache_generation: u64,
) -> bool {
    reused
        && cached_vertices
            .is_some_and(|cached| cached.glyph_cache_generation == glyph_cache_generation)
}

fn fragment_vertex_ranges(fragments: &[&[Vertex]]) -> Vec<Range<usize>> {
    let mut next_start = 0;
    let mut ranges = Vec::with_capacity(fragments.len());
    for fragment in fragments {
        let start = next_start;
        next_start += fragment.len();
        ranges.push(start..next_start);
    }
    ranges
}

fn fragment_ranges_match(current: &[Range<usize>], next: &[Range<usize>]) -> bool {
    current.len() == next.len()
        && current
            .iter()
            .zip(next)
            .all(|(current, next)| current.len() == next.len())
}

fn dirty_fragment_ranges(
    fragment_ranges: &[Range<usize>],
    fragments: &[&[Vertex]],
    fragment_reused: &[bool],
    cached_vertices: &mut [Vertex],
) -> Vec<Range<usize>> {
    debug_assert_eq!(fragment_ranges.len(), fragments.len());
    debug_assert_eq!(fragments.len(), fragment_reused.len());

    let mut dirty_ranges: Vec<Range<usize>> = Vec::new();

    for (index, fragment) in fragments.iter().enumerate() {
        if fragment_reused[index] {
            continue;
        }

        let range = fragment_ranges[index].clone();
        cached_vertices[range.clone()].copy_from_slice(fragment);
        if let Some(previous) = dirty_ranges.last_mut()
            && previous.end == range.start
        {
            previous.end = range.end;
            continue;
        }
        dirty_ranges.push(range);
    }

    dirty_ranges
}

impl Drop for D3d12PanelRenderer {
    fn drop(&mut self) {
        let _ = self.wait_for_gpu();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TerminalScrollbarGeometry {
    thumb_rect: ClientRect,
    thumb_height: i32,
    travel: i32,
    max_offset: u64,
}

fn chrome_scene_fragment(
    cached_chrome_scene: &mut Option<CachedChromeScene>,
    layout: TerminalLayout,
    cell_number: usize,
) -> (Arc<RenderScene>, bool) {
    if let Some(cached) = cached_chrome_scene.as_ref()
        && cached.layout == layout
        && cached.cell_number == cell_number
    {
        return (Arc::clone(&cached.scene), true);
    }

    let mut scene = build_panel_scene(layout);
    push_centered_text(
        &mut scene,
        layout.drag_handle_rect().to_win32_rect(),
        &cell_number.to_string(),
        [0.95, 0.95, 0.98, 1.0],
    );
    let scene = Arc::new(scene);
    *cached_chrome_scene = Some(CachedChromeScene {
        layout,
        cell_number,
        scene: Arc::clone(&scene),
    });
    (scene, false)
}

fn output_scene_fragment(
    cached_output_scene: &mut Option<CachedOutputScene>,
    layout: TerminalLayout,
    output_text: &str,
    output_cell_width: i32,
    output_cell_height: i32,
) -> (Arc<RenderScene>, bool) {
    if let Some(cached) = cached_output_scene.as_ref()
        && cached.layout == layout
        && cached.output_text == output_text
        && cached.output_cell_width == output_cell_width
        && cached.output_cell_height == output_cell_height
    {
        return (Arc::clone(&cached.scene), true);
    }

    let mut scene = RenderScene {
        panels: Vec::new(),
        glyphs: Vec::with_capacity(output_text.chars().count()),
        overlay_panels: Vec::new(),
    };
    push_text_block(
        &mut scene,
        layout.result_panel_rect().inset(14).to_win32_rect(),
        output_text,
        output_cell_width,
        output_cell_height,
        [0.96, 0.95, 0.90, 1.0],
    );
    let scene = Arc::new(scene);
    *cached_output_scene = Some(CachedOutputScene {
        layout,
        output_text: output_text.to_owned(),
        output_cell_width,
        output_cell_height,
        scene: Arc::clone(&scene),
    });
    (scene, false)
}

fn terminal_scene_fragments(
    cached_terminal_scene: &mut Option<CachedTerminalScene>,
    layout: TerminalLayout,
    display: &SharedTerminalDisplayState,
    visual_state: RendererTerminalVisualState,
    terminal_cell_width: i32,
    terminal_cell_height: i32,
) -> (Vec<Arc<RenderScene>>, Vec<bool>) {
    let terminal_rect = layout.terminal_viewport_rect().inset(4);
    let scrollbar_rect = layout.terminal_scrollbar_rect().inset(4);

    let cached = cached_terminal_scene
        .as_ref()
        .filter(|cached| cached.layout == layout);

    let mut row_fragments = Vec::with_capacity(display.rows.len() + 2);
    let mut reused = Vec::with_capacity(display.rows.len() + 2);
    let mut cached_rows = Vec::with_capacity(display.rows.len());

    for (index, row) in display.rows.iter().enumerate() {
        let cached_row = cached.and_then(|cached| cached.rows.get(index));
        if let Some(cached_row) = cached_row && cached_row.row == *row {
            row_fragments.push(Arc::clone(&cached_row.scene));
            reused.push(true);
            cached_rows.push(cached_row.clone());
            continue;
        }

        let scene = Arc::new(build_terminal_row_scene(
            terminal_rect,
            terminal_cell_width,
            terminal_cell_height,
            row,
        ));
        row_fragments.push(Arc::clone(&scene));
        reused.push(false);
        cached_rows.push(CachedTerminalRowScene {
            row: row.clone(),
            scene,
        });
    }

    let (cursor_scene, cursor_reused) = if let Some(cached) = cached
        && cached.cursor == display.cursor
    {
        (cached.cursor_scene.clone(), true)
    } else {
        (
            display.cursor.map(|cursor| {
                Arc::new(build_terminal_cursor_scene(
                    terminal_rect,
                    terminal_cell_width,
                    terminal_cell_height,
                    cursor,
                ))
            }),
            false,
        )
    };

    if let Some(cursor_scene) = cursor_scene.as_ref() {
        row_fragments.push(Arc::clone(cursor_scene));
        reused.push(cursor_reused);
    }

    let (scrollbar_scene, scrollbar_reused) = if let Some(cached) = cached
        && cached.scrollbar == display.scrollbar
        && cached.visual_state == visual_state
    {
        (Arc::clone(&cached.scrollbar_scene), true)
    } else {
        (
            Arc::new(build_terminal_scrollbar_scene(
                scrollbar_rect,
                display.scrollbar,
                visual_state,
            )),
            false,
        )
    };
    row_fragments.push(Arc::clone(&scrollbar_scene));
    reused.push(scrollbar_reused);

    *cached_terminal_scene = Some(CachedTerminalScene {
        layout,
        rows: cached_rows,
        cursor: display.cursor,
        cursor_scene,
        scrollbar: display.scrollbar,
        visual_state,
        scrollbar_scene,
    });

    (row_fragments, reused)
}

fn build_terminal_row_scene(
    terminal_rect: ClientRect,
    cell_width: i32,
    cell_height: i32,
    row: &TerminalDisplayRow,
) -> RenderScene {
    let mut scene = RenderScene {
        panels: Vec::with_capacity(row.backgrounds.len()),
        glyphs: Vec::with_capacity(row.glyphs.len()),
        overlay_panels: Vec::new(),
    };

    for background in &row.backgrounds {
        push_panel(
            &mut scene,
            terminal_cell_rect(terminal_rect, background.cell, cell_width, cell_height)
                .to_win32_rect(),
            background.color,
            PanelEffect::TerminalFill,
        );
    }

    for glyph in &row.glyphs {
        push_glyph(
            &mut scene,
            terminal_cell_rect(terminal_rect, glyph.cell, cell_width, cell_height).to_win32_rect(),
            glyph.character,
            glyph.color,
        );
    }

    scene
}

fn build_terminal_cursor_scene(
    terminal_rect: ClientRect,
    cell_width: i32,
    cell_height: i32,
    cursor: TerminalDisplayCursor,
) -> RenderScene {
    let mut scene = RenderScene {
        panels: Vec::new(),
        glyphs: Vec::new(),
        overlay_panels: Vec::with_capacity(4),
    };
    let cell_rect = terminal_cell_rect(terminal_rect, cursor.cell, cell_width, cell_height);
    for rect in terminal_cursor_overlay_rects(cell_rect, cursor.style) {
        push_overlay_panel(
            &mut scene,
            rect.to_win32_rect(),
            terminal_cursor_overlay_color(cursor.color, cursor.style),
            PanelEffect::TerminalCursor,
        );
    }

    scene
}

fn build_terminal_scrollbar_scene(
    scrollbar_rect: ClientRect,
    scrollbar: Option<TerminalDisplayScrollbar>,
    visual_state: RendererTerminalVisualState,
) -> RenderScene {
    let mut scene = RenderScene {
        panels: Vec::with_capacity(2),
        glyphs: Vec::new(),
        overlay_panels: Vec::new(),
    };
    if scrollbar_rect.width() <= 0 || scrollbar_rect.height() <= 0 {
        return scene;
    }

    push_panel(
        &mut scene,
        scrollbar_rect.to_win32_rect(),
        if visual_state.track_hovered {
            [0.28, 0.10, 0.40, 0.90]
        } else {
            [0.19, 0.08, 0.28, 0.78]
        },
        PanelEffect::TerminalScrollbarTrack,
    );

    let Some(scrollbar) = scrollbar else {
        return scene;
    };
    let Some(geometry) = terminal_scrollbar_geometry(scrollbar_rect, scrollbar) else {
        return scene;
    };

    push_panel(
        &mut scene,
        geometry.thumb_rect.to_win32_rect(),
        if visual_state.thumb_grabbed {
            [1.00, 0.72, 1.00, 1.00]
        } else if visual_state.thumb_hovered {
            [0.92, 0.55, 1.00, 0.96]
        } else {
            [0.82, 0.38, 0.98, 0.88]
        },
        PanelEffect::TerminalScrollbarThumb,
    );

    scene
}

fn terminal_scrollbar_geometry(
    scrollbar_rect: ClientRect,
    scrollbar: TerminalDisplayScrollbar,
) -> Option<TerminalScrollbarGeometry> {
    if scrollbar_rect.width() <= 0
        || scrollbar_rect.height() <= 0
        || scrollbar.total == 0
        || scrollbar.visible == 0
    {
        return None;
    }

    let track_height = u64::try_from(scrollbar_rect.height().max(1)).ok()?;
    let min_thumb_height = scrollbar_rect.width().max(22);
    let proportional_thumb = (track_height.saturating_mul(scrollbar.visible) / scrollbar.total)
        .max(u64::try_from(min_thumb_height).ok()?);
    let thumb_height = i32::try_from(proportional_thumb.min(track_height))
        .ok()?
        .clamp(min_thumb_height, scrollbar_rect.height().max(1));
    let travel = (scrollbar_rect.height() - thumb_height).max(0);
    let max_offset = scrollbar.total.saturating_sub(scrollbar.visible);
    let clamped_offset = scrollbar.offset.min(max_offset);
    let thumb_offset = if travel == 0 || max_offset == 0 {
        0
    } else {
        let travel = u64::try_from(travel).ok()?;
        i32::try_from(travel.saturating_mul(clamped_offset) / max_offset).ok()?
    };
    let thumb_top = scrollbar_rect.top() + thumb_offset;

    Some(TerminalScrollbarGeometry {
        thumb_rect: ClientRect::new(
            scrollbar_rect.left(),
            thumb_top,
            scrollbar_rect.right(),
            (thumb_top + thumb_height).min(scrollbar_rect.bottom()),
        ),
        thumb_height,
        travel,
        max_offset,
    })
}

fn terminal_cursor_overlay_color(
    mut color: [f32; 4],
    style: TerminalDisplayCursorStyle,
) -> [f32; 4] {
    color[3] = match style {
        TerminalDisplayCursorStyle::Block => 0.42,
        TerminalDisplayCursorStyle::BlockHollow => 0.95,
        TerminalDisplayCursorStyle::Bar | TerminalDisplayCursorStyle::Underline => 0.9,
    };
    color
}

fn terminal_cell_rect(
    terminal_rect: ClientRect,
    cell: TerminalCellPoint,
    cell_width: i32,
    cell_height: i32,
) -> ClientRect {
    cell.to_client_rect(terminal_rect, cell_width, cell_height)
}

fn terminal_cursor_overlay_rects(
    cell_rect: ClientRect,
    style: TerminalDisplayCursorStyle,
) -> Vec<ClientRect> {
    let width = cell_rect.width().max(1);
    let height = cell_rect.height().max(1);
    let thickness = (width.min(height) / 6).clamp(2, 4);

    match style {
        TerminalDisplayCursorStyle::Bar => vec![ClientRect::new(
            cell_rect.left(),
            cell_rect.top(),
            (cell_rect.left() + thickness).min(cell_rect.right()),
            cell_rect.bottom(),
        )],
        TerminalDisplayCursorStyle::Block => vec![cell_rect],
        TerminalDisplayCursorStyle::Underline => vec![ClientRect::new(
            cell_rect.left(),
            (cell_rect.bottom() - thickness).max(cell_rect.top()),
            cell_rect.right(),
            cell_rect.bottom(),
        )],
        TerminalDisplayCursorStyle::BlockHollow => vec![
            ClientRect::new(
                cell_rect.left(),
                cell_rect.top(),
                cell_rect.right(),
                (cell_rect.top() + thickness).min(cell_rect.bottom()),
            ),
            ClientRect::new(
                cell_rect.left(),
                (cell_rect.bottom() - thickness).max(cell_rect.top()),
                cell_rect.right(),
                cell_rect.bottom(),
            ),
            ClientRect::new(
                cell_rect.left(),
                cell_rect.top(),
                (cell_rect.left() + thickness).min(cell_rect.right()),
                cell_rect.bottom(),
            ),
            ClientRect::new(
                (cell_rect.right() - thickness).max(cell_rect.left()),
                cell_rect.top(),
                cell_rect.right(),
                cell_rect.bottom(),
            ),
        ],
    }
}

/// behavior[impl window.appearance.chrome]
/// behavior[impl window.appearance.backgrounds.blue-half-transparent]
/// behavior[impl window.appearance.code-panel.single-surface]
pub fn build_panel_scene(layout: TerminalLayout) -> RenderScene {
    let blue = [0.11, 0.44, 0.94, 0.5];
    let sidecar = [0.55, 0.14, 0.14, 1.0];
    let drag = [0.42, 0.18, 0.60, 1.0];
    let code = [0.05, 0.06, 0.08, 1.0];
    let result = [0.84, 0.44, 0.13, 1.0];
    let button = [0.12, 0.13, 0.17, 1.0];
    let mut panels = Vec::with_capacity(8);
    panels.push(PanelRect {
        rect: RECT {
            left: 0,
            top: 0,
            right: layout.client_width,
            bottom: layout.client_height,
        },
        color: blue,
        effect: PanelEffect::BlueBackground,
    });
    panels.push(PanelRect {
        rect: layout.sidecar_rect().to_win32_rect(),
        color: sidecar,
        effect: PanelEffect::Sidecar,
    });
    panels.push(PanelRect {
        rect: layout.drag_handle_rect().to_win32_rect(),
        color: drag,
        effect: PanelEffect::DragHandle,
    });
    panels.push(PanelRect {
        rect: layout.code_panel_rect().to_win32_rect(),
        color: code,
        effect: PanelEffect::CodePanel,
    });
    panels.push(PanelRect {
        rect: layout.result_panel_rect().to_win32_rect(),
        color: result,
        effect: PanelEffect::ResultPanel,
    });
    panels.push(PanelRect {
        rect: layout.plus_button_rect().to_win32_rect(),
        color: button,
        effect: PanelEffect::PlusButton,
    });
    panels.push(PanelRect {
        rect: layout.sidecar_button_rect(0).to_win32_rect(),
        color: button,
        effect: PanelEffect::PlayButton,
    });
    panels.push(PanelRect {
        rect: layout.sidecar_button_rect(1).to_win32_rect(),
        color: button,
        effect: PanelEffect::StopButton,
    });
    RenderScene {
        panels,
        glyphs: Vec::with_capacity(2_048),
        overlay_panels: Vec::with_capacity(16),
    }
}

pub fn push_panel(scene: &mut RenderScene, rect: RECT, color: [f32; 4], effect: PanelEffect) {
    if scene.panels.len() + scene.overlay_panels.len() >= MAX_PANEL_COUNT {
        return;
    }

    scene.panels.push(PanelRect {
        rect,
        color,
        effect,
    });
}

pub fn push_overlay_panel(
    scene: &mut RenderScene,
    rect: RECT,
    color: [f32; 4],
    effect: PanelEffect,
) {
    if scene.panels.len() + scene.overlay_panels.len() >= MAX_PANEL_COUNT {
        return;
    }

    scene.overlay_panels.push(PanelRect {
        rect,
        color,
        effect,
    });
}

pub fn push_text_block(
    scene: &mut RenderScene,
    rect: RECT,
    text: &str,
    glyph_width: i32,
    glyph_height: i32,
    color: [f32; 4],
) {
    let mut cursor_x = rect.left;
    let mut cursor_y = rect.top;

    for character in text.chars() {
        if character == '\n' {
            cursor_x = rect.left;
            cursor_y += glyph_height;
            if cursor_y + glyph_height > rect.bottom {
                break;
            }
            continue;
        }

        if cursor_x + glyph_width > rect.right {
            cursor_x = rect.left;
            cursor_y += glyph_height;
        }
        if cursor_y + glyph_height > rect.bottom {
            break;
        }

        if character != ' ' && scene.glyphs.len() < MAX_GLYPH_COUNT {
            push_glyph(
                scene,
                RECT {
                    left: cursor_x,
                    top: cursor_y,
                    right: cursor_x + glyph_width,
                    bottom: cursor_y + glyph_height,
                },
                character,
                color,
            );
        }

        cursor_x += glyph_width;
    }
}

pub fn push_centered_text(scene: &mut RenderScene, rect: RECT, text: &str, color: [f32; 4]) {
    let glyph_count = i32::try_from(text.chars().count())
        .unwrap_or_default()
        .max(1);
    let available_width = (rect.right - rect.left - 16).max(8);
    let available_height = (rect.bottom - rect.top - 16).max(8);
    let glyph_height = available_height.clamp(12, 28);
    let glyph_width = ((available_width / glyph_count).min((glyph_height * 3) / 2)).max(8);
    let total_width = glyph_width * glyph_count;
    let text_rect = RECT {
        left: rect.left + (((rect.right - rect.left) - total_width).max(0) / 2),
        top: rect.top + (((rect.bottom - rect.top) - glyph_height).max(0) / 2),
        right: rect.right,
        bottom: rect.bottom,
    };
    push_text_block(scene, text_rect, text, glyph_width, glyph_height, color);
}

pub fn push_glyph(scene: &mut RenderScene, rect: RECT, character: char, color: [f32; 4]) {
    if scene.glyphs.len() >= MAX_GLYPH_COUNT || character == ' ' {
        return;
    }
    scene.glyphs.push(GlyphQuad {
        rect,
        color,
        character,
    });
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "compatibility wrapper while callers migrate to fragment-based rendering"
    )
)]
fn collect_scene_chars(scene: &RenderScene) -> Vec<char> {
    collect_scene_chars_from_fragments(&[scene])
}

fn collect_scene_chars_from_fragments(scenes: &[&RenderScene]) -> Vec<char> {
    let glyph_capacity = scenes.iter().map(|scene| scene.glyphs.len()).sum::<usize>() + 1;
    let mut chars = Vec::with_capacity(glyph_capacity);
    chars.push(FALLBACK_GLYPH);
    for scene in scenes {
        for glyph in &scene.glyphs {
            if !chars.contains(&glyph.character) {
                chars.push(glyph.character);
            }
        }
    }
    chars
}

fn load_terminal_font() -> eyre::Result<LoadedTerminalFont> {
    let mut database = Database::new();
    database.load_system_fonts();
    let query = Query {
        families: &[Family::Name(TERMINAL_FONT_FAMILY)],
        ..Query::default()
    };
    let font_id = database
        .query(&query)
        .ok_or_else(|| eyre::eyre!("failed to locate installed terminal font"))?;
    let face_info = database
        .face(font_id)
        .ok_or_else(|| eyre::eyre!("fontdb returned an invalid font handle"))?;

    let font_bytes = match &face_info.source {
        Source::File(path) => std::fs::read(path)
            .wrap_err_with(|| format!("failed to read font file {}", path.display()))?,
        Source::Binary(data) => data.as_ref().as_ref().to_vec(),
        Source::SharedFile(path, _) => std::fs::read(path)
            .wrap_err_with(|| format!("failed to read shared font file {}", path.display()))?,
    };

    let face = Face::parse(&font_bytes, face_info.index)
        .wrap_err("failed to parse installed terminal font")?;
    let fallback_id = face
        .glyph_index(FALLBACK_GLYPH)
        .or_else(|| face.glyph_index('W'))
        .ok_or_else(|| eyre::eyre!("terminal font did not contain expected fallback glyphs"))?;
    let cell_advance = face
        .glyph_hor_advance(fallback_id)
        .map_or(1024.0, f32::from);
    let units_per_em = f32::from(face.units_per_em());
    let ascender = f32::from(face.ascender());
    let descender = f32::from(face.descender());

    Ok(LoadedTerminalFont {
        font_bytes,
        face_index: face_info.index,
        units_per_em,
        ascender,
        descender,
        cell_advance,
    })
}

fn build_slug_curve_buffer(
    font: &LoadedTerminalFont,
    chars: &[char],
) -> eyre::Result<(Vec<[f32; 4]>, Vec<u32>, HashMap<char, SlugGlyph>)> {
    let face = Face::parse(&font.font_bytes, font.face_index)
        .wrap_err("failed to parse terminal font for slug curve build")?;
    let fallback_id = face
        .glyph_index(FALLBACK_GLYPH)
        .ok_or_else(|| eyre::eyre!("terminal font did not contain fallback glyph"))?;
    let mut curve_data = Vec::new();
    let mut band_data = Vec::new();
    let mut glyph_cache = HashMap::new();

    for character in chars {
        let glyph_id = face.glyph_index(*character).unwrap_or(fallback_id);
        let curves = extract_glyph_curves(&face, glyph_id);
        let curve_start = curve_data.len() as u32;
        for curve in &curves {
            curve_data.push([curve.p0[0], curve.p0[1], curve.p1[0], curve.p1[1]]);
            curve_data.push([curve.p2[0], curve.p2[1], 0.0, 0.0]);
        }
        let band_start = band_data.len() as u32;
        let glyph = build_slug_glyph_from_face(font, &face, glyph_id, curve_start, curves.len());
        let (band_count_x, band_count_y, band_transform) =
            append_slug_band_data(&curves, glyph, &mut band_data);
        glyph_cache.insert(
            *character,
            SlugGlyph {
                band_start,
                band_count_x,
                band_count_y,
                band_transform,
                ..glyph
            },
        );
    }

    if curve_data.len() > MAX_CURVE_FLOAT4_COUNT {
        eyre::bail!("slug curve buffer capacity exceeded")
    }

    if band_data.len() > MAX_BAND_UINT_COUNT {
        eyre::bail!("slug band buffer capacity exceeded")
    }

    Ok((curve_data, band_data, glyph_cache))
}

fn extract_glyph_curves(face: &Face<'_>, glyph_id: GlyphId) -> Vec<QuadraticCurve> {
    let mut builder = QuadraticCurveBuilder::default();
    let _ = face.outline_glyph(glyph_id, &mut builder);
    builder.curves
}

pub fn write_slug_snapshot_png(
    character: char,
    font_size_px: u32,
    image_width: u32,
    image_height: u32,
    output_path: &Path,
) -> eyre::Result<()> {
    let font = load_terminal_font()?;
    let face = Face::parse(&font.font_bytes, font.face_index)
        .wrap_err("failed to parse terminal font for snapshot")?;
    let (curves, band_data, glyph) = load_snapshot_glyph(&font, &face, character)?;
    let mut image = ImageBuffer::<Rgba<u8>, Vec<u8>>::new(image_width, image_height);
    clear_snapshot_background(&mut image);
    render_snapshot_glyph_into_image(
        &mut image,
        0,
        0,
        image_width,
        image_height,
        font_size_px,
        &font,
        &curves,
        &band_data,
        glyph,
    );

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).wrap_err_with(|| {
            format!("failed to create snapshot directory {}", parent.display())
        })?;
    }
    image
        .save(output_path)
        .wrap_err_with(|| format!("failed to write snapshot png {}", output_path.display()))?;
    Ok(())
}

pub fn write_slug_snapshot_sheet_png(
    font_size_px: u32,
    cell_size_px: u32,
    columns: u32,
    output_path: &Path,
    index_output_path: &Path,
) -> eyre::Result<()> {
    let font = load_terminal_font()?;
    let face = Face::parse(&font.font_bytes, font.face_index)
        .wrap_err("failed to parse terminal font for snapshot sheet")?;
    let characters = collect_font_unicode_chars(&face);
    let columns = columns.max(1);
    let cell_size_px = cell_size_px.max(24);
    let rows = u32::try_from(characters.len().div_ceil(columns as usize))
        .unwrap_or(1)
        .max(1);
    let image_width = columns * cell_size_px;
    let image_height = rows * cell_size_px;
    let mut image = ImageBuffer::<Rgba<u8>, Vec<u8>>::new(image_width, image_height);
    clear_snapshot_background(&mut image);
    draw_snapshot_grid(&mut image, cell_size_px);
    let mut index_text = String::new();

    for (index, character) in characters.iter().copied().enumerate() {
        let cell_index = u32::try_from(index).unwrap_or_default();
        let cell_x = (cell_index % columns) * cell_size_px;
        let cell_y = (cell_index / columns) * cell_size_px;
        let (curves, band_data, glyph) = load_snapshot_glyph(&font, &face, character)?;
        render_snapshot_glyph_into_image(
            &mut image,
            cell_x,
            cell_y,
            cell_size_px,
            cell_size_px,
            font_size_px,
            &font,
            &curves,
            &band_data,
            glyph,
        );
        use std::fmt::Write as _;
        let _ = writeln!(
            index_text,
            "row={} col={} codepoint=U+{:04X} char={:?}",
            cell_index / columns,
            cell_index % columns,
            u32::from(character),
            character
        );
    }

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .wrap_err_with(|| format!("failed to create sheet directory {}", parent.display()))?;
    }
    if let Some(parent) = index_output_path.parent() {
        std::fs::create_dir_all(parent).wrap_err_with(|| {
            format!(
                "failed to create sheet index directory {}",
                parent.display()
            )
        })?;
    }

    image
        .save(output_path)
        .wrap_err_with(|| format!("failed to write snapshot sheet {}", output_path.display()))?;
    std::fs::write(index_output_path, index_text).wrap_err_with(|| {
        format!(
            "failed to write snapshot sheet index {}",
            index_output_path.display()
        )
    })?;
    Ok(())
}

fn load_snapshot_glyph(
    font: &LoadedTerminalFont,
    face: &Face<'_>,
    character: char,
) -> eyre::Result<(Vec<QuadraticCurve>, Vec<u32>, SlugGlyph)> {
    let glyph_id = face
        .glyph_index(character)
        .or_else(|| face.glyph_index(FALLBACK_GLYPH))
        .ok_or_else(|| eyre::eyre!("failed to resolve snapshot glyph in font"))?;
    let curves = extract_glyph_curves(face, glyph_id);
    let glyph = build_slug_glyph_from_face(font, face, glyph_id, 0, curves.len());
    let mut band_data = Vec::new();
    let (band_count_x, band_count_y, band_transform) =
        append_slug_band_data(&curves, glyph, &mut band_data);
    Ok((
        curves,
        band_data,
        SlugGlyph {
            band_start: 0,
            band_count_x,
            band_count_y,
            band_transform,
            ..glyph
        },
    ))
}

fn build_slug_glyph_from_face(
    font: &LoadedTerminalFont,
    face: &Face<'_>,
    glyph_id: GlyphId,
    curve_start: u32,
    curve_count: usize,
) -> SlugGlyph {
    let advance = face
        .glyph_hor_advance(glyph_id)
        .map_or(font.cell_advance, f32::from);
    let bbox = face.glyph_bounding_box(glyph_id);
    SlugGlyph {
        curve_start,
        curve_count: u32::try_from(curve_count).unwrap_or(u32::MAX),
        band_start: 0,
        band_count_x: 1,
        band_count_y: 1,
        band_transform: [0.0; 4],
        x_min: bbox.map_or(0.0, |rect| f32::from(rect.x_min)),
        y_min: bbox.map_or(font.descender, |rect| f32::from(rect.y_min)),
        x_max: bbox.map_or(advance, |rect| f32::from(rect.x_max)),
        y_max: bbox.map_or(font.ascender, |rect| f32::from(rect.y_max)),
        advance,
    }
}

fn append_slug_band_data(
    curves: &[QuadraticCurve],
    glyph: SlugGlyph,
    band_data: &mut Vec<u32>,
) -> (u32, u32, [f32; 4]) {
    let band_count_x = compute_band_count(glyph.x_max - glyph.x_min);
    let band_count_y = compute_band_count(glyph.y_max - glyph.y_min);
    let band_transform = [
        compute_band_scale(glyph.x_min, glyph.x_max, band_count_x),
        compute_band_scale(glyph.y_min, glyph.y_max, band_count_y),
        compute_band_offset(glyph.x_min, glyph.x_max, band_count_x),
        compute_band_offset(glyph.y_min, glyph.y_max, band_count_y),
    ];
    let curve_extents: Vec<_> = curves.iter().copied().map(curve_extents).collect();
    let mut horizontal_bands = vec![Vec::<usize>::new(); band_count_y as usize];
    let mut vertical_bands = vec![Vec::<usize>::new(); band_count_x as usize];

    for (curve_index, extents) in curve_extents.iter().enumerate() {
        let horizontal_start = band_index(
            extents.min_y,
            band_transform[1],
            band_transform[3],
            band_count_y,
        );
        let horizontal_end = band_index(
            extents.max_y,
            band_transform[1],
            band_transform[3],
            band_count_y,
        );
        for band in horizontal_start..=horizontal_end {
            horizontal_bands[band as usize].push(curve_index);
        }

        let vertical_start = band_index(
            extents.min_x,
            band_transform[0],
            band_transform[2],
            band_count_x,
        );
        let vertical_end = band_index(
            extents.max_x,
            band_transform[0],
            band_transform[2],
            band_count_x,
        );
        for band in vertical_start..=vertical_end {
            vertical_bands[band as usize].push(curve_index);
        }
    }

    for band in &mut horizontal_bands {
        band.sort_by(|lhs, rhs| {
            curve_extents[*rhs]
                .max_x
                .total_cmp(&curve_extents[*lhs].max_x)
        });
    }
    for band in &mut vertical_bands {
        band.sort_by(|lhs, rhs| {
            curve_extents[*rhs]
                .max_y
                .total_cmp(&curve_extents[*lhs].max_y)
        });
    }

    let table_start = band_data.len();
    let table_len = ((band_count_x + band_count_y) as usize) * 2;
    band_data.resize(table_start + table_len, 0);

    for (band_index, band) in horizontal_bands.iter().enumerate() {
        let entry_index = table_start + (band_index * 2);
        band_data[entry_index] = band.len() as u32;
        band_data[entry_index + 1] = band_data.len() as u32;
        for curve_index in band {
            band_data.push(*curve_index as u32);
        }
    }

    let vertical_table_start = table_start + (band_count_y as usize * 2);
    for (band_index, band) in vertical_bands.iter().enumerate() {
        let entry_index = vertical_table_start + (band_index * 2);
        band_data[entry_index] = band.len() as u32;
        band_data[entry_index + 1] = band_data.len() as u32;
        for curve_index in band {
            band_data.push(*curve_index as u32);
        }
    }

    (band_count_x, band_count_y, band_transform)
}

fn curve_extents(curve: QuadraticCurve) -> CurveExtents {
    CurveExtents {
        min_x: curve.p0[0].min(curve.p1[0]).min(curve.p2[0]),
        max_x: curve.p0[0].max(curve.p1[0]).max(curve.p2[0]),
        min_y: curve.p0[1].min(curve.p1[1]).min(curve.p2[1]),
        max_y: curve.p0[1].max(curve.p1[1]).max(curve.p2[1]),
    }
}

fn compute_band_count(span: f32) -> u32 {
    ((span.max(1.0) / SLUG_BAND_SIZE_FONT_UNITS).ceil() as u32).clamp(1, 255)
}

fn compute_band_scale(min_value: f32, max_value: f32, band_count: u32) -> f32 {
    let _ = min_value;
    band_count.max(1) as f32 / (max_value - min_value).max(1.0)
}

fn compute_band_offset(min_value: f32, max_value: f32, band_count: u32) -> f32 {
    -(min_value * compute_band_scale(min_value, max_value, band_count))
}

fn band_index(value: f32, scale: f32, offset: f32, band_count: u32) -> u32 {
    ((value * scale) + offset)
        .trunc()
        .clamp(0.0, band_count.saturating_sub(1) as f32) as u32
}

fn collect_font_unicode_chars(face: &Face<'_>) -> Vec<char> {
    let mut chars = BTreeSet::new();
    if let Some(cmap) = face.tables().cmap {
        for subtable in cmap.subtables {
            if !subtable.is_unicode() {
                continue;
            }
            subtable.codepoints(|codepoint| {
                if let Some(character) = char::from_u32(codepoint) {
                    if face.glyph_index(character).is_some() {
                        chars.insert(character);
                    }
                }
            });
        }
    }
    chars.into_iter().collect()
}

fn clear_snapshot_background(image: &mut ImageBuffer<Rgba<u8>, Vec<u8>>) {
    for pixel in image.pixels_mut() {
        *pixel = Rgba([0, 0, 0, 255]);
    }
}

fn draw_snapshot_grid(image: &mut ImageBuffer<Rgba<u8>, Vec<u8>>, cell_size_px: u32) {
    let grid = Rgba([20, 20, 20, 255]);
    for y in (0..image.height()).step_by(cell_size_px as usize) {
        for x in 0..image.width() {
            image.put_pixel(x, y, grid);
        }
    }
    for x in (0..image.width()).step_by(cell_size_px as usize) {
        for y in 0..image.height() {
            image.put_pixel(x, y, grid);
        }
    }
}

fn render_snapshot_glyph_into_image(
    image: &mut ImageBuffer<Rgba<u8>, Vec<u8>>,
    origin_x: u32,
    origin_y: u32,
    image_width: u32,
    image_height: u32,
    font_size_px: u32,
    font: &LoadedTerminalFont,
    curves: &[QuadraticCurve],
    band_data: &[u32],
    glyph: SlugGlyph,
) {
    let font_height_units = font.units_per_em.max(1.0);
    let scale = font_size_px as f32 / font_height_units;
    let uv_pad_x = SLUG_GLYPH_DILATION_PX / scale;
    let uv_pad_y = SLUG_GLYPH_DILATION_PX / scale;
    let glyph_width_px =
        (((glyph.x_max - glyph.x_min) + (uv_pad_x * 2.0)).max(1.0) * scale).max(1.0);
    let glyph_height_px =
        (((glyph.y_max - glyph.y_min) + (uv_pad_y * 2.0)).max(1.0) * scale).max(1.0);
    let offset_x = origin_x as f32 + ((image_width as f32 - glyph_width_px) * 0.5).max(0.0);
    let offset_y = origin_y as f32 + ((image_height as f32 - glyph_height_px) * 0.5).max(0.0);
    let render_x_min = glyph.x_min - uv_pad_x;
    let render_y_max = glyph.y_max + uv_pad_y;
    let start_x = offset_x.floor().max(origin_x as f32) as u32;
    let end_x = (offset_x + glyph_width_px)
        .ceil()
        .min((origin_x + image_width) as f32)
        .max(start_x as f32) as u32;
    let start_y = offset_y.floor().max(origin_y as f32) as u32;
    let end_y = (offset_y + glyph_height_px)
        .ceil()
        .min((origin_y + image_height) as f32)
        .max(start_y as f32) as u32;

    for y in start_y..end_y {
        for x in start_x..end_x {
            let sample_x = x as f32 + 0.5;
            let sample_y = y as f32 + 0.5;
            let render_coord = [
                render_x_min + ((sample_x - offset_x) / scale),
                render_y_max - ((sample_y - offset_y) / scale),
            ];
            let coverage = cpu_slug_coverage(render_coord, scale, curves, band_data, glyph);
            if coverage <= 0.0 {
                continue;
            }
            let value = (coverage * 255.0).clamp(0.0, 255.0) as u8;
            image.put_pixel(x, y, Rgba([255, 255, 255, value]));
        }
    }
}

fn cpu_slug_coverage(
    render_coord: [f32; 2],
    pixels_per_em: f32,
    curves: &[QuadraticCurve],
    band_data: &[u32],
    glyph: SlugGlyph,
) -> f32 {
    if glyph.curve_count == 0 {
        return 0.0;
    }

    let mut xcov: f32 = 0.0;
    let mut ycov: f32 = 0.0;
    let mut xwgt: f32 = 0.0;
    let mut ywgt: f32 = 0.0;
    let horizontal_band = band_index(
        render_coord[1],
        glyph.band_transform[1],
        glyph.band_transform[3],
        glyph.band_count_y,
    ) as usize;
    let horizontal_entry = glyph.band_start as usize + (horizontal_band * 2);
    let horizontal_count = band_data.get(horizontal_entry).copied().unwrap_or_default() as usize;
    let horizontal_start = band_data
        .get(horizontal_entry + 1)
        .copied()
        .unwrap_or_default() as usize;
    for offset in 0..horizontal_count {
        let curve_index = band_data
            .get(horizontal_start + offset)
            .copied()
            .unwrap_or_default() as usize;
        let Some(curve) = curves.get(curve_index) else {
            continue;
        };
        if (curve_extents(*curve).max_x - render_coord[0]) * pixels_per_em < -0.5 {
            break;
        }
        accumulate_horizontal_curve_coverage(
            curve,
            render_coord,
            pixels_per_em,
            &mut xcov,
            &mut xwgt,
        );
    }

    let vertical_band = band_index(
        render_coord[0],
        glyph.band_transform[0],
        glyph.band_transform[2],
        glyph.band_count_x,
    ) as usize;
    let vertical_entry =
        glyph.band_start as usize + (glyph.band_count_y as usize * 2) + (vertical_band * 2);
    let vertical_count = band_data.get(vertical_entry).copied().unwrap_or_default() as usize;
    let vertical_start = band_data
        .get(vertical_entry + 1)
        .copied()
        .unwrap_or_default() as usize;
    for offset in 0..vertical_count {
        let curve_index = band_data
            .get(vertical_start + offset)
            .copied()
            .unwrap_or_default() as usize;
        let Some(curve) = curves.get(curve_index) else {
            continue;
        };
        if (curve_extents(*curve).max_y - render_coord[1]) * pixels_per_em < -0.5 {
            break;
        }
        accumulate_vertical_curve_coverage(
            curve,
            render_coord,
            pixels_per_em,
            &mut ycov,
            &mut ywgt,
        );
    }

    calc_coverage(xcov, ycov, xwgt, ywgt)
}

#[cfg(test)]
fn cpu_slug_coverage_all_curves(
    render_coord: [f32; 2],
    pixels_per_em: f32,
    curves: &[QuadraticCurve],
    glyph: SlugGlyph,
) -> f32 {
    if glyph.curve_count == 0 {
        return 0.0;
    }

    let mut xcov: f32 = 0.0;
    let mut ycov: f32 = 0.0;
    let mut xwgt: f32 = 0.0;
    let mut ywgt: f32 = 0.0;
    let start = usize::try_from(glyph.curve_start).unwrap_or_default();
    let end = start + usize::try_from(glyph.curve_count).unwrap_or_default();

    for curve in curves.iter().skip(start).take(end.saturating_sub(start)) {
        accumulate_horizontal_curve_coverage(
            curve,
            render_coord,
            pixels_per_em,
            &mut xcov,
            &mut xwgt,
        );
        accumulate_vertical_curve_coverage(
            curve,
            render_coord,
            pixels_per_em,
            &mut ycov,
            &mut ywgt,
        );
    }

    calc_coverage(xcov, ycov, xwgt, ywgt)
}

fn is_degenerate_quadratic(curve: &QuadraticCurve) -> bool {
    let epsilon = 1.0 / 1024.0;
    let ax = curve.p0[0] - (curve.p1[0] * 2.0) + curve.p2[0];
    let ay = curve.p0[1] - (curve.p1[1] * 2.0) + curve.p2[1];
    ax.abs() <= epsilon && ay.abs() <= epsilon
}

fn should_use_degenerate_line_fallback(curve: &QuadraticCurve) -> bool {
    if !is_degenerate_quadratic(curve) {
        return false;
    }

    let axis_epsilon = 1.0 / 65536.0;
    let dx = (curve.p2[0] - curve.p0[0]).abs();
    let dy = (curve.p2[1] - curve.p0[1]).abs();
    dx > axis_epsilon && dy > axis_epsilon
}

fn apply_degenerate_horizontal_coverage(
    curve: &QuadraticCurve,
    render_coord: [f32; 2],
    pixels_per_em: f32,
    xcov: &mut f32,
    xwgt: &mut f32,
) {
    let p0 = [curve.p0[0] - render_coord[0], curve.p0[1] - render_coord[1]];
    let p1 = [curve.p2[0] - render_coord[0], curve.p2[1] - render_coord[1]];
    if let Some(intersection_x) = horizontal_line_intersection(p0, p1) {
        let sample = saturate((intersection_x * pixels_per_em) + 0.5);
        if p1[1] > p0[1] {
            *xcov += sample;
        } else {
            *xcov -= sample;
        }
        *xwgt = (*xwgt).max(saturate(1.0 - (intersection_x * pixels_per_em).abs() * 2.0));
    }
}

fn apply_degenerate_vertical_coverage(
    curve: &QuadraticCurve,
    render_coord: [f32; 2],
    pixels_per_em: f32,
    ycov: &mut f32,
    ywgt: &mut f32,
) {
    let p0 = [curve.p0[0] - render_coord[0], curve.p0[1] - render_coord[1]];
    let p1 = [curve.p2[0] - render_coord[0], curve.p2[1] - render_coord[1]];
    if let Some(intersection_y) = vertical_line_intersection(p0, p1) {
        let sample = saturate((intersection_y * pixels_per_em) + 0.5);
        if p1[0] > p0[0] {
            *ycov += sample;
        } else {
            *ycov -= sample;
        }
        *ywgt = (*ywgt).max(saturate(1.0 - (intersection_y * pixels_per_em).abs() * 2.0));
    }
}

fn horizontal_line_intersection(p0: [f32; 2], p1: [f32; 2]) -> Option<f32> {
    if !crosses_zero_half_open(p0[1], p1[1]) {
        return None;
    }
    let dy = p1[1] - p0[1];
    if dy.abs() <= f32::EPSILON {
        return None;
    }
    let t = -p0[1] / dy;
    Some(p0[0] + (p1[0] - p0[0]) * t)
}

fn vertical_line_intersection(p0: [f32; 2], p1: [f32; 2]) -> Option<f32> {
    if !crosses_zero_half_open(p0[0], p1[0]) {
        return None;
    }
    let dx = p1[0] - p0[0];
    if dx.abs() <= f32::EPSILON {
        return None;
    }
    let t = -p0[0] / dx;
    Some(p0[1] + (p1[1] - p0[1]) * t)
}

fn crosses_zero_half_open(a: f32, b: f32) -> bool {
    (a <= 0.0 && b > 0.0) || (b <= 0.0 && a > 0.0)
}

fn accumulate_horizontal_curve_coverage(
    curve: &QuadraticCurve,
    render_coord: [f32; 2],
    pixels_per_em: f32,
    xcov: &mut f32,
    xwgt: &mut f32,
) {
    if should_use_degenerate_line_fallback(curve) {
        apply_degenerate_horizontal_coverage(curve, render_coord, pixels_per_em, xcov, xwgt);
        return;
    }

    let p12 = [
        curve.p0[0] - render_coord[0],
        curve.p0[1] - render_coord[1],
        curve.p1[0] - render_coord[0],
        curve.p1[1] - render_coord[1],
    ];
    let p3 = [curve.p2[0] - render_coord[0], curve.p2[1] - render_coord[1]];
    let hcode = calc_root_code(p12[1], p12[3], p3[1]);
    if hcode == 0 {
        return;
    }

    let hr = solve_horiz_poly(p12, p3);
    if (hcode & 1) != 0 {
        let sample = saturate((hr[0] * pixels_per_em) + 0.5);
        *xcov += sample;
        *xwgt = (*xwgt).max(saturate(1.0 - (hr[0] * pixels_per_em).abs() * 2.0));
    }
    if hcode > 1 {
        let sample = saturate((hr[1] * pixels_per_em) + 0.5);
        *xcov -= sample;
        *xwgt = (*xwgt).max(saturate(1.0 - (hr[1] * pixels_per_em).abs() * 2.0));
    }
}

fn accumulate_vertical_curve_coverage(
    curve: &QuadraticCurve,
    render_coord: [f32; 2],
    pixels_per_em: f32,
    ycov: &mut f32,
    ywgt: &mut f32,
) {
    if should_use_degenerate_line_fallback(curve) {
        apply_degenerate_vertical_coverage(curve, render_coord, pixels_per_em, ycov, ywgt);
        return;
    }

    let p12 = [
        curve.p0[0] - render_coord[0],
        curve.p0[1] - render_coord[1],
        curve.p1[0] - render_coord[0],
        curve.p1[1] - render_coord[1],
    ];
    let p3 = [curve.p2[0] - render_coord[0], curve.p2[1] - render_coord[1]];
    let vcode = calc_root_code(p12[0], p12[2], p3[0]);
    if vcode == 0 {
        return;
    }

    let vr = solve_vert_poly(p12, p3);
    if (vcode & 1) != 0 {
        let sample = saturate((vr[0] * pixels_per_em) + 0.5);
        *ycov -= sample;
        *ywgt = (*ywgt).max(saturate(1.0 - (vr[0] * pixels_per_em).abs() * 2.0));
    }
    if vcode > 1 {
        let sample = saturate((vr[1] * pixels_per_em) + 0.5);
        *ycov += sample;
        *ywgt = (*ywgt).max(saturate(1.0 - (vr[1] * pixels_per_em).abs() * 2.0));
    }
}

fn calc_root_code(y1: f32, y2: f32, y3: f32) -> u32 {
    let i1 = y1.to_bits() >> 31;
    let i2 = y2.to_bits() >> 30;
    let i3 = y3.to_bits() >> 29;
    let mut shift = (i2 & 2) | (i1 & !2);
    shift = (i3 & 4) | (shift & !4);
    (0x2E74_u32 >> shift) & 0x0101
}

fn solve_horiz_poly(p12: [f32; 4], p3: [f32; 2]) -> [f32; 2] {
    let a = [
        p12[0] - (p12[2] * 2.0) + p3[0],
        p12[1] - (p12[3] * 2.0) + p3[1],
    ];
    let b = [p12[0] - p12[2], p12[1] - p12[3]];
    let ra = 1.0 / a[1];
    let rb = 0.5 / b[1];
    let d = (b[1] * b[1] - a[1] * p12[1]).max(0.0).sqrt();
    let mut t1 = (b[1] - d) * ra;
    let mut t2 = (b[1] + d) * ra;
    if a[1].abs() < 1.0 / 65536.0 {
        t1 = p12[1] * rb;
        t2 = t1;
    }
    [
        ((a[0] * t1) - (b[0] * 2.0)) * t1 + p12[0],
        ((a[0] * t2) - (b[0] * 2.0)) * t2 + p12[0],
    ]
}

fn solve_vert_poly(p12: [f32; 4], p3: [f32; 2]) -> [f32; 2] {
    let a = [
        p12[0] - (p12[2] * 2.0) + p3[0],
        p12[1] - (p12[3] * 2.0) + p3[1],
    ];
    let b = [p12[0] - p12[2], p12[1] - p12[3]];
    let ra = 1.0 / a[0];
    let rb = 0.5 / b[0];
    let d = (b[0] * b[0] - a[0] * p12[0]).max(0.0).sqrt();
    let mut t1 = (b[0] - d) * ra;
    let mut t2 = (b[0] + d) * ra;
    if a[0].abs() < 1.0 / 65536.0 {
        t1 = p12[0] * rb;
        t2 = t1;
    }
    [
        ((a[1] * t1) - (b[1] * 2.0)) * t1 + p12[1],
        ((a[1] * t2) - (b[1] * 2.0)) * t2 + p12[1],
    ]
}

fn calc_coverage(xcov: f32, ycov: f32, xwgt: f32, ywgt: f32) -> f32 {
    ((xcov * xwgt + ycov * ywgt).abs() / (xwgt + ywgt).max(1.0 / 65536.0))
        .max(xcov.abs().min(ycov.abs()))
        .clamp(0.0, 1.0)
}

fn saturate(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

#[derive(Default)]
struct QuadraticCurveBuilder {
    curves: Vec<QuadraticCurve>,
    start: Option<[f32; 2]>,
    current: Option<[f32; 2]>,
}

impl QuadraticCurveBuilder {
    fn push_line(&mut self, to: [f32; 2]) {
        if let Some(from) = self.current {
            let midpoint = [(from[0] + to[0]) * 0.5, (from[1] + to[1]) * 0.5];
            self.curves.push(QuadraticCurve {
                p0: from,
                p1: midpoint,
                p2: to,
            });
            self.current = Some(to);
        }
    }

    fn append_quadratic(&mut self, from: [f32; 2], control: [f32; 2], to: [f32; 2]) {
        self.curves.push(QuadraticCurve {
            p0: from,
            p1: control,
            p2: to,
        });
    }

    fn append_cubic_as_quadratics(
        &mut self,
        p0: [f32; 2],
        p1: [f32; 2],
        p2: [f32; 2],
        p3: [f32; 2],
        depth: u32,
    ) {
        let q1_from_p1 = [((3.0 * p1[0]) - p0[0]) * 0.5, ((3.0 * p1[1]) - p0[1]) * 0.5];
        let q1_from_p2 = [((3.0 * p2[0]) - p3[0]) * 0.5, ((3.0 * p2[1]) - p3[1]) * 0.5];
        let error = (q1_from_p1[0] - q1_from_p2[0])
            .abs()
            .max((q1_from_p1[1] - q1_from_p2[1]).abs());

        if error <= 0.25 || depth >= 8 {
            let control = [
                (q1_from_p1[0] + q1_from_p2[0]) * 0.5,
                (q1_from_p1[1] + q1_from_p2[1]) * 0.5,
            ];
            self.append_quadratic(p0, control, p3);
            return;
        }

        let p01 = [(p0[0] + p1[0]) * 0.5, (p0[1] + p1[1]) * 0.5];
        let p12 = [(p1[0] + p2[0]) * 0.5, (p1[1] + p2[1]) * 0.5];
        let p23 = [(p2[0] + p3[0]) * 0.5, (p2[1] + p3[1]) * 0.5];
        let p01_12 = [(p01[0] + p12[0]) * 0.5, (p01[1] + p12[1]) * 0.5];
        let p12_23 = [(p12[0] + p23[0]) * 0.5, (p12[1] + p23[1]) * 0.5];
        let midpoint = [(p01_12[0] + p12_23[0]) * 0.5, (p01_12[1] + p12_23[1]) * 0.5];

        self.append_cubic_as_quadratics(p0, p01, p01_12, midpoint, depth + 1);
        self.append_cubic_as_quadratics(midpoint, p12_23, p23, p3, depth + 1);
    }
}

impl OutlineBuilder for QuadraticCurveBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        let point = [x, y];
        self.start = Some(point);
        self.current = Some(point);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.push_line([x, y]);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        if let Some(from) = self.current {
            self.append_quadratic(from, [x1, y1], [x, y]);
            self.current = Some([x, y]);
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        if let Some(from) = self.current {
            self.append_cubic_as_quadratics(from, [x1, y1], [x2, y2], [x, y], 0);
            self.current = Some([x, y]);
        }
    }

    fn close(&mut self) {
        if let (Some(current), Some(start)) = (self.current, self.start) {
            if current != start {
                self.push_line(start);
            }
        }
    }
}

fn create_device() -> eyre::Result<(IDXGIFactory4, ID3D12Device, Option<IDXGIInfoQueue>)> {
    let mut dxgi_flags = DXGI_CREATE_FACTORY_FLAGS(0);
    let mut dxgi_info_queue = None;
    if cfg!(debug_assertions) {
        unsafe {
            let mut debug_enabled = false;
            let mut debug1 = None;
            if D3D12GetDebugInterface::<ID3D12Debug1>(&mut debug1).is_ok() {
                if let Some(debug1) = debug1 {
                    let gpu_validation_enabled = std::env::var_os(TEAMY_D3D12_GPU_VALIDATION_ENV)
                        .is_some_and(|value| !value.is_empty() && value != "0");
                    if gpu_validation_enabled {
                        info!(
                            env = TEAMY_D3D12_GPU_VALIDATION_ENV,
                            "enabled D3D12 debug layer with GPU-based validation"
                        );
                        debug1.SetEnableGPUBasedValidation(true);
                    } else {
                        info!(
                            env = TEAMY_D3D12_GPU_VALIDATION_ENV,
                            "enabled D3D12 debug layer without GPU-based validation"
                        );
                    }
                    debug1.EnableDebugLayer();
                    dxgi_flags |= DXGI_CREATE_FACTORY_DEBUG;
                    debug_enabled = true;
                }
            } else {
                let mut debug = None;
                if D3D12GetDebugInterface::<ID3D12Debug>(&mut debug).is_ok() {
                    if let Some(debug) = debug {
                        info!("enabled D3D12 debug layer");
                        debug.EnableDebugLayer();
                        dxgi_flags |= DXGI_CREATE_FACTORY_DEBUG;
                        debug_enabled = true;
                    }
                } else {
                    warn!("D3D12 debug layer unavailable");
                }
            }

            if debug_enabled {
                match DXGIGetDebugInterface1::<IDXGIInfoQueue>(0) {
                    Ok(queue) => {
                        let _ = queue.SetBreakOnSeverity(
                            DXGI_DEBUG_ALL,
                            DXGI_INFO_QUEUE_MESSAGE_SEVERITY_CORRUPTION,
                            true,
                        );
                        let _ = queue.SetBreakOnSeverity(
                            DXGI_DEBUG_ALL,
                            DXGI_INFO_QUEUE_MESSAGE_SEVERITY_ERROR,
                            true,
                        );
                        queue.ClearStoredMessages(DXGI_DEBUG_ALL);
                        info!("acquired DXGI info queue");
                        dxgi_info_queue = Some(queue);
                    }
                    Err(error) => {
                        warn!(?error, "failed to acquire DXGI info queue");
                    }
                }
            }
        }
    }

    let dxgi_factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(dxgi_flags) }?;
    let adapter = get_hardware_adapter(&dxgi_factory)?;

    let mut device = None;
    unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }?;
    let device = device.expect("device should be initialized after D3D12CreateDevice succeeds");
    Ok((dxgi_factory, device, dxgi_info_queue))
}

fn get_hardware_adapter(factory: &IDXGIFactory4) -> eyre::Result<IDXGIAdapter1> {
    for index in 0.. {
        let adapter = match unsafe { factory.EnumAdapters1(index) } {
            Ok(adapter) => adapter,
            Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(error) => return Err(error.into()),
        };

        let description = unsafe { adapter.GetDesc1() }?;
        let is_software = (DXGI_ADAPTER_FLAG(description.Flags as i32)
            & DXGI_ADAPTER_FLAG_SOFTWARE)
            != DXGI_ADAPTER_FLAG_NONE;
        if is_software {
            continue;
        }

        let mut test_device: Option<ID3D12Device> = None;
        if unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut test_device) }.is_ok()
        {
            return Ok(adapter);
        }
    }

    Err(Error::new(E_FAIL, "no suitable D3D12 adapter found").into())
}

fn create_command_queue(device: &ID3D12Device) -> eyre::Result<ID3D12CommandQueue> {
    Ok(unsafe {
        device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
            Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
            ..Default::default()
        })?
    })
}

fn create_command_allocators(
    device: &ID3D12Device,
) -> eyre::Result<[ID3D12CommandAllocator; FRAME_COUNT]> {
    let mut allocators = std::array::from_fn(|_| None::<ID3D12CommandAllocator>);
    for slot in &mut allocators {
        *slot = Some(unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }?);
    }

    Ok(allocators.map(Option::unwrap))
}

fn create_closed_command_list(
    device: &ID3D12Device,
    command_allocator: &ID3D12CommandAllocator,
    pipeline_state: &ID3D12PipelineState,
) -> eyre::Result<ID3D12GraphicsCommandList> {
    let command_list: ID3D12GraphicsCommandList = unsafe {
        device.CreateCommandList(
            0,
            D3D12_COMMAND_LIST_TYPE_DIRECT,
            command_allocator,
            pipeline_state,
        )
    }?;
    unsafe { command_list.Close()? };
    Ok(command_list)
}

fn create_swap_chain(
    factory: &IDXGIFactory4,
    command_queue: &ID3D12CommandQueue,
    hwnd: HWND,
    width: u32,
    height: u32,
) -> eyre::Result<IDXGISwapChain3> {
    let description = DXGI_SWAP_CHAIN_DESC1 {
        Width: width,
        Height: height,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        Stereo: false.into(),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: FRAME_COUNT as u32,
        Scaling: DXGI_SCALING_STRETCH,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        AlphaMode: DXGI_ALPHA_MODE_IGNORE,
        Flags: DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0 as u32,
    };

    let swap_chain: IDXGISwapChain1 =
        unsafe { factory.CreateSwapChainForHwnd(command_queue, hwnd, &description, None, None)? };
    Ok(swap_chain.cast()?)
}

fn client_size(hwnd: HWND) -> eyre::Result<(u32, u32)> {
    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rect) }.wrap_err("failed to query client size")?;
    let width = (rect.right - rect.left).max(0) as u32;
    let height = (rect.bottom - rect.top).max(0) as u32;
    Ok((width, height))
}

fn create_render_targets(
    device: &ID3D12Device,
    swap_chain: &IDXGISwapChain3,
) -> eyre::Result<(ID3D12DescriptorHeap, u32, [ID3D12Resource; FRAME_COUNT])> {
    let rtv_heap = create_empty_rtv_heap(device)?;
    let rtv_descriptor_size =
        unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV) };
    let heap_start = unsafe { rtv_heap.GetCPUDescriptorHandleForHeapStart() };

    let mut render_targets = std::array::from_fn(|_| None::<ID3D12Resource>);
    for (index, slot) in render_targets.iter_mut().enumerate() {
        let resource: ID3D12Resource = unsafe { swap_chain.GetBuffer(index as u32) }?;
        let descriptor = D3D12_CPU_DESCRIPTOR_HANDLE {
            ptr: heap_start.ptr + index * rtv_descriptor_size as usize,
        };
        unsafe { device.CreateRenderTargetView(&resource, None, descriptor) };
        *slot = Some(resource);
    }

    Ok((
        rtv_heap,
        rtv_descriptor_size,
        render_targets.map(Option::unwrap),
    ))
}

fn create_empty_rtv_heap(device: &ID3D12Device) -> eyre::Result<ID3D12DescriptorHeap> {
    Ok(unsafe {
        device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
            NumDescriptors: FRAME_COUNT as u32,
            ..Default::default()
        })?
    })
}

fn create_root_signature(device: &ID3D12Device) -> eyre::Result<ID3D12RootSignature> {
    let descriptor_ranges = [D3D12_DESCRIPTOR_RANGE {
        RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
        NumDescriptors: 2,
        BaseShaderRegister: 0,
        RegisterSpace: 0,
        OffsetInDescriptorsFromTableStart: D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND,
    }];
    let root_parameters = [
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                Descriptor: D3D12_ROOT_DESCRIPTOR {
                    ShaderRegister: 0,
                    RegisterSpace: 0,
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        },
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                    NumDescriptorRanges: descriptor_ranges.len() as u32,
                    pDescriptorRanges: descriptor_ranges.as_ptr(),
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
        },
    ];
    let description = D3D12_ROOT_SIGNATURE_DESC {
        NumParameters: root_parameters.len() as u32,
        pParameters: root_parameters.as_ptr(),
        Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
        ..Default::default()
    };

    let mut signature = None;
    let mut error = None;
    unsafe {
        D3D12SerializeRootSignature(
            &description,
            D3D_ROOT_SIGNATURE_VERSION_1,
            &mut signature,
            Some(&mut error),
        )
    }
    .map_err(|err| shader_error(err, error).wrap_err("failed to serialize root signature"))?;

    let signature = signature.expect("root signature blob should be initialized");
    Ok(unsafe {
        device.CreateRootSignature(
            0,
            std::slice::from_raw_parts(
                signature.GetBufferPointer() as *const u8,
                signature.GetBufferSize(),
            ),
        )?
    })
}

fn create_pipeline_state(
    device: &ID3D12Device,
    root_signature: &ID3D12RootSignature,
) -> eyre::Result<ID3D12PipelineState> {
    let compile_flags = if cfg!(debug_assertions) {
        D3DCOMPILE_DEBUG | D3DCOMPILE_SKIP_OPTIMIZATION
    } else {
        0
    };

    let shader_path = shader_path();
    let shader_path_hstring: HSTRING = shader_path.to_string_lossy().as_ref().into();
    let vertex_shader = compile_shader(
        &shader_path_hstring,
        s!("VSMain"),
        s!("vs_5_0"),
        compile_flags,
    )?;
    let pixel_shader = compile_shader(
        &shader_path_hstring,
        s!("PSMain"),
        s!("ps_5_0"),
        compile_flags,
    )?;

    let input_layout = [
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("POSITION"),
            Format: DXGI_FORMAT_R32G32B32_FLOAT,
            ..Default::default()
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("COLOR"),
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            AlignedByteOffset: 12,
            ..Default::default()
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("TEXCOORD"),
            Format: DXGI_FORMAT_R32G32_FLOAT,
            AlignedByteOffset: 28,
            ..Default::default()
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("EFFECT"),
            Format: DXGI_FORMAT_R32_FLOAT,
            AlignedByteOffset: 36,
            ..Default::default()
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("GLYPH"),
            Format: DXGI_FORMAT_R32_FLOAT,
            AlignedByteOffset: 40,
            ..Default::default()
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("GLYPHDATA"),
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            AlignedByteOffset: 44,
            ..Default::default()
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("BANDING"),
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            AlignedByteOffset: 60,
            ..Default::default()
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("NORMAL"),
            Format: DXGI_FORMAT_R32G32_FLOAT,
            AlignedByteOffset: 76,
            ..Default::default()
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("JACOBIAN"),
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            AlignedByteOffset: 84,
            ..Default::default()
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("VIEWPORT"),
            Format: DXGI_FORMAT_R32G32_FLOAT,
            AlignedByteOffset: 100,
            ..Default::default()
        },
    ];

    let blend_target = D3D12_RENDER_TARGET_BLEND_DESC {
        BlendEnable: TRUE,
        LogicOpEnable: false.into(),
        SrcBlend: D3D12_BLEND_SRC_ALPHA,
        DestBlend: D3D12_BLEND_INV_SRC_ALPHA,
        BlendOp: D3D12_BLEND_OP_ADD,
        SrcBlendAlpha: D3D12_BLEND_ONE,
        DestBlendAlpha: D3D12_BLEND_INV_SRC_ALPHA,
        BlendOpAlpha: D3D12_BLEND_OP_ADD,
        LogicOp: D3D12_LOGIC_OP_NOOP,
        RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
    };

    let description = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
        pRootSignature: std::mem::ManuallyDrop::new(Some(root_signature.clone())),
        VS: shader_bytecode(&vertex_shader),
        PS: shader_bytecode(&pixel_shader),
        BlendState: D3D12_BLEND_DESC {
            AlphaToCoverageEnable: false.into(),
            IndependentBlendEnable: false.into(),
            RenderTarget: [blend_target; 8],
        },
        SampleMask: u32::MAX,
        RasterizerState: D3D12_RASTERIZER_DESC {
            FillMode: D3D12_FILL_MODE_SOLID,
            CullMode: D3D12_CULL_MODE_NONE,
            FrontCounterClockwise: false.into(),
            DepthBias: D3D12_DEFAULT_DEPTH_BIAS,
            DepthBiasClamp: D3D12_DEFAULT_DEPTH_BIAS_CLAMP,
            SlopeScaledDepthBias: D3D12_DEFAULT_SLOPE_SCALED_DEPTH_BIAS,
            DepthClipEnable: TRUE,
            MultisampleEnable: false.into(),
            AntialiasedLineEnable: false.into(),
            ForcedSampleCount: 0,
            ConservativeRaster: D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF,
        },
        DepthStencilState: D3D12_DEPTH_STENCIL_DESC {
            DepthEnable: false.into(),
            StencilEnable: false.into(),
            ..Default::default()
        },
        InputLayout: D3D12_INPUT_LAYOUT_DESC {
            pInputElementDescs: input_layout.as_ptr(),
            NumElements: input_layout.len() as u32,
        },
        PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
        NumRenderTargets: 1,
        RTVFormats: [
            DXGI_FORMAT_B8G8R8A8_UNORM,
            DXGI_FORMAT_UNKNOWN,
            DXGI_FORMAT_UNKNOWN,
            DXGI_FORMAT_UNKNOWN,
            DXGI_FORMAT_UNKNOWN,
            DXGI_FORMAT_UNKNOWN,
            DXGI_FORMAT_UNKNOWN,
            DXGI_FORMAT_UNKNOWN,
        ],
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        ..Default::default()
    };

    Ok(unsafe { device.CreateGraphicsPipelineState(&description) }?)
}

fn compile_shader(
    path: &HSTRING,
    entry_point: PCSTR,
    target: PCSTR,
    flags: u32,
) -> eyre::Result<ID3DBlob> {
    let mut shader = None;
    let mut error = None;
    unsafe {
        D3DCompileFromFile(
            path,
            None,
            None,
            entry_point,
            target,
            flags,
            0,
            &mut shader,
            Some(&mut error),
        )
    }
    .map_err(|err| shader_error(err, error))?;

    Ok(shader.expect("shader blob should be initialized"))
}

fn shader_error(error: windows::core::Error, blob: Option<ID3DBlob>) -> eyre::Error {
    if let Some(blob) = blob {
        let bytes = unsafe {
            std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())
        };
        eyre::eyre!("{error}: {}", String::from_utf8_lossy(bytes).trim())
    } else {
        error.into()
    }
}

fn shader_bytecode(shader: &ID3DBlob) -> D3D12_SHADER_BYTECODE {
    D3D12_SHADER_BYTECODE {
        pShaderBytecode: unsafe { shader.GetBufferPointer() },
        BytecodeLength: unsafe { shader.GetBufferSize() },
    }
}

fn shader_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("app")
        .join("windows_panel_shaders.hlsl")
}

/// behavior[impl window.appearance.panel-borders.absolute-pixels]
/// behavior[impl window.appearance.backgrounds.animated-time-based]
fn build_shader_params(width: f32, height: f32, elapsed_seconds: f32) -> ShaderParams {
    let safe_width = width.max(1.0);
    let safe_height = height.max(1.0);
    ShaderParams {
        slug_matrix: [
            [2.0 / safe_width, 0.0, 0.0, -1.0],
            [0.0, -2.0 / safe_height, 0.0, 1.0],
            [0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        slug_viewport: [safe_width, safe_height, 0.0, 0.0],
        scene_time: [elapsed_seconds, 0.0, 0.0, 0.0],
    }
}

fn create_vertex_buffer(
    device: &ID3D12Device,
) -> eyre::Result<(ID3D12Resource, D3D12_VERTEX_BUFFER_VIEW)> {
    let buffer_size = (std::mem::size_of::<Vertex>() * MAX_VERTEX_COUNT) as u64;

    let mut vertex_buffer = None;
    unsafe {
        device.CreateCommittedResource(
            &D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_UPLOAD,
                ..Default::default()
            },
            D3D12_HEAP_FLAG_NONE,
            &D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                Width: buffer_size,
                Height: 1,
                DepthOrArraySize: 1,
                MipLevels: 1,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                ..Default::default()
            },
            D3D12_RESOURCE_STATE_GENERIC_READ,
            None,
            &mut vertex_buffer,
        )?
    };
    let vertex_buffer: ID3D12Resource = vertex_buffer.expect("vertex buffer should be initialized");

    Ok((
        vertex_buffer.clone(),
        D3D12_VERTEX_BUFFER_VIEW {
            BufferLocation: unsafe { vertex_buffer.GetGPUVirtualAddress() },
            SizeInBytes: buffer_size as u32,
            StrideInBytes: std::mem::size_of::<Vertex>() as u32,
        },
    ))
}

fn create_shader_param_buffer(device: &ID3D12Device) -> eyre::Result<ID3D12Resource> {
    let buffer_size = 256_u64;
    let mut shader_param_buffer = None;
    unsafe {
        device.CreateCommittedResource(
            &D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_UPLOAD,
                ..Default::default()
            },
            D3D12_HEAP_FLAG_NONE,
            &D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                Width: buffer_size,
                Height: 1,
                DepthOrArraySize: 1,
                MipLevels: 1,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                ..Default::default()
            },
            D3D12_RESOURCE_STATE_GENERIC_READ,
            None,
            &mut shader_param_buffer,
        )?
    };
    Ok(shader_param_buffer.expect("shader parameter buffer should be initialized"))
}

fn create_slug_buffers_and_srv(
    device: &ID3D12Device,
) -> eyre::Result<(ID3D12DescriptorHeap, ID3D12Resource, ID3D12Resource)> {
    let curve_data = vec![[0.0_f32; 4]; MAX_CURVE_FLOAT4_COUNT];
    let byte_len = (curve_data.len() * std::mem::size_of::<[f32; 4]>()) as u64;
    let band_data = vec![0_u32; MAX_BAND_UINT_COUNT];
    let band_byte_len = (band_data.len() * std::mem::size_of::<u32>()) as u64;

    let mut curve_buffer = None;
    unsafe {
        device.CreateCommittedResource(
            &D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_UPLOAD,
                ..Default::default()
            },
            D3D12_HEAP_FLAG_NONE,
            &D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                Width: byte_len,
                Height: 1,
                DepthOrArraySize: 1,
                MipLevels: 1,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                ..Default::default()
            },
            D3D12_RESOURCE_STATE_GENERIC_READ,
            None,
            &mut curve_buffer,
        )?
    };
    let curve_buffer: ID3D12Resource = curve_buffer.expect("curve buffer should be initialized");

    let mut band_buffer = None;
    unsafe {
        device.CreateCommittedResource(
            &D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_UPLOAD,
                ..Default::default()
            },
            D3D12_HEAP_FLAG_NONE,
            &D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                Width: band_byte_len,
                Height: 1,
                DepthOrArraySize: 1,
                MipLevels: 1,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                ..Default::default()
            },
            D3D12_RESOURCE_STATE_GENERIC_READ,
            None,
            &mut band_buffer,
        )?
    };
    let band_buffer: ID3D12Resource = band_buffer.expect("band buffer should be initialized");

    unsafe {
        let mut mapped = std::ptr::null_mut();
        curve_buffer.Map(0, None, Some(&mut mapped))?;
        std::ptr::copy_nonoverlapping(
            curve_data.as_ptr(),
            mapped as *mut [f32; 4],
            curve_data.len(),
        );
        curve_buffer.Unmap(0, None);

        let mut band_mapped = std::ptr::null_mut();
        band_buffer.Map(0, None, Some(&mut band_mapped))?;
        std::ptr::copy_nonoverlapping(band_data.as_ptr(), band_mapped as *mut u32, band_data.len());
        band_buffer.Unmap(0, None);
    }

    let srv_heap: ID3D12DescriptorHeap = unsafe {
        device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            NumDescriptors: 2,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
            ..Default::default()
        })?
    };
    let descriptor_size =
        unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) }
            as usize;

    let curve_desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
        ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
            Buffer: D3D12_BUFFER_SRV {
                FirstElement: 0,
                NumElements: curve_data.len() as u32,
                StructureByteStride: 0,
                Flags: D3D12_BUFFER_SRV_FLAG_NONE,
            },
        },
    };

    let band_desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_R32_UINT,
        ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
            Buffer: D3D12_BUFFER_SRV {
                FirstElement: 0,
                NumElements: band_data.len() as u32,
                StructureByteStride: 0,
                Flags: D3D12_BUFFER_SRV_FLAG_NONE,
            },
        },
    };

    unsafe {
        let heap_start = srv_heap.GetCPUDescriptorHandleForHeapStart();
        device.CreateShaderResourceView(&curve_buffer, Some(&curve_desc), heap_start);
        device.CreateShaderResourceView(
            &band_buffer,
            Some(&band_desc),
            D3D12_CPU_DESCRIPTOR_HANDLE {
                ptr: heap_start.ptr + descriptor_size,
            },
        );
    }

    Ok((srv_heap, curve_buffer, band_buffer))
}

fn append_text_rect(
    vertices: &mut Vec<Vertex>,
    rect: RECT,
    color: [f32; 4],
    glyph: SlugGlyph,
    font: &LoadedTerminalFont,
) {
    if vertices.len() + 6 > MAX_VERTEX_COUNT {
        return;
    }

    let left = rect.left as f32;
    let top = rect.top as f32;
    let glyph_data = [
        glyph.curve_start as f32,
        glyph.curve_count as f32,
        glyph.band_count_x.saturating_sub(1) as f32,
        glyph.band_count_y.saturating_sub(1) as f32,
    ];
    let banding = glyph.band_transform;
    let screen_width = (rect.right - rect.left) as f32;
    let screen_height = (rect.bottom - rect.top) as f32;
    let advance = glyph.advance.max(1.0);
    let font_height = font.units_per_em.max(1.0);
    let glyph_left = left + (glyph.x_min / advance) * screen_width;
    let glyph_right = left + (glyph.x_max / advance) * screen_width;
    let glyph_top = top + ((font.ascender - glyph.y_max) / font_height) * screen_height;
    let glyph_bottom = top + ((font.ascender - glyph.y_min) / font_height) * screen_height;
    let jacobian = [
        advance / screen_width.max(1.0),
        0.0,
        0.0,
        -font_height / screen_height.max(1.0),
    ];
    let effect = PanelEffect::Text as u32 as f32;

    let top_left = Vertex {
        position: [glyph_left, glyph_top, 0.0],
        color,
        uv: [glyph.x_min, glyph.y_max],
        effect,
        glyph: glyph.band_start as f32,
        glyph_data,
        banding,
        normal: [-1.0, 1.0],
        jacobian,
        _padding: [0.0; 2],
    };
    let top_right = Vertex {
        position: [glyph_right, glyph_top, 0.0],
        color,
        uv: [glyph.x_max, glyph.y_max],
        effect,
        glyph: glyph.band_start as f32,
        glyph_data,
        banding,
        normal: [1.0, 1.0],
        jacobian,
        _padding: [0.0; 2],
    };
    let bottom_right = Vertex {
        position: [glyph_right, glyph_bottom, 0.0],
        color,
        uv: [glyph.x_max, glyph.y_min],
        effect,
        glyph: glyph.band_start as f32,
        glyph_data,
        banding,
        normal: [1.0, -1.0],
        jacobian,
        _padding: [0.0; 2],
    };
    let bottom_left = Vertex {
        position: [glyph_left, glyph_bottom, 0.0],
        color,
        uv: [glyph.x_min, glyph.y_min],
        effect,
        glyph: glyph.band_start as f32,
        glyph_data,
        banding,
        normal: [-1.0, -1.0],
        jacobian,
        _padding: [0.0; 2],
    };

    vertices.extend_from_slice(&[
        top_left,
        top_right,
        bottom_right,
        top_left,
        bottom_right,
        bottom_left,
    ]);
}

fn append_rect(
    vertices: &mut Vec<Vertex>,
    rect: RECT,
    color: [f32; 4],
    effect: u32,
    glyph_index: u32,
) {
    if vertices.len() + 6 > MAX_VERTEX_COUNT {
        return;
    }

    let left = rect.left as f32;
    let top = rect.top as f32;
    let right = rect.right as f32;
    let bottom = rect.bottom as f32;
    let effect = effect as f32;
    let glyph = glyph_index as f32;

    let top_left = Vertex {
        position: [left, top, 0.0],
        color,
        uv: [0.0, 0.0],
        effect,
        glyph,
        glyph_data: [0.0; 4],
        banding: [0.0; 4],
        normal: [0.0; 2],
        jacobian: [0.0; 4],
        _padding: [0.0; 2],
    };
    let top_right = Vertex {
        position: [right, top, 0.0],
        color,
        uv: [1.0, 0.0],
        effect,
        glyph,
        glyph_data: [0.0; 4],
        banding: [0.0; 4],
        normal: [0.0; 2],
        jacobian: [0.0; 4],
        _padding: [0.0; 2],
    };
    let bottom_right = Vertex {
        position: [right, bottom, 0.0],
        color,
        uv: [1.0, 1.0],
        effect,
        glyph,
        glyph_data: [0.0; 4],
        banding: [0.0; 4],
        normal: [0.0; 2],
        jacobian: [0.0; 4],
        _padding: [0.0; 2],
    };
    let bottom_left = Vertex {
        position: [left, bottom, 0.0],
        color,
        uv: [0.0, 1.0],
        effect,
        glyph,
        glyph_data: [0.0; 4],
        banding: [0.0; 4],
        normal: [0.0; 2],
        jacobian: [0.0; 4],
        _padding: [0.0; 2],
    };

    vertices.extend_from_slice(&[
        top_left,
        top_right,
        bottom_right,
        top_left,
        bottom_right,
        bottom_left,
    ]);
}

fn transition_barrier(
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: std::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: std::mem::ManuallyDrop::new(Some(resource.clone())),
                StateBefore: before,
                StateAfter: after,
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            }),
        },
    }
}

fn issue_transition_barrier(
    command_list: &ID3D12GraphicsCommandList,
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) {
    let mut barriers = [transition_barrier(resource, before, after)];
    unsafe {
        command_list.ResourceBarrier(&barriers);

        let transition = &mut barriers[0].Anonymous.Transition;
        let resource = std::mem::ManuallyDrop::take(&mut transition.pResource);
        drop(resource);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CachedSceneVertices, FALLBACK_GLYPH, PanelEffect, RenderScene, Vertex, append_rect,
        append_slug_band_data, build_panel_scene, build_shader_params,
        can_reuse_cached_scene_vertices, collect_scene_chars, cpu_slug_coverage,
        cpu_slug_coverage_all_curves, dirty_fragment_ranges, extract_glyph_curves,
        fragment_ranges_match, fragment_vertex_ranges, load_terminal_font, push_centered_text,
        push_glyph, push_overlay_panel, push_panel, push_text_block,
        render_snapshot_glyph_into_image,
    };
    use crate::app::windows_terminal::TerminalLayout;
    use eyre::WrapErr;
    use image::RgbaImage;
    use ttf_parser::{Face, OutlineBuilder};
    use windows::Win32::Foundation::RECT;

    #[test]
    fn push_text_block_emits_visible_glyphs() {
        let mut scene = RenderScene {
            panels: Vec::new(),
            glyphs: Vec::new(),
            overlay_panels: Vec::new(),
        };
        push_text_block(
            &mut scene,
            RECT {
                left: 0,
                top: 0,
                right: 100,
                bottom: 40,
            },
            "A B",
            8,
            16,
            [1.0, 1.0, 1.0, 1.0],
        );

        assert_eq!(scene.glyphs.len(), 2);
    }

    #[test]
    fn push_centered_text_places_a_glyph() {
        let mut scene = RenderScene {
            panels: Vec::new(),
            glyphs: Vec::new(),
            overlay_panels: Vec::new(),
        };
        push_centered_text(
            &mut scene,
            RECT {
                left: 0,
                top: 0,
                right: 80,
                bottom: 60,
            },
            "7",
            [1.0, 1.0, 1.0, 1.0],
        );

        assert_eq!(scene.glyphs.len(), 1);
    }

    #[test]
    fn cached_scene_vertices_are_not_reused_after_glyph_cache_generation_changes() {
        let cached_vertices = CachedSceneVertices {
            glyph_cache_generation: 4,
            vertices: Vec::new(),
        };

        assert!(!can_reuse_cached_scene_vertices(
            true,
            Some(&cached_vertices),
            5,
        ));
    }

    #[test]
    fn cached_scene_vertices_are_reused_when_generation_matches() {
        let cached_vertices = CachedSceneVertices {
            glyph_cache_generation: 4,
            vertices: Vec::new(),
        };

        assert!(can_reuse_cached_scene_vertices(
            true,
            Some(&cached_vertices),
            4,
        ));
    }

    #[test]
    fn fragment_vertex_ranges_follow_fragment_lengths() {
        let vertex = Vertex {
            position: [0.0; 3],
            color: [0.0; 4],
            uv: [0.0; 2],
            effect: 0.0,
            glyph: 0.0,
            glyph_data: [0.0; 4],
            banding: [0.0; 4],
            normal: [0.0; 2],
            jacobian: [0.0; 4],
            _padding: [0.0; 2],
        };
        let fragment_a = vec![vertex];
        let fragment_b = vec![vertex; 3];
        let fragments = vec![fragment_a.as_slice(), fragment_b.as_slice()];

        assert_eq!(fragment_vertex_ranges(&fragments), vec![0..1, 1..4]);
    }

    #[test]
    fn fragment_ranges_match_only_when_fragment_lengths_match() {
        assert!(fragment_ranges_match(&[0..2, 2..5], &[0..2, 2..5]));
        assert!(fragment_ranges_match(&[4..6, 6..9], &[0..2, 2..5]));
        assert!(!fragment_ranges_match(&[0..2, 2..5], &[0..2, 2..6]));
        assert!(!fragment_ranges_match(&[0..2], &[0..2, 2..5]));
    }

    #[test]
    fn dirty_fragment_ranges_patch_and_merge_adjacent_updates() {
        let base_vertex = Vertex {
            position: [0.0; 3],
            color: [0.0; 4],
            uv: [0.0; 2],
            effect: 0.0,
            glyph: 0.0,
            glyph_data: [0.0; 4],
            banding: [0.0; 4],
            normal: [0.0; 2],
            jacobian: [0.0; 4],
            _padding: [0.0; 2],
        };
        let fragment_a = vec![Vertex {
            position: [1.0, 0.0, 0.0],
            ..base_vertex
        }];
        let fragment_b = vec![Vertex {
            position: [2.0, 0.0, 0.0],
            ..base_vertex
        }];
        let fragment_c = vec![Vertex {
            position: [3.0, 0.0, 0.0],
            ..base_vertex
        }];
        let fragments = vec![
            fragment_a.as_slice(),
            fragment_b.as_slice(),
            fragment_c.as_slice(),
        ];
        let ranges = fragment_vertex_ranges(&fragments);
        let mut cached_vertices = vec![base_vertex; 3];

        let dirty_ranges = dirty_fragment_ranges(
            &ranges,
            &fragments,
            &[false, false, true],
            &mut cached_vertices,
        );

        assert_eq!(dirty_ranges, vec![0..2]);
        assert_eq!(cached_vertices[0].position[0], 1.0);
        assert_eq!(cached_vertices[1].position[0], 2.0);
        assert_eq!(cached_vertices[2].position[0], 0.0);
    }

    #[test]
    fn append_rect_preserves_text_effect_and_glyph_index_order() {
        let mut vertices = Vec::new();
        append_rect(
            &mut vertices,
            RECT {
                left: 0,
                top: 0,
                right: 10,
                bottom: 20,
            },
            [1.0, 1.0, 1.0, 1.0],
            PanelEffect::Text as u32,
            u32::from('A'),
        );

        assert_eq!(vertices.len(), 6);
        assert_eq!(vertices[0].effect, PanelEffect::Text as u32 as f32);
        assert_eq!(vertices[0].glyph, u32::from('A') as f32);
    }

    // behavior[verify window.appearance.code-panel.single-surface]
    // os[verify os.windows.rendering.direct3d12]
    #[test]
    fn build_panel_scene_uses_single_code_panel_surface() {
        let layout = TerminalLayout {
            client_width: 1040,
            client_height: 680,
            cell_width: 8,
            cell_height: 16,
        };

        let scene = build_panel_scene(layout);
        let code_panel_count = scene
            .panels
            .iter()
            .filter(|panel| matches!(panel.effect, PanelEffect::CodePanel))
            .count();

        assert_eq!(code_panel_count, 1);
    }

    // behavior[verify window.appearance.backgrounds.blue-half-transparent]
    // os[verify os.windows.rendering.direct3d12]
    #[test]
    fn build_panel_scene_keeps_blue_background_half_transparent() {
        let layout = TerminalLayout {
            client_width: 1040,
            client_height: 680,
            cell_width: 8,
            cell_height: 16,
        };

        let scene = build_panel_scene(layout);
        let blue_panel = scene
            .panels
            .iter()
            .find(|panel| matches!(panel.effect, PanelEffect::BlueBackground))
            .expect("blue background panel should exist");

        assert_eq!(blue_panel.color[3], 0.5);
    }

    // behavior[verify window.appearance.chrome]
    #[test]
    fn build_panel_scene_includes_drag_handle_panel() {
        let layout = TerminalLayout {
            client_width: 1040,
            client_height: 680,
            cell_width: 8,
            cell_height: 16,
        };

        let scene = build_panel_scene(layout);
        let drag_panel_count = scene
            .panels
            .iter()
            .filter(|panel| matches!(panel.effect, PanelEffect::DragHandle))
            .count();

        assert_eq!(drag_panel_count, 1);
    }

    // behavior[verify window.appearance.backgrounds.animated-time-based]
    #[test]
    fn build_shader_params_stores_elapsed_seconds_in_scene_time() {
        let params = build_shader_params(1040.0, 680.0, 12.5);

        assert_eq!(params.scene_time, [12.5, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn collect_scene_atlas_chars_keeps_fallback_and_unicode_glyphs() {
        let mut scene = RenderScene {
            panels: Vec::new(),
            glyphs: Vec::new(),
            overlay_panels: Vec::new(),
        };
        push_glyph(
            &mut scene,
            RECT {
                left: 0,
                top: 0,
                right: 10,
                bottom: 10,
            },
            '❯',
            [1.0, 1.0, 1.0, 1.0],
        );
        push_glyph(
            &mut scene,
            RECT {
                left: 10,
                top: 0,
                right: 20,
                bottom: 10,
            },
            'A',
            [1.0, 1.0, 1.0, 1.0],
        );

        let atlas_chars = collect_scene_chars(&scene);

        assert_eq!(atlas_chars[0], FALLBACK_GLYPH);
        assert!(atlas_chars.contains(&'❯'));
        assert!(atlas_chars.contains(&'A'));
    }

    #[test]
    fn push_overlay_panel_tracks_overlays_separately() {
        let mut scene = RenderScene {
            panels: Vec::new(),
            glyphs: Vec::new(),
            overlay_panels: Vec::new(),
        };

        push_panel(
            &mut scene,
            RECT {
                left: 0,
                top: 0,
                right: 10,
                bottom: 10,
            },
            [1.0, 0.0, 0.0, 1.0],
            PanelEffect::TerminalFill,
        );
        push_overlay_panel(
            &mut scene,
            RECT {
                left: 1,
                top: 1,
                right: 9,
                bottom: 9,
            },
            [0.0, 1.0, 0.0, 1.0],
            PanelEffect::TerminalCursor,
        );

        assert_eq!(scene.panels.len(), 1);
        assert_eq!(scene.overlay_panels.len(), 1);
    }

    #[test]
    fn slash_snapshot_has_single_alpha_span_per_scanline() -> eyre::Result<()> {
        let font = load_terminal_font()?;
        let face = Face::parse(&font.font_bytes, font.face_index)?;
        let glyph_id = face.glyph_index('/').expect("slash glyph should exist");
        let curves = extract_glyph_curves(&face, glyph_id);
        let glyph = super::SlugGlyph {
            curve_start: 0,
            curve_count: curves.len() as u32,
            band_start: 0,
            band_count_x: 1,
            band_count_y: 1,
            band_transform: [0.0; 4],
            x_min: face
                .glyph_bounding_box(glyph_id)
                .map_or(0.0, |rect| f32::from(rect.x_min)),
            y_min: face
                .glyph_bounding_box(glyph_id)
                .map_or(font.descender, |rect| f32::from(rect.y_min)),
            x_max: face
                .glyph_bounding_box(glyph_id)
                .map_or(font.cell_advance, |rect| f32::from(rect.x_max)),
            y_max: face
                .glyph_bounding_box(glyph_id)
                .map_or(font.ascender, |rect| f32::from(rect.y_max)),
            advance: face
                .glyph_hor_advance(glyph_id)
                .map_or(font.cell_advance, f32::from),
        };
        let mut band_data = Vec::new();
        let (band_count_x, band_count_y, band_transform) =
            append_slug_band_data(&curves, glyph, &mut band_data);
        let image = render_test_glyph(
            &font,
            &curves,
            &band_data,
            super::SlugGlyph {
                band_count_x,
                band_count_y,
                band_transform,
                ..glyph
            },
            256,
            512,
            512,
        );

        let multi_span_rows = count_rows_with_multiple_spans(&image);
        assert_eq!(
            multi_span_rows, 0,
            "slash should be convex per occupied scanline"
        );
        assert_eq!(
            count_connected_components(&image, 8),
            1,
            "slash should render as one connected component"
        );
        Ok(())
    }

    #[test]
    fn b_snapshot_left_edge_stays_close_to_fontdue() -> eyre::Result<()> {
        let font = load_terminal_font()?;
        let face = Face::parse(&font.font_bytes, font.face_index)?;
        let glyph_id = face.glyph_index('b').expect("b glyph should exist");
        let curves = extract_glyph_curves(&face, glyph_id);
        let glyph = super::build_slug_glyph_from_face(&font, &face, glyph_id, 0, curves.len());
        let mut band_data = Vec::new();
        let (band_count_x, band_count_y, band_transform) =
            append_slug_band_data(&curves, glyph, &mut band_data);
        let slug = render_test_glyph(
            &font,
            &curves,
            &band_data,
            super::SlugGlyph {
                band_count_x,
                band_count_y,
                band_transform,
                ..glyph
            },
            256,
            512,
            512,
        );
        let fontdue = render_fontdue_reference_glyph('b', 256, 512, 512)?;

        let slug_rows = foreground_row_spans(&slug, 24);
        let fontdue_rows = foreground_row_spans(&fontdue, 24);
        let overlap = slug_rows.len().min(fontdue_rows.len());
        assert!(
            overlap >= 64,
            "expected enough overlapping rows for b comparison"
        );

        let first_delta_sum: i32 = slug_rows
            .iter()
            .zip(fontdue_rows.iter())
            .take(overlap)
            .map(|(lhs, rhs)| (lhs.0 as i32 - rhs.0 as i32).abs())
            .sum();
        let average_first_delta = first_delta_sum as f32 / overlap as f32;

        assert!(
            average_first_delta <= 3.5,
            "b left edge drifted too far from fontdue: average first-edge delta = {average_first_delta}"
        );
        Ok(())
    }

    #[test]
    fn g_and_six_outlines_use_quadratic_segments_in_this_font() -> eyre::Result<()> {
        let font = load_terminal_font()?;
        let face = Face::parse(&font.font_bytes, font.face_index)?;

        for character in ['b', 'r', 'g', '6'] {
            let glyph_id = face
                .glyph_index(character)
                .expect("diagnostic glyph should exist in terminal font");
            let mut builder = SegmentCountingOutlineBuilder::default();
            let _ = face.outline_glyph(glyph_id, &mut builder);

            assert_eq!(
                builder.cubic_segments, 0,
                "{character} unexpectedly uses cubic outlines in the installed terminal font"
            );
            assert!(
                builder.quadratic_segments > 0 || builder.line_segments > 0,
                "{character} should produce outline segments"
            );
        }

        Ok(())
    }

    #[test]
    fn glyph_snapshots_write_debug_artifacts() -> eyre::Result<()> {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let output_dir = manifest_dir
            .join("target")
            .join("test-artifacts")
            .join("slug");

        super::write_slug_snapshot_png('/', 256, 512, 512, &output_dir.join("slash-256.png"))?;
        super::write_slug_snapshot_png('b', 256, 512, 512, &output_dir.join("b-256.png"))?;
        super::write_slug_snapshot_png('r', 256, 512, 512, &output_dir.join("r-256.png"))?;
        super::write_slug_snapshot_png('g', 256, 512, 512, &output_dir.join("g-256.png"))?;
        super::write_slug_snapshot_png('6', 256, 512, 512, &output_dir.join("6-256.png"))?;

        assert!(output_dir.join("slash-256.png").exists());
        assert!(output_dir.join("b-256.png").exists());
        assert!(output_dir.join("r-256.png").exists());
        assert!(output_dir.join("g-256.png").exists());
        assert!(output_dir.join("6-256.png").exists());
        Ok(())
    }

    #[test]
    fn fontdue_reference_snapshots_write_debug_artifacts() -> eyre::Result<()> {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let output_dir = manifest_dir
            .join("target")
            .join("test-artifacts")
            .join("slug");

        write_fontdue_reference_png(
            '/',
            256,
            512,
            512,
            &output_dir.join("slash-fontdue-256.png"),
        )?;
        write_fontdue_reference_png('b', 256, 512, 512, &output_dir.join("b-fontdue-256.png"))?;
        write_fontdue_reference_png('r', 256, 512, 512, &output_dir.join("r-fontdue-256.png"))?;

        assert!(output_dir.join("slash-fontdue-256.png").exists());
        assert!(output_dir.join("b-fontdue-256.png").exists());
        assert!(output_dir.join("r-fontdue-256.png").exists());
        Ok(())
    }

    #[test]
    fn fontdue_comparison_diffs_write_debug_artifacts() -> eyre::Result<()> {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let output_dir = manifest_dir
            .join("target")
            .join("test-artifacts")
            .join("slug");
        let font = load_terminal_font()?;
        let face = Face::parse(&font.font_bytes, font.face_index)?;

        for character in ['/', 'b', 'r'] {
            let glyph_id = face
                .glyph_index(character)
                .expect("comparison glyph should exist in terminal font");
            let curves = extract_glyph_curves(&face, glyph_id);
            let glyph = super::build_slug_glyph_from_face(&font, &face, glyph_id, 0, curves.len());
            let mut band_data = Vec::new();
            let (band_count_x, band_count_y, band_transform) =
                append_slug_band_data(&curves, glyph, &mut band_data);
            let slug = render_test_glyph(
                &font,
                &curves,
                &band_data,
                super::SlugGlyph {
                    band_count_x,
                    band_count_y,
                    band_transform,
                    ..glyph
                },
                256,
                512,
                512,
            );
            let fontdue = render_fontdue_reference_glyph(character, 256, 512, 512)?;
            let diff = render_alpha_diff(&slug, &fontdue);
            diff.save(output_dir.join(format!(
                "{}-slug-fontdue-diff.png",
                debug_glyph_name(character)
            )))?;
        }

        assert!(output_dir.join("slash-slug-fontdue-diff.png").exists());
        assert!(output_dir.join("b-slug-fontdue-diff.png").exists());
        assert!(output_dir.join("r-slug-fontdue-diff.png").exists());
        Ok(())
    }

    #[test]
    fn unicode_snapshot_sheet_writes_debug_artifacts() -> eyre::Result<()> {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let output_dir = manifest_dir
            .join("target")
            .join("test-artifacts")
            .join("slug");

        super::write_slug_snapshot_sheet_png(
            48,
            64,
            24,
            &output_dir.join("unicode-sheet.png"),
            &output_dir.join("unicode-sheet-index.txt"),
        )?;

        assert!(output_dir.join("unicode-sheet.png").exists());
        assert!(output_dir.join("unicode-sheet-index.txt").exists());
        Ok(())
    }

    #[test]
    fn banded_cpu_coverage_matches_full_curve_walk() -> eyre::Result<()> {
        let font = load_terminal_font()?;
        let face = Face::parse(&font.font_bytes, font.face_index)?;

        for character in ['b', 'r'] {
            let glyph_id = face
                .glyph_index(character)
                .expect("comparison glyph should exist in terminal font");
            let curves = extract_glyph_curves(&face, glyph_id);
            let glyph = super::build_slug_glyph_from_face(&font, &face, glyph_id, 0, curves.len());
            let mut band_data = Vec::new();
            let (band_count_x, band_count_y, band_transform) =
                append_slug_band_data(&curves, glyph, &mut band_data);
            let glyph = super::SlugGlyph {
                band_count_x,
                band_count_y,
                band_transform,
                ..glyph
            };

            let step_x = ((glyph.x_max - glyph.x_min).max(1.0)) / 24.0;
            let step_y = ((glyph.y_max - glyph.y_min).max(1.0)) / 24.0;
            for y in 0..=24 {
                for x in 0..=24 {
                    let sample = [
                        glyph.x_min + (step_x * x as f32),
                        glyph.y_min + (step_y * y as f32),
                    ];
                    let banded = cpu_slug_coverage(sample, 16.0, &curves, &band_data, glyph);
                    let full = cpu_slug_coverage_all_curves(sample, 16.0, &curves, glyph);
                    assert!(
                        (banded - full).abs() <= 0.0001,
                        "{character} coverage mismatch at ({}, {}): banded={banded} full={full}",
                        sample[0],
                        sample[1]
                    );
                }
            }
        }

        Ok(())
    }

    fn render_test_glyph(
        font: &super::LoadedTerminalFont,
        curves: &[super::QuadraticCurve],
        band_data: &[u32],
        glyph: super::SlugGlyph,
        font_size_px: u32,
        image_width: u32,
        image_height: u32,
    ) -> RgbaImage {
        let mut image = RgbaImage::new(image_width, image_height);
        super::clear_snapshot_background(&mut image);
        render_snapshot_glyph_into_image(
            &mut image,
            0,
            0,
            image_width,
            image_height,
            font_size_px,
            font,
            curves,
            band_data,
            glyph,
        );

        image
    }

    fn write_fontdue_reference_png(
        character: char,
        font_size_px: u32,
        image_width: u32,
        image_height: u32,
        output_path: &std::path::Path,
    ) -> eyre::Result<()> {
        let image =
            render_fontdue_reference_glyph(character, font_size_px, image_width, image_height)?;

        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).wrap_err_with(|| {
                format!(
                    "failed to create fontdue snapshot directory {}",
                    parent.display()
                )
            })?;
        }
        image.save(output_path).wrap_err_with(|| {
            format!(
                "failed to write fontdue snapshot png {}",
                output_path.display()
            )
        })?;
        Ok(())
    }

    fn render_fontdue_reference_glyph(
        character: char,
        font_size_px: u32,
        image_width: u32,
        image_height: u32,
    ) -> eyre::Result<RgbaImage> {
        use fontdue::{Font as FontdueFont, FontSettings as FontdueSettings};
        use image::Rgba;

        let font = load_terminal_font()?;
        let fontdue_font =
            FontdueFont::from_bytes(font.font_bytes.clone(), FontdueSettings::default())
                .map_err(|message| eyre::eyre!(message))?;
        let (metrics, bitmap) = fontdue_font.rasterize(character, font_size_px as f32);
        let mut image = RgbaImage::new(image_width, image_height);
        super::clear_snapshot_background(&mut image);

        let offset_x = ((image_width as i32 - metrics.width as i32) / 2).max(0) as u32;
        let offset_y = ((image_height as i32 - metrics.height as i32) / 2).max(0) as u32;

        for y in 0..metrics.height {
            for x in 0..metrics.width {
                let alpha = bitmap[y * metrics.width + x];
                if alpha == 0 {
                    continue;
                }
                let dst_x = offset_x + x as u32;
                let dst_y = offset_y + y as u32;
                if dst_x < image_width && dst_y < image_height {
                    image.put_pixel(dst_x, dst_y, Rgba([255, 255, 255, alpha]));
                }
            }
        }

        Ok(image)
    }

    fn debug_glyph_name(character: char) -> &'static str {
        match character {
            '/' => "slash",
            '\\' => "backslash",
            _ => {
                if character == 'b' {
                    "b"
                } else if character == 'r' {
                    "r"
                } else {
                    "glyph"
                }
            }
        }
    }

    fn render_alpha_diff(lhs: &RgbaImage, rhs: &RgbaImage) -> RgbaImage {
        let width = lhs.width().min(rhs.width());
        let height = lhs.height().min(rhs.height());
        let mut image = RgbaImage::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let left = lhs.get_pixel(x, y)[3] as i16;
                let right = rhs.get_pixel(x, y)[3] as i16;
                let delta = (left - right).unsigned_abs() as u8;
                image.put_pixel(x, y, image::Rgba([delta, delta, delta, 255]));
            }
        }
        image
    }

    fn foreground_row_spans(image: &RgbaImage, rgb_threshold: u16) -> Vec<(u32, u32)> {
        let mut spans = Vec::new();
        for y in 0..image.height() {
            let mut first = None;
            let mut last = None;
            for x in 0..image.width() {
                let pixel = image.get_pixel(x, y);
                let intensity = u16::from(pixel[0]) + u16::from(pixel[1]) + u16::from(pixel[2]);
                if intensity <= rgb_threshold {
                    continue;
                }
                first.get_or_insert(x);
                last = Some(x);
            }
            if let (Some(first), Some(last)) = (first, last) {
                spans.push((first, last));
            }
        }
        spans
    }

    fn count_rows_with_multiple_spans(image: &RgbaImage) -> usize {
        let mut count = 0;
        for y in 0..image.height() {
            let mut spans = 0;
            let mut in_span = false;
            for x in 0..image.width() {
                let filled = image.get_pixel(x, y)[3] > 0;
                if filled && !in_span {
                    spans += 1;
                    in_span = true;
                } else if !filled {
                    in_span = false;
                }
            }
            if spans > 1 {
                count += 1;
            }
        }
        count
    }

    fn count_connected_components(image: &RgbaImage, alpha_threshold: u8) -> usize {
        let width = image.width() as usize;
        let height = image.height() as usize;
        let mut visited = vec![false; width * height];
        let mut components = 0;

        for y in 0..height {
            for x in 0..width {
                let index = y * width + x;
                if visited[index] || image.get_pixel(x as u32, y as u32)[3] <= alpha_threshold {
                    continue;
                }

                components += 1;
                let mut stack = vec![(x, y)];
                visited[index] = true;

                while let Some((cx, cy)) = stack.pop() {
                    let min_x = cx.saturating_sub(1);
                    let max_x = (cx + 1).min(width - 1);
                    let min_y = cy.saturating_sub(1);
                    let max_y = (cy + 1).min(height - 1);
                    for ny in min_y..=max_y {
                        for nx in min_x..=max_x {
                            let neighbor_index = ny * width + nx;
                            if visited[neighbor_index] {
                                continue;
                            }
                            if image.get_pixel(nx as u32, ny as u32)[3] <= alpha_threshold {
                                continue;
                            }
                            visited[neighbor_index] = true;
                            stack.push((nx, ny));
                        }
                    }
                }
            }
        }

        components
    }

    #[derive(Default)]
    struct SegmentCountingOutlineBuilder {
        line_segments: usize,
        quadratic_segments: usize,
        cubic_segments: usize,
    }

    impl OutlineBuilder for SegmentCountingOutlineBuilder {
        fn move_to(&mut self, _x: f32, _y: f32) {}

        fn line_to(&mut self, _x: f32, _y: f32) {
            self.line_segments += 1;
        }

        fn quad_to(&mut self, _x1: f32, _y1: f32, _x: f32, _y: f32) {
            self.quadratic_segments += 1;
        }

        fn curve_to(&mut self, _x1: f32, _y1: f32, _x2: f32, _y2: f32, _x: f32, _y: f32) {
            self.cubic_segments += 1;
        }

        fn close(&mut self) {}
    }
}
