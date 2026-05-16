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
    clippy::unused_self
)]
#![allow(clippy::wildcard_imports)]
use std::collections::{BTreeSet, HashMap};
use std::ffi::{CStr, CString, c_void};
use std::fmt::Write as _;
use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::Instant;

use crate::win32_support::string::{EasyPCWSTR, int_resource_pcwstr};
// os[impl os.windows.rendering.direct3d12]
use eyre::Context;
use fontdb::{Database, Family, Query, Source};
use image::imageops::{FilterType, resize};
use image::{ImageBuffer, Rgba, RgbaImage};
#[cfg(feature = "tracy")]
use tracing::debug_span;
use tracing::{info, info_span, instrument, warn};
use ttf_parser::{Face, GlyphId, OutlineBuilder};
use windows::Win32::Foundation::{E_FAIL, FreeLibrary, HANDLE, HWND, RECT, TRUE};
use windows::Win32::Graphics::Direct3D::Fxc::{
    D3DCOMPILE_DEBUG, D3DCOMPILE_SKIP_OPTIMIZATION, D3DCompile,
};
use windows::Win32::Graphics::Direct3D::{
    D3D_FEATURE_LEVEL_11_0, D3D_INCLUDE_TYPE, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, ID3DBlob,
    ID3DInclude, ID3DInclude_Impl,
};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::Dwm::DwmGetColorizationColor;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDC, HBITMAP, HDC, HGDIOBJ, ReleaseDC, SelectObject,
};
use windows::Win32::System::LibraryLoader::LoadLibraryW;
use windows::Win32::System::Threading::{CreateEventW, INFINITE, WaitForSingleObjectEx};
use windows::Win32::UI::WindowsAndMessaging::{
    DI_NORMAL, DestroyIcon, DrawIconEx, GetClientRect, HICON, IDC_ARROW, IDC_CROSS, IDC_HAND,
    IDC_HELP, IDC_IBEAM, IDC_SIZEALL, IDC_WAIT, IMAGE_FLAGS, IMAGE_ICON, LoadCursorW, LoadImageW,
};
use windows::core::BOOL;
use windows::core::{Error, Interface, Owned, PCSTR, PCWSTR, s};

use super::cell_grid;
use super::spatial::{ClientRect, TerminalCellPoint};
use super::windows_terminal::{
    SharedTerminalDisplayState, TerminalDisplayCursor, TerminalDisplayCursorStyle,
    TerminalDisplayRow, TerminalDisplayScrollbar, TerminalLayout, TerminalSelection,
};

const FRAME_COUNT: usize = 2;
const MAX_PANEL_COUNT: usize = 8_192;
const MAX_GLYPH_COUNT: usize = 8_192;
const MAX_SPRITE_COUNT: usize = 256;
const MAX_VERTEX_COUNT: usize = (MAX_PANEL_COUNT + MAX_GLYPH_COUNT + MAX_SPRITE_COUNT) * 6;
const FALLBACK_GLYPH: char = '?';
// These are initial upload capacities. Large glyph sets such as the sprite-sheet
// explorer may require the renderer to grow the backing upload resources.
const MAX_CURVE_FLOAT4_COUNT: usize = 262_144;
const MAX_BAND_UINT_COUNT: usize = 1_048_576;
const MAX_TRANSFORMED_GLYPH_INVERSE_FLOAT4_COUNT: usize = MAX_GLYPH_COUNT * 2;
const TERMINAL_FONT_FAMILY: &str = "CaskaydiaCove Nerd Font Mono";
const SLUG_GLYPH_DILATION_PX: f32 = 0.5;
const SLUG_BAND_SIZE_FONT_UNITS: f32 = 64.0;
const SLUG_HORIZONTAL_COVERAGE_EPSILON: f32 = 1.0 / 65536.0;
const TEAMY_D3D12_GPU_VALIDATION_ENV: &str = "TEAMY_D3D12_GPU_VALIDATION";
const TEAMY_D3D12_OFFSCREEN_ADAPTER_ENV: &str = "TEAMY_D3D12_OFFSCREEN_ADAPTER";
const OFFSCREEN_RENDER_FENCE_TIMEOUT_MS: u32 = 10_000;
const WINDOWS_PANEL_SHADERS_PATH: &str = "src/app/windows_panel_shaders.hlsl";
const WINDOWS_CHROME_SHADERS_PATH: &str = "src/app/windows_chrome_shaders.hlsl";
const WINDOWS_PANEL_SHADERS_SOURCE: &str = include_str!("windows_panel_shaders.hlsl");
const WINDOWS_CHROME_SHADERS_SOURCE: &str = include_str!("windows_chrome_shaders.hlsl");
const SPRITE_SLOT_SIZE: u32 = 320;
const SPRITE_TARGET_SIZE: u32 = 256;
const SPRITE_ATLAS_COLUMNS: u32 = 4;
const SPRITE_ATLAS_ROWS: u32 = 4;

static TERMINAL_FONT_CACHE: OnceLock<Result<Arc<LoadedTerminalFont>, String>> = OnceLock::new();
static TERMINAL_FONT_LAYOUT_CACHE: OnceLock<Result<Arc<TerminalFontLayoutSnapshot>, String>> =
    OnceLock::new();
static SPRITE_ATLAS_CACHE: OnceLock<Result<Arc<SpriteAtlas>, String>> = OnceLock::new();
static COMPILED_SHADER_CACHE: OnceLock<Result<CompiledShaders, String>> = OnceLock::new();

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
    local_bounds: [f32; 4],
    _padding: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct ShaderParams {
    slug_matrix: [[f32; 4]; 4],
    slug_viewport: [f32; 4],
    scene_time: [f32; 4],
    transformed_text_clip_rect: [f32; 4],
    transformed_text_debug_hover: [f32; 4],
    transformed_text_projection: [[f32; 4]; 2],
    transformed_text_inverse_homography: [[f32; 4]; 2],
    sprite_atlas: [f32; 4],
}

#[expect(
    clippy::struct_field_names,
    reason = "uv coordinates map directly to shader inputs"
)]
#[derive(Clone, Copy, Debug)]
struct AtlasSprite {
    uv_left: f32,
    uv_top: f32,
    uv_right: f32,
    uv_bottom: f32,
}

#[derive(Clone, Debug)]
struct SpriteAtlas {
    width: u32,
    height: u32,
    pixels: Vec<u32>,
    terminal: AtlasSprite,
    storage: AtlasSprite,
    audio: AtlasSprite,
    windows_audio: AtlasSprite,
    file_audio: AtlasSprite,
    back: AtlasSprite,
    transcription: AtlasSprite,
    cursor_arrow: AtlasSprite,
    cursor_hand: AtlasSprite,
    cursor_ibeam: AtlasSprite,
    cursor_cross: AtlasSprite,
    cursor_wait: AtlasSprite,
    cursor_size_all: AtlasSprite,
    cursor_help: AtlasSprite,
}

impl SpriteAtlas {
    fn uv_rect(&self, sprite: SpriteId) -> [f32; 4] {
        let rect = match sprite {
            SpriteId::Terminal => self.terminal,
            SpriteId::Storage => self.storage,
            SpriteId::Audio => self.audio,
            SpriteId::WindowsAudio => self.windows_audio,
            SpriteId::FileAudio => self.file_audio,
            SpriteId::Back => self.back,
            SpriteId::Transcription => self.transcription,
            SpriteId::CursorArrow => self.cursor_arrow,
            SpriteId::CursorHand => self.cursor_hand,
            SpriteId::CursorIBeam => self.cursor_ibeam,
            SpriteId::CursorCross => self.cursor_cross,
            SpriteId::CursorWait => self.cursor_wait,
            SpriteId::CursorSizeAll => self.cursor_size_all,
            SpriteId::CursorHelp => self.cursor_help,
        };
        [rect.uv_left, rect.uv_top, rect.uv_right, rect.uv_bottom]
    }
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

#[derive(Clone, Copy, Debug)]
struct DirectionalBandHeader {
    count: u32,
    descending_start: u32,
    ascending_start: u32,
    split: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CoverageRayDirection {
    Left,
    Right,
}

#[derive(Clone, Debug)]
struct LoadedTerminalFont {
    font_bytes: Vec<u8>,
    face_index: u32,
    units_per_em: f32,
    ascender: f32,
    descender: f32,
    cell_advance: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct TerminalFontGlyphMetrics {
    pub x_min: f32,
    pub y_min: f32,
    pub x_max: f32,
    pub y_max: f32,
    pub advance: f32,
}

#[derive(Clone, Debug)]
pub struct TerminalFontLayoutSnapshot {
    pub units_per_em: f32,
    pub ascender: f32,
    pub descender: f32,
    pub cell_advance: f32,
    pub glyphs: HashMap<char, TerminalFontGlyphMetrics>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelEffect {
    BlueBackground = 0,
    GardenFrame = 1,
    TitleBar = 2,
    TerminalPanel = 3,
    DiagnosticPanel = 4,
    TerminalFill = 8,
    TerminalCursor = 9,
    TerminalScrollbarTrack = 10,
    TerminalScrollbarThumb = 11,
    Text = 12,
    SpriteImage = 13,
    SceneButtonCard = 14,
    SceneBody = 15,
    WindowChromePin = 16,
    WindowChromeLatency = 17,
    WindowChromeDiagnostics = 18,
    WindowChromeMinimize = 19,
    WindowChromeMaximize = 20,
    WindowChromeRestore = 21,
    WindowChromeClose = 22,
    GearButton = 23,
    RecordArmButton = 24,
    LoopbackButton = 25,
    TimelineHeadGrabber = 26,
    DemoToggle = 27,
    PlaybackButton = 28,
    TranscriptionToggle = 29,
    TargetMarker = 30,
    TimelineAddTextTrackButton = 31,
    CursorLatencyRipple = 32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelRect {
    pub rect: RECT,
    pub color: [f32; 4],
    pub effect: PanelEffect,
    pub data: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphQuad {
    pub rect: RECT,
    pub color: [f32; 4],
    pub character: char,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransformedGlyphQuad {
    pub corners: [[f32; 2]; 4],
    pub corner_w: [f32; 4],
    pub local_bounds: [f32; 4],
    pub color: [f32; 4],
    pub character: char,
    pub debug_id: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransformedPanelQuad {
    pub corners: [[f32; 2]; 4],
    pub color: [f32; 4],
    pub effect: PanelEffect,
    pub data: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransformedTextPlaneBasis {
    pub screen_corners: [[f32; 2]; 4],
    pub local_corners: [[f32; 2]; 4],
    pub screen_center: [f32; 2],
    pub yaw_radians: f32,
    pub pitch_radians: f32,
    pub camera_distance: f32,
    pub near_plane_distance: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpriteId {
    Terminal,
    Storage,
    Audio,
    WindowsAudio,
    FileAudio,
    Back,
    Transcription,
    CursorArrow,
    CursorHand,
    CursorIBeam,
    CursorCross,
    CursorWait,
    CursorSizeAll,
    CursorHelp,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteQuad {
    pub rect: RECT,
    pub color: [f32; 4],
    pub sprite: SpriteId,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ButtonVisualState {
    pub hover_near: f32,
    pub hovered: bool,
    pub pressed: bool,
    pub click_decay: f32,
    pub active: bool,
}

impl ButtonVisualState {
    #[must_use]
    pub fn shader_data(self) -> [f32; 4] {
        [
            self.hover_near.clamp(0.0, 1.0),
            if self.hovered { 1.0 } else { 0.0 },
            if self.pressed { 1.0 } else { 0.0 },
            self.click_decay.clamp(0.0, 1.0),
        ]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderScene {
    pub panels: Vec<PanelRect>,
    pub glyphs: Vec<GlyphQuad>,
    pub transformed_glyphs: Vec<TransformedGlyphQuad>,
    pub transformed_glyph_clip_rect: Option<RECT>,
    pub transformed_text_plane_basis: Option<TransformedTextPlaneBasis>,
    pub sprites: Vec<SpriteQuad>,
    pub overlay_panels: Vec<PanelRect>,
    pub overlay_transformed_panels: Vec<TransformedPanelQuad>,
    pub overlay_glyphs: Vec<GlyphQuad>,
}

#[derive(Debug)]
pub struct D3d12PanelRenderer {
    _dxgi_factory: IDXGIFactory4,
    dxgi_info_queue: Option<IDXGIInfoQueue>,
    device: ID3D12Device,
    _dcomp_device: IDCompositionDevice,
    _dcomp_target: IDCompositionTarget,
    _dcomp_visual: IDCompositionVisual,
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
    curve_buffer_capacity: usize,
    band_buffer: ID3D12Resource,
    band_buffer_capacity: usize,
    transformed_glyph_inverse_buffer: ID3D12Resource,
    sprite_buffer_keepalive: ID3D12Resource,
    sprite_atlas: Arc<SpriteAtlas>,
    font: Arc<LoadedTerminalFont>,
    glyph_cache: HashMap<char, SlugGlyph>,
    cached_chars: Vec<char>,
    glyph_cache_generation: u64,
    transformed_glyph_clip_rect: Option<RECT>,
    transformed_glyph_debug_enabled: bool,
    transformed_glyph_debug_hover: [f32; 4],
    transformed_text_projection: [[f32; 4]; 2],
    transformed_text_inverse_homography: [[f32; 4]; 2],
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

#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "window chrome button visuals mirror several independent live toggles from the app state"
)]
pub struct WindowChromeButtonsState {
    pub pin: ButtonVisualState,
    pub latency_enabled: bool,
    pub latency: ButtonVisualState,
    pub diagnostics: ButtonVisualState,
    pub minimize: ButtonVisualState,
    pub maximize_restore: ButtonVisualState,
    pub close: ButtonVisualState,
    pub pinned: bool,
    pub latency_visible: bool,
    pub maximized: bool,
    pub focused: bool,
}

#[derive(Clone, Debug)]
pub struct RenderFrameModel {
    pub layout: TerminalLayout,
    pub title: Option<String>,
    pub diagnostic_text: String,
    pub diagnostic_selection: Option<TerminalSelection>,
    pub window_chrome_buttons_state: WindowChromeButtonsState,
    pub diagnostic_cell_width: i32,
    pub diagnostic_cell_height: i32,
    pub scene: Option<RenderScene>,
    pub terminal_cell_width: i32,
    pub terminal_cell_height: i32,
    pub terminal_display: SharedTerminalDisplayState,
    pub terminal_visual_state: RendererTerminalVisualState,
}

impl PartialEq for RenderFrameModel {
    fn eq(&self, other: &Self) -> bool {
        self.layout == other.layout
            && self.title == other.title
            && self.scene == other.scene
            && self.diagnostic_text == other.diagnostic_text
            && self.diagnostic_selection == other.diagnostic_selection
            && self.window_chrome_buttons_state == other.window_chrome_buttons_state
            && self.diagnostic_cell_width == other.diagnostic_cell_width
            && self.diagnostic_cell_height == other.diagnostic_cell_height
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
    title: Option<String>,
    window_chrome_buttons_state: WindowChromeButtonsState,
    scene: Arc<RenderScene>,
}

#[derive(Clone, Debug)]
struct CachedDiagnosticScene {
    layout: TerminalLayout,
    diagnostic_text: String,
    diagnostic_selection: Option<TerminalSelection>,
    diagnostic_cell_width: i32,
    diagnostic_cell_height: i32,
    scene: Arc<RenderScene>,
}

#[derive(Clone, Debug)]
struct CachedTerminalRowScene {
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
    scene_vertices: Option<CachedSceneVertices>,
    chrome: Option<CachedChromeScene>,
    chrome_vertices: Option<CachedSceneVertices>,
    diagnostic: Option<CachedDiagnosticScene>,
    diagnostic_vertices: Option<CachedSceneVertices>,
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
                        let message =
                            format!("failed to create D3D12 renderer thread: {error:#}");
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
                state.error = Some(format!("{error:#}"));
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
        let font_prewarm = maybe_spawn_terminal_font_prewarm()?;
        let sprite_atlas_prewarm = maybe_spawn_sprite_atlas_prewarm()?;
        let shader_prewarm = maybe_spawn_compiled_shader_prewarm()?;

        let (dxgi_factory, device, dxgi_info_queue) =
            info_span!("create_d3d12_device").in_scope(create_device)?;
        let command_queue =
            info_span!("create_d3d12_command_queue").in_scope(|| create_command_queue(&device))?;
        let (width, height) =
            info_span!("query_renderer_client_size").in_scope(|| client_size(hwnd))?;
        let swap_chain = info_span!("create_swap_chain", width, height)
            .in_scope(|| create_swap_chain(&dxgi_factory, &command_queue, width, height))?;
        unsafe { dxgi_factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER)? };
        let (dcomp_device, dcomp_target, dcomp_visual) = info_span!("attach_swap_chain_to_window")
            .in_scope(|| attach_swap_chain_to_window(hwnd, &swap_chain))?;
        unsafe { swap_chain.SetMaximumFrameLatency(1)? };
        let frame_latency_waitable_object =
            unsafe { Owned::new(swap_chain.GetFrameLatencyWaitableObject()) };
        let sprite_atlas = await_renderer_startup_task(
            sprite_atlas_prewarm,
            cached_sprite_atlas,
            "sprite atlas prewarm",
        )?;

        let (rtv_heap, rtv_descriptor_size, render_targets) =
            info_span!("create_render_targets")
                .in_scope(|| create_render_targets(&device, &swap_chain))?;
        let command_allocators = info_span!("create_command_allocators")
            .in_scope(|| create_command_allocators(&device))?;
        let (
            srv_heap,
            curve_buffer,
            band_buffer,
            transformed_glyph_inverse_buffer,
            sprite_buffer,
            sprite_atlas,
        ) = info_span!("create_shader_resources_and_srv")
            .in_scope(|| create_shader_resources_and_srv(&device, Arc::clone(&sprite_atlas)))?;
        let root_signature =
            info_span!("create_root_signature").in_scope(|| create_root_signature(&device))?;
        await_renderer_startup_task(
            shader_prewarm,
            || cached_compiled_shaders().map(|_| ()),
            "compiled shader prewarm",
        )?;
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
        let font = await_renderer_startup_task(
            font_prewarm,
            cached_terminal_font,
            "terminal font prewarm",
        )?;

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
            _dcomp_device: dcomp_device,
            _dcomp_target: dcomp_target,
            _dcomp_visual: dcomp_visual,
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
            curve_buffer_capacity: MAX_CURVE_FLOAT4_COUNT,
            band_buffer,
            band_buffer_capacity: MAX_BAND_UINT_COUNT,
            transformed_glyph_inverse_buffer,
            sprite_buffer_keepalive: sprite_buffer,
            sprite_atlas,
            font,
            glyph_cache: HashMap::new(),
            cached_chars: Vec::new(),
            glyph_cache_generation: 0,
            transformed_glyph_clip_rect: None,
            transformed_glyph_debug_enabled: false,
            transformed_glyph_debug_hover: [0.0; 4],
            transformed_text_projection: [[0.0; 4]; 2],
            transformed_text_inverse_homography: [[0.0; 4]; 2],
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

    fn device_removed_reason_summary(&self) -> String {
        match unsafe { self.device.GetDeviceRemovedReason() } {
            Ok(()) => "GetDeviceRemovedReason returned S_OK".to_owned(),
            Err(error) => format!("GetDeviceRemovedReason returned {error}"),
        }
    }

    fn report_live_device_failure<T>(&self, context: &str, error: &Error) -> eyre::Result<T> {
        self.log_dxgi_debug_messages(context);
        Err(eyre::eyre!(
            "{context}: {error}; {}",
            self.device_removed_reason_summary()
        ))
    }

    fn ensure_live_device_active(&self, context: &str) -> eyre::Result<()> {
        if let Err(error) = unsafe { self.device.GetDeviceRemovedReason() } {
            self.log_dxgi_debug_messages(context);
            eyre::bail!("{context}: {error}");
        }
        Ok(())
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
        // Curve, band, inverse, and vertex uploads all reuse shared upload buffers, so the
        // previous frame must be finished before we overwrite them for a new live frame.
        self.wait_for_last_submitted_frame()?;
        self.transformed_glyph_clip_rect = scenes
            .iter()
            .find_map(|scene| scene.transformed_glyph_clip_rect);
        self.transformed_glyph_debug_enabled = scenes.iter().any(|scene| {
            scene.transformed_glyph_clip_rect.is_some()
                && !scene.overlay_transformed_panels.is_empty()
        });
        self.transformed_glyph_debug_hover = scenes
            .iter()
            .filter(|scene| scene.transformed_glyph_clip_rect.is_some())
            .find_map(|scene| {
                scene
                    .overlay_transformed_panels
                    .first()
                    .map(|panel| panel.data)
            })
            .unwrap_or([0.0; 4]);
        self.transformed_text_projection = scenes
            .iter()
            .find_map(|scene| transformed_text_projection_for_scene(scene))
            .unwrap_or([[0.0; 4]; 2]);
        self.transformed_text_inverse_homography = scenes
            .iter()
            .find_map(|scene| transformed_text_inverse_homography_for_scene(scene))
            .unwrap_or([[0.0; 4]; 2]);
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
        upload_transformed_glyph_inverse_data(&self.transformed_glyph_inverse_buffer, scenes)?;
        self.execute_prepared_frame(vertex_count)
    }

    fn render_frame_model(
        &mut self,
        frame: &RenderFrameModel,
        force_redraw: bool,
        resized: bool,
        scene_cache: &mut RenderThreadSceneCache,
    ) -> eyre::Result<()> {
        // Frame-model rendering updates shared upload buffers before command recording, so keep
        // the previous submission drained before producing the next frame.
        self.wait_for_last_submitted_frame()?;
        if !force_redraw && !resized && scene_cache.last_frame.as_ref() == Some(frame) {
            return Ok(());
        }

        if let Some(scene) = frame.scene.as_ref() {
            self.transformed_glyph_clip_rect = scene.transformed_glyph_clip_rect;
            self.transformed_glyph_debug_enabled = scene.transformed_glyph_clip_rect.is_some()
                && !scene.overlay_transformed_panels.is_empty();
            self.transformed_glyph_debug_hover = scene
                .overlay_transformed_panels
                .first()
                .map_or([0.0; 4], |panel| panel.data);
            self.transformed_text_projection =
                transformed_text_projection_for_scene(scene).unwrap_or([[0.0; 4]; 2]);
            self.transformed_text_inverse_homography =
                transformed_text_inverse_homography_for_scene(scene).unwrap_or([[0.0; 4]; 2]);
            let _glyph_cache_changed = {
                #[cfg(feature = "tracy")]
                let _span = debug_span!("update_slug_curves").entered();
                self.update_slug_curves_for_fragments(&[scene])?
            };
            let scene_vertices = {
                #[cfg(feature = "tracy")]
                let _span = debug_span!("update_scene_vertices").entered();
                self.cached_fragment_vertices(scene, false, &mut scene_cache.scene_vertices)
            };
            let vertex_count = self.upload_cached_fragment_vertices(&[scene_vertices])?;
            upload_transformed_glyph_inverse_data(
                &self.transformed_glyph_inverse_buffer,
                &[scene],
            )?;
            scene_cache.last_frame = Some(frame.clone());
            return self.execute_prepared_frame(vertex_count);
        }

        let (chrome_scene, chrome_reused) = chrome_scene_fragment(
            &mut scene_cache.chrome,
            frame.layout,
            frame.title.as_deref(),
            frame.window_chrome_buttons_state,
        );
        self.transformed_glyph_clip_rect = None;
        self.transformed_glyph_debug_enabled = false;
        self.transformed_glyph_debug_hover = [0.0; 4];
        self.transformed_text_projection = [[0.0; 4]; 2];
        self.transformed_text_inverse_homography = [[0.0; 4]; 2];
        let (terminal_scenes, terminal_reused) = terminal_scene_fragments(
            &mut scene_cache.terminal,
            frame.layout,
            &frame.terminal_display,
            frame.terminal_visual_state,
            frame.terminal_cell_width,
            frame.terminal_cell_height,
        );
        let (diagnostic_scene, diagnostic_reused) = diagnostic_scene_fragment(
            &mut scene_cache.diagnostic,
            frame.layout,
            &frame.diagnostic_text,
            frame.diagnostic_selection,
            frame.diagnostic_cell_width,
            frame.diagnostic_cell_height,
        );

        let glyph_cache_changed = {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("update_slug_curves").entered();
            let mut scenes = Vec::with_capacity(terminal_scenes.len() + 2);
            scenes.push(chrome_scene.as_ref());
            scenes.extend(terminal_scenes.iter().map(Arc::as_ref));
            scenes.push(diagnostic_scene.as_ref());
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
        let diagnostic_vertices = {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("update_diagnostic_vertices").entered();
            self.cached_fragment_vertices(
                diagnostic_scene.as_ref(),
                diagnostic_reused && !glyph_cache_changed,
                &mut scene_cache.diagnostic_vertices,
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
            fragments.push(diagnostic_vertices);
            fragment_reused.push(diagnostic_reused && !glyph_cache_changed);
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
        build_scene_vertices_with_assets(scene, &self.sprite_atlas, &self.glyph_cache, &self.font)
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
            let full_range = 0..vertex_count;
            self.upload_vertex_ranges(&vertices, std::slice::from_ref(&full_range))?;
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
            }
            self.ensure_live_device_active("device was removed after command submission")?;
            if let Err(error) = unsafe { self.swap_chain.Present(0, DXGI_PRESENT(0)).ok() } {
                return self.report_live_device_failure("swap-chain present failed", &error);
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
        self.ensure_shader_resource_capacities(curve_data.len(), band_data.len())?;
        unsafe {
            let mut mapped = std::ptr::null_mut();
            self.curve_buffer.Map(0, None, Some(&mut mapped))?;
            std::ptr::write_bytes(
                mapped,
                0,
                self.curve_buffer_capacity * std::mem::size_of::<[f32; 4]>(),
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
                self.band_buffer_capacity * std::mem::size_of::<u32>(),
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

    fn ensure_shader_resource_capacities(
        &mut self,
        required_curve_capacity: usize,
        required_band_capacity: usize,
    ) -> eyre::Result<()> {
        if required_curve_capacity <= self.curve_buffer_capacity
            && required_band_capacity <= self.band_buffer_capacity
        {
            return Ok(());
        }

        // These upload buffers and the descriptor heap are bound directly as SRVs.
        // Recreate them only after the GPU is idle so earlier frames cannot observe
        // a partially swapped heap/resource set.
        self.wait_for_gpu()?;

        let next_curve_capacity = required_curve_capacity
            .max(self.curve_buffer_capacity.saturating_mul(2))
            .max(MAX_CURVE_FLOAT4_COUNT);
        let next_band_capacity = required_band_capacity
            .max(self.band_buffer_capacity.saturating_mul(2))
            .max(MAX_BAND_UINT_COUNT);
        let (
            srv_heap,
            curve_buffer,
            band_buffer,
            transformed_glyph_inverse_buffer,
            sprite_buffer,
            sprite_atlas,
        ) = create_shader_resources_and_srv_with_capacities(
            &self.device,
            Arc::clone(&self.sprite_atlas),
            next_curve_capacity,
            next_band_capacity,
        )
        .wrap_err_with(|| {
            format!(
                "failed to grow shader resources for curves={next_curve_capacity} bands={next_band_capacity}",
            )
        })?;

        self.srv_heap = srv_heap;
        self.curve_buffer = curve_buffer;
        self.curve_buffer_capacity = next_curve_capacity;
        self.band_buffer = band_buffer;
        self.band_buffer_capacity = next_band_capacity;
        self.transformed_glyph_inverse_buffer = transformed_glyph_inverse_buffer;
        self.sprite_buffer_keepalive = sprite_buffer;
        self.sprite_atlas = sprite_atlas;
        Ok(())
    }

    fn update_shader_params(&self) -> eyre::Result<()> {
        let elapsed_seconds = self.animation_start.elapsed().as_secs_f32();
        let mut params =
            build_shader_params(self.width as f32, self.height as f32, elapsed_seconds);
        params.transformed_text_clip_rect =
            self.transformed_glyph_clip_rect
                .map_or([-1.0, -1.0, -1.0, -1.0], |rect| {
                    [
                        rect.left as f32,
                        rect.top as f32,
                        rect.right as f32,
                        rect.bottom as f32,
                    ]
                });
        params.transformed_text_debug_hover = self.transformed_glyph_debug_hover;
        params.transformed_text_projection = self.transformed_text_projection;
        params.transformed_text_inverse_homography = self.transformed_text_inverse_homography;
        params.scene_time[1] = if self.transformed_glyph_debug_enabled {
            1.0
        } else {
            0.0
        };
        params.sprite_atlas = [
            self.sprite_atlas.width as f32,
            self.sprite_atlas.height as f32,
            0.0,
            0.0,
        ];
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
            return Ok(());
        }

        unsafe {
            WaitForSingleObjectEx(*self.frame_latency_waitable_object, INFINITE, false);
        }

        self.ensure_live_device_active("device was removed while waiting for frame latency")
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
                    .SetEventOnCompletion(fence_value, *self.fence_event)
                    .map_err(|error| {
                        eyre::eyre!(
                            "failed to register frame fence completion event: {error}; {}",
                            self.device_removed_reason_summary()
                        )
                    })?;
                WaitForSingleObjectEx(*self.fence_event, INFINITE, false);
            }
        }

        self.ensure_live_device_active("device was removed while waiting for frame fence")?;

        Ok(())
    }

    fn wait_for_last_submitted_frame(&self) -> eyre::Result<()> {
        let fence_value = self.next_fence_value.saturating_sub(1);
        if fence_value == 0 {
            return Ok(());
        }

        unsafe {
            if self.fence.GetCompletedValue() < fence_value {
                self.fence
                    .SetEventOnCompletion(fence_value, *self.fence_event)
                    .map_err(|error| {
                        eyre::eyre!(
                            "failed to wait for previous frame upload reuse point: {error}; {}",
                            self.device_removed_reason_summary()
                        )
                    })?;
                WaitForSingleObjectEx(*self.fence_event, INFINITE, false);
            }
        }

        self.ensure_live_device_active("device was removed while waiting for previous frame completion")
    }

    fn signal_frame(&mut self, frame_index: usize) -> eyre::Result<()> {
        let fence_value = self.next_fence_value;
        unsafe {
            self.command_queue
                .Signal(&self.fence, fence_value)
                .map_err(|error| {
                    eyre::eyre!(
                        "failed to signal frame fence: {error}; {}",
                        self.device_removed_reason_summary()
                    )
                })?;
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

fn build_scene_vertices_with_assets(
    scene: &RenderScene,
    sprite_atlas: &SpriteAtlas,
    glyph_cache: &HashMap<char, SlugGlyph>,
    font: &LoadedTerminalFont,
) -> Vec<Vertex> {
    let mut vertices = Vec::with_capacity(
        (scene.panels.len()
            + scene.sprites.len()
            + scene.glyphs.len()
            + scene.transformed_glyphs.len()
            + scene.overlay_panels.len()
            + scene.overlay_transformed_panels.len()
            + scene.overlay_glyphs.len())
            * 6,
    );
    for panel in &scene.panels {
        append_rect_with_data(
            &mut vertices,
            panel.rect,
            panel.color,
            panel.effect as u32,
            0,
            [0.0, 0.0, 1.0, 1.0],
            panel.data,
        );
    }
    for sprite in &scene.sprites {
        append_rect_with_data(
            &mut vertices,
            sprite.rect,
            sprite.color,
            PanelEffect::SpriteImage as u32,
            0,
            sprite_atlas.uv_rect(sprite.sprite),
            [0.0; 4],
        );
    }
    for glyph in &scene.glyphs {
        let slug_glyph = glyph_cache
            .get(&glyph.character)
            .or_else(|| glyph_cache.get(&FALLBACK_GLYPH))
            .copied()
            .unwrap_or_else(|| SlugGlyph::empty(font));
        append_text_rect(&mut vertices, glyph.rect, glyph.color, slug_glyph, font);
    }
    for panel in &scene.overlay_transformed_panels {
        append_transformed_panel_quad(
            &mut vertices,
            panel.corners,
            panel.color,
            panel.effect,
            panel.data,
        );
    }
    for glyph in &scene.transformed_glyphs {
        let slug_glyph = glyph_cache
            .get(&glyph.character)
            .or_else(|| glyph_cache.get(&FALLBACK_GLYPH))
            .copied()
            .unwrap_or_else(|| SlugGlyph::empty(font));
        append_transformed_text_quad(
            &mut vertices,
            glyph.corners,
            glyph.corner_w,
            glyph.local_bounds,
            glyph.color,
            slug_glyph,
            glyph.debug_id,
        );
    }
    for panel in &scene.overlay_panels {
        append_rect_with_data(
            &mut vertices,
            panel.rect,
            panel.color,
            panel.effect as u32,
            0,
            [0.0, 0.0, 1.0, 1.0],
            panel.data,
        );
    }
    for glyph in &scene.overlay_glyphs {
        let slug_glyph = glyph_cache
            .get(&glyph.character)
            .or_else(|| glyph_cache.get(&FALLBACK_GLYPH))
            .copied()
            .unwrap_or_else(|| SlugGlyph::empty(font));
        append_text_rect(&mut vertices, glyph.rect, glyph.color, slug_glyph, font);
    }
    vertices
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
    title: Option<&str>,
    window_chrome_buttons_state: WindowChromeButtonsState,
) -> (Arc<RenderScene>, bool) {
    if let Some(cached) = cached_chrome_scene.as_ref()
        && cached.layout == layout
        && cached.title.as_deref() == title
        && cached.window_chrome_buttons_state == window_chrome_buttons_state
    {
        return (Arc::clone(&cached.scene), true);
    }

    let mut scene = build_panel_scene(layout, window_chrome_buttons_state);
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        push_title_text(
            &mut scene,
            layout.title_text_rect().to_win32_rect(),
            title,
            [0.95, 0.95, 0.98, 1.0],
        );
    }
    let scene = Arc::new(scene);
    *cached_chrome_scene = Some(CachedChromeScene {
        layout,
        title: title.map(ToOwned::to_owned),
        window_chrome_buttons_state,
        scene: Arc::clone(&scene),
    });
    (scene, false)
}

fn diagnostic_scene_fragment(
    cached_diagnostic_scene: &mut Option<CachedDiagnosticScene>,
    layout: TerminalLayout,
    diagnostic_text: &str,
    diagnostic_selection: Option<TerminalSelection>,
    diagnostic_cell_width: i32,
    diagnostic_cell_height: i32,
) -> (Arc<RenderScene>, bool) {
    if let Some(cached) = cached_diagnostic_scene.as_ref()
        && cached.layout == layout
        && cached.diagnostic_text == diagnostic_text
        && cached.diagnostic_selection == diagnostic_selection
        && cached.diagnostic_cell_width == diagnostic_cell_width
        && cached.diagnostic_cell_height == diagnostic_cell_height
    {
        return (Arc::clone(&cached.scene), true);
    }

    let scene = cell_grid::build_text_grid_scene(
        layout.diagnostic_panel_rect().inset(14),
        diagnostic_text,
        diagnostic_cell_width,
        diagnostic_cell_height,
        diagnostic_selection,
    );
    let scene = Arc::new(scene);
    *cached_diagnostic_scene = Some(CachedDiagnosticScene {
        layout,
        diagnostic_text: diagnostic_text.to_owned(),
        diagnostic_selection,
        diagnostic_cell_width,
        diagnostic_cell_height,
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
    let terminal_rect = layout.terminal_content_rect();
    let scrollbar_rect = layout.terminal_scrollbar_rect().inset(4);

    let cached = cached_terminal_scene
        .as_ref()
        .filter(|cached| cached.layout == layout);

    let mut row_fragments = Vec::with_capacity(display.rows.len() + 2);
    let mut reused = Vec::with_capacity(display.rows.len() + 2);
    let mut cached_rows = Vec::with_capacity(display.rows.len());
    let dirty_rows = &display.dirty_rows;

    for (index, row) in display.rows.iter().enumerate() {
        let cached_row = cached.and_then(|cached| cached.rows.get(index));
        let row_is_dirty = dirty_rows.binary_search(&index).is_ok();
        if let Some(cached_row) = cached_row
            && !row_is_dirty
        {
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
        cached_rows.push(CachedTerminalRowScene { scene });
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
        transformed_glyphs: Vec::new(),
        transformed_glyph_clip_rect: None,
        transformed_text_plane_basis: None,
        sprites: Vec::new(),
        overlay_panels: Vec::new(),
        overlay_transformed_panels: Vec::new(),
        overlay_glyphs: Vec::new(),
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
        transformed_glyphs: Vec::new(),
        transformed_glyph_clip_rect: None,
        transformed_text_plane_basis: None,
        sprites: Vec::new(),
        overlay_panels: Vec::with_capacity(4),
        overlay_transformed_panels: Vec::new(),
        overlay_glyphs: Vec::new(),
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
        transformed_glyphs: Vec::new(),
        transformed_glyph_clip_rect: None,
        transformed_text_plane_basis: None,
        sprites: Vec::new(),
        overlay_panels: Vec::new(),
        overlay_transformed_panels: Vec::new(),
        overlay_glyphs: Vec::new(),
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

    let track_height_i32 = scrollbar_rect.height().max(1);
    let track_height = u64::try_from(track_height_i32).ok()?;
    let min_thumb_height = scrollbar_rect.width().max(22).min(track_height_i32);
    let proportional_thumb = (track_height.saturating_mul(scrollbar.visible) / scrollbar.total)
        .max(u64::try_from(min_thumb_height).ok()?);
    let thumb_height = i32::try_from(proportional_thumb.min(track_height))
        .ok()?
        .clamp(min_thumb_height, track_height_i32);
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
/// behavior[impl window.appearance.backgrounds.system-accent-half-transparent]
/// behavior[impl window.appearance.chrome.active-window-accent]
/// behavior[impl window.appearance.chrome.inactive-window-muted]
/// behavior[impl window.appearance.code-panel.single-surface]
/// windowing[impl garden-band.shared]
pub fn build_panel_scene(
    layout: TerminalLayout,
    window_chrome_buttons_state: WindowChromeButtonsState,
) -> RenderScene {
    let blue = preferred_background_color();
    let garden = [0.78, 0.88, 0.98, 1.0];
    let title_bar = preferred_title_bar_color(window_chrome_buttons_state.focused);
    let terminal_panel = [0.05, 0.06, 0.08, 1.0];
    let diagnostic_panel = [0.84, 0.44, 0.13, 1.0];
    let mut scene = RenderScene {
        panels: Vec::with_capacity(10),
        glyphs: Vec::with_capacity(2_048),
        transformed_glyphs: Vec::new(),
        transformed_glyph_clip_rect: None,
        transformed_text_plane_basis: None,
        sprites: Vec::new(),
        overlay_panels: Vec::with_capacity(16),
        overlay_transformed_panels: Vec::new(),
        overlay_glyphs: Vec::new(),
    };
    push_panel(
        &mut scene,
        layout.content_frame_rect().to_win32_rect(),
        blue,
        PanelEffect::BlueBackground,
    );
    push_panel_with_data(
        &mut scene,
        layout.garden_rect().to_win32_rect(),
        garden,
        PanelEffect::GardenFrame,
        window_garden_shader_data(layout),
    );
    push_panel(
        &mut scene,
        layout.title_bar_rect().to_win32_rect(),
        title_bar,
        PanelEffect::TitleBar,
    );
    push_panel(
        &mut scene,
        layout.terminal_panel_rect().to_win32_rect(),
        terminal_panel,
        PanelEffect::TerminalPanel,
    );
    push_panel(
        &mut scene,
        layout.diagnostic_panel_rect().to_win32_rect(),
        diagnostic_panel,
        PanelEffect::DiagnosticPanel,
    );
    push_window_chrome_buttons(&mut scene, layout, window_chrome_buttons_state);
    scene
}

#[must_use]
pub fn preferred_background_color() -> [f32; 4] {
    preferred_background_color_from_dwm().unwrap_or([0.11, 0.44, 0.94, 0.5])
}

#[must_use]
pub fn preferred_title_bar_color(focused: bool) -> [f32; 4] {
    if focused {
        preferred_background_color_with_alpha(1.0).unwrap_or([0.11, 0.44, 0.94, 1.0])
    } else {
        [43.0 / 255.0, 43.0 / 255.0, 43.0 / 255.0, 1.0]
    }
}

fn preferred_background_color_from_dwm() -> Option<[f32; 4]> {
    preferred_background_color_with_alpha(0.5)
}

fn preferred_background_color_with_alpha(alpha: f32) -> Option<[f32; 4]> {
    let mut colorization = 0_u32;
    let mut opaque_blend = BOOL(0);

    // Safety: DwmGetColorizationColor writes to the provided out parameters.
    unsafe { DwmGetColorizationColor(&mut colorization, &mut opaque_blend) }.ok()?;

    Some(colorization_color_to_rgba(colorization, alpha))
}

fn colorization_color_to_rgba(colorization: u32, alpha: f32) -> [f32; 4] {
    let red = ((colorization >> 16) & 0xFF) as u8;
    let green = ((colorization >> 8) & 0xFF) as u8;
    let blue = (colorization & 0xFF) as u8;

    [
        f32::from(red) / 255.0,
        f32::from(green) / 255.0,
        f32::from(blue) / 255.0,
        alpha,
    ]
}

#[must_use]
fn normalized_rect_within_parent(parent: ClientRect, child: ClientRect) -> [f32; 4] {
    let width = parent.width().max(1) as f32;
    let height = parent.height().max(1) as f32;
    [
        ((child.left() - parent.left()) as f32 / width).clamp(0.0, 1.0),
        ((child.top() - parent.top()) as f32 / height).clamp(0.0, 1.0),
        ((child.right() - parent.left()) as f32 / width).clamp(0.0, 1.0),
        ((child.bottom() - parent.top()) as f32 / height).clamp(0.0, 1.0),
    ]
}

#[must_use]
pub fn window_garden_shader_data(layout: TerminalLayout) -> [f32; 4] {
    normalized_rect_within_parent(layout.garden_rect(), layout.content_frame_rect())
}

/// windowing[impl garden-band.outward]
/// windowing[impl garden-band.feathered]
pub fn push_window_garden_frame(scene: &mut RenderScene, layout: TerminalLayout) {
    push_panel_with_data(
        scene,
        layout.garden_rect().to_win32_rect(),
        [0.78, 0.88, 0.98, 1.0],
        PanelEffect::GardenFrame,
        window_garden_shader_data(layout),
    );
}

/// windowing[impl diagnostics.toggle.shared-titlebar-button]
pub fn push_window_chrome_buttons(
    scene: &mut RenderScene,
    layout: TerminalLayout,
    window_chrome_buttons_state: WindowChromeButtonsState,
) {
    let mut buttons = vec![
        (
            layout.pin_button_rect().to_win32_rect(),
            window_chrome_button_color(
                window_chrome_buttons_state.pin,
                window_chrome_buttons_state.pinned,
                false,
            ),
            window_chrome_buttons_state.pin,
            PanelEffect::WindowChromePin,
        ),
        (
            layout.diagnostics_button_rect().to_win32_rect(),
            window_chrome_button_color(
                window_chrome_buttons_state.diagnostics,
                window_chrome_buttons_state.diagnostics.active,
                false,
            ),
            window_chrome_buttons_state.diagnostics,
            PanelEffect::WindowChromeDiagnostics,
        ),
        (
            layout.minimize_button_rect().to_win32_rect(),
            window_chrome_button_color(window_chrome_buttons_state.minimize, false, false),
            window_chrome_buttons_state.minimize,
            PanelEffect::WindowChromeMinimize,
        ),
        (
            layout.maximize_restore_button_rect().to_win32_rect(),
            window_chrome_button_color(
                window_chrome_buttons_state.maximize_restore,
                window_chrome_buttons_state.maximized,
                false,
            ),
            window_chrome_buttons_state.maximize_restore,
            if window_chrome_buttons_state.maximized {
                PanelEffect::WindowChromeRestore
            } else {
                PanelEffect::WindowChromeMaximize
            },
        ),
        (
            layout.close_button_rect().to_win32_rect(),
            window_chrome_button_color(window_chrome_buttons_state.close, false, true),
            window_chrome_buttons_state.close,
            PanelEffect::WindowChromeClose,
        ),
    ];

    if window_chrome_buttons_state.latency_enabled {
        buttons.insert(
            1,
            (
                layout.latency_button_rect().to_win32_rect(),
                window_chrome_button_color(
                    window_chrome_buttons_state.latency,
                    window_chrome_buttons_state.latency_visible,
                    false,
                ),
                window_chrome_buttons_state.latency,
                PanelEffect::WindowChromeLatency,
            ),
        );
    }

    for (rect, color, state, effect) in buttons {
        push_panel_with_data(scene, rect, color, effect, state.shader_data());
    }
}

fn window_chrome_button_color(
    _state: ButtonVisualState,
    active: bool,
    destructive: bool,
) -> [f32; 4] {
    if destructive {
        [0.34, 0.12, 0.15, 1.0]
    } else if active {
        [0.23, 0.48, 0.69, 1.0]
    } else {
        [0.12, 0.13, 0.17, 1.0]
    }
}

pub fn push_panel(scene: &mut RenderScene, rect: RECT, color: [f32; 4], effect: PanelEffect) {
    push_panel_with_data(scene, rect, color, effect, [0.0; 4]);
}

pub fn push_panel_with_data(
    scene: &mut RenderScene,
    rect: RECT,
    color: [f32; 4],
    effect: PanelEffect,
    data: [f32; 4],
) {
    if scene.panels.len() + scene.overlay_panels.len() >= MAX_PANEL_COUNT {
        return;
    }

    scene.panels.push(PanelRect {
        rect,
        color,
        effect,
        data,
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
        data: [0.0; 4],
    });
}

pub fn push_overlay_text_block(
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

        if character != ' ' && scene.overlay_glyphs.len() < MAX_GLYPH_COUNT {
            push_overlay_glyph(
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

pub fn push_overlay_transformed_panel(
    scene: &mut RenderScene,
    corners: [[f32; 2]; 4],
    color: [f32; 4],
    effect: PanelEffect,
    data: [f32; 4],
) {
    if scene.overlay_transformed_panels.len() >= MAX_PANEL_COUNT {
        return;
    }
    scene.overlay_transformed_panels.push(TransformedPanelQuad {
        corners,
        color,
        effect,
        data,
    });
}

pub fn push_sprite(scene: &mut RenderScene, rect: RECT, color: [f32; 4], sprite: SpriteId) {
    scene.sprites.push(SpriteQuad {
        rect,
        color,
        sprite,
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

pub fn push_title_text(scene: &mut RenderScene, rect: RECT, text: &str, color: [f32; 4]) {
    let glyph_count = i32::try_from(text.chars().count())
        .unwrap_or_default()
        .max(1);
    let available_width = (rect.right - rect.left - 20).max(8);
    let available_height = (rect.bottom - rect.top - 12).max(8);
    let glyph_height = available_height.clamp(16, 36);
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

pub fn push_overlay_glyph(scene: &mut RenderScene, rect: RECT, character: char, color: [f32; 4]) {
    if scene.overlay_glyphs.len() >= MAX_GLYPH_COUNT || character == ' ' {
        return;
    }
    scene.overlay_glyphs.push(GlyphQuad {
        rect,
        color,
        character,
    });
}

pub fn push_transformed_glyph(
    scene: &mut RenderScene,
    corners: [[f32; 2]; 4],
    corner_w: [f32; 4],
    local_bounds: [f32; 4],
    character: char,
    color: [f32; 4],
    debug_id: f32,
) {
    if scene.transformed_glyphs.len() >= MAX_GLYPH_COUNT || character == ' ' {
        return;
    }
    scene.transformed_glyphs.push(TransformedGlyphQuad {
        corners,
        corner_w,
        local_bounds,
        color,
        character,
        debug_id,
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
    let glyph_capacity = scenes
        .iter()
        .map(|scene| {
            scene.glyphs.len() + scene.transformed_glyphs.len() + scene.overlay_glyphs.len()
        })
        .sum::<usize>()
        + 1;
    let mut chars = Vec::with_capacity(glyph_capacity);
    chars.push(FALLBACK_GLYPH);
    for scene in scenes {
        for glyph in &scene.glyphs {
            if !chars.contains(&glyph.character) {
                chars.push(glyph.character);
            }
        }
        for glyph in &scene.transformed_glyphs {
            if !chars.contains(&glyph.character) {
                chars.push(glyph.character);
            }
        }
        for glyph in &scene.overlay_glyphs {
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

fn cached_terminal_font() -> eyre::Result<Arc<LoadedTerminalFont>> {
    TERMINAL_FONT_CACHE
        .get_or_init(|| {
            load_terminal_font()
                .map(Arc::new)
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map(Arc::clone)
        .map_err(|error| eyre::eyre!(error.clone()))
}

fn build_terminal_font_layout_snapshot() -> eyre::Result<TerminalFontLayoutSnapshot> {
    let font = load_terminal_font()?;
    let face = Face::parse(&font.font_bytes, font.face_index)
        .wrap_err("failed to parse terminal font for layout snapshot")?;
    let glyphs = collect_font_unicode_chars(&face)
        .into_iter()
        .filter_map(|character| {
            let glyph_id = face.glyph_index(character)?;
            let glyph = build_slug_glyph_from_face(&font, &face, glyph_id, 0, 0);
            Some((
                character,
                TerminalFontGlyphMetrics {
                    x_min: glyph.x_min,
                    y_min: glyph.y_min,
                    x_max: glyph.x_max,
                    y_max: glyph.y_max,
                    advance: glyph.advance,
                },
            ))
        })
        .collect();
    Ok(TerminalFontLayoutSnapshot {
        units_per_em: font.units_per_em,
        ascender: font.ascender,
        descender: font.descender,
        cell_advance: font.cell_advance,
        glyphs,
    })
}

pub fn terminal_font_layout_snapshot() -> eyre::Result<Arc<TerminalFontLayoutSnapshot>> {
    TERMINAL_FONT_LAYOUT_CACHE
        .get_or_init(|| {
            build_terminal_font_layout_snapshot()
                .map(Arc::new)
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map(Arc::clone)
        .map_err(|error| eyre::eyre!(error.clone()))
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

#[must_use]
pub fn offscreen_render_backend_name() -> &'static str {
    if offscreen_uses_warp_adapter() {
        "d3d12-warp"
    } else {
        "d3d12-hardware"
    }
}

fn log_offscreen_dxgi_debug_messages(queue: &IDXGIInfoQueue, context: &str) {
    let count = unsafe { queue.GetNumStoredMessages(DXGI_DEBUG_ALL) };
    if count == 0 {
        return;
    }

    warn!(context, count, "offscreen DXGI debug messages");
    for index in 0..count {
        let mut message_size = 0;
        if unsafe { queue.GetMessage(DXGI_DEBUG_ALL, index, None, &mut message_size) }.is_err() {
            warn!(
                context,
                index, "failed to query offscreen DXGI debug message size"
            );
            continue;
        }

        let mut message_buffer = vec![0_u8; message_size];
        let message_ptr = message_buffer.as_mut_ptr() as *mut DXGI_INFO_QUEUE_MESSAGE;
        if unsafe { queue.GetMessage(DXGI_DEBUG_ALL, index, Some(message_ptr), &mut message_size) }
            .is_err()
        {
            warn!(
                context,
                index, "failed to read offscreen DXGI debug message"
            );
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

        warn!(context, index, severity, %description, "offscreen DXGI debug message");
    }

    unsafe { queue.ClearStoredMessages(DXGI_DEBUG_ALL) };
}

/// os[impl os.windows.rendering.direct3d12.offscreen-terminal-verification]
pub fn render_frame_model_offscreen_image(
    frame: &RenderFrameModel,
) -> eyre::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    let width = u32::try_from(frame.layout.client_width.max(1)).unwrap_or(1);
    let height = u32::try_from(frame.layout.client_height.max(1)).unwrap_or(1);
    info!(width, height, "starting offscreen render");
    let use_warp_adapter = offscreen_uses_warp_adapter();
    let (_dxgi_factory, device, dxgi_info_queue) = create_device_with_adapter(use_warp_adapter)
        .wrap_err_with(|| {
            format!(
                "failed to create {} D3D12 device for offscreen render",
                offscreen_render_backend_name()
            )
        })?;
    if let Some(queue) = &dxgi_info_queue {
        unsafe {
            let _ = queue.SetBreakOnSeverity(
                DXGI_DEBUG_ALL,
                DXGI_INFO_QUEUE_MESSAGE_SEVERITY_CORRUPTION,
                false,
            );
            let _ = queue.SetBreakOnSeverity(
                DXGI_DEBUG_ALL,
                DXGI_INFO_QUEUE_MESSAGE_SEVERITY_ERROR,
                false,
            );
            queue.ClearStoredMessages(DXGI_DEBUG_ALL);
        }
        info!(
            "disabled DXGI break-on-error for offscreen render so self-tests can report diagnostics"
        );
    }
    let command_queue = create_command_queue(&device)
        .wrap_err("failed to create D3D12 command queue for offscreen render")?;
    let command_allocator: ID3D12CommandAllocator =
        unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }
            .wrap_err("failed to create D3D12 command allocator for offscreen render")?;
    let font = load_terminal_font()?;
    let root_signature =
        create_root_signature(&device).wrap_err("failed to create offscreen root signature")?;
    let pipeline_state = create_pipeline_state(&device, &root_signature)
        .wrap_err("failed to create offscreen pipeline state")?;
    let command_list: ID3D12GraphicsCommandList = unsafe {
        device.CreateCommandList(
            0,
            D3D12_COMMAND_LIST_TYPE_DIRECT,
            &command_allocator,
            &pipeline_state,
        )
    }
    .wrap_err("failed to create offscreen command list")?;

    let (vertex_buffer, vertex_buffer_view) =
        create_vertex_buffer(&device).wrap_err("failed to create offscreen vertex buffer")?;
    let shader_param_buffer = create_shader_param_buffer(&device)
        .wrap_err("failed to create offscreen shader parameter buffer")?;
    let sprite_atlas = cached_sprite_atlas()?;
    let (render_target, rtv_heap) = create_offscreen_render_target(&device, width, height)
        .wrap_err_with(|| {
            format!("failed to create offscreen render target for {width}x{height} image")
        })?;
    let (readback_buffer, row_pitch) = create_offscreen_readback_buffer(&device, width, height)
        .wrap_err_with(|| {
            format!("failed to create offscreen readback buffer for {width}x{height} image")
        })?;
    let fence: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }
        .wrap_err("failed to create offscreen fence")?;

    let scenes = build_offscreen_frame_scenes(frame);
    info!(fragment_count = scenes.len(), "built offscreen scenes");
    let scene_refs = scenes.iter().map(Arc::as_ref).collect::<Vec<_>>();
    let scene_chars = collect_scene_chars_from_fragments(&scene_refs);
    let (curve_data, band_data, glyph_cache) =
        build_slug_curve_buffer(&font, &scene_chars).wrap_err("failed to build slug curve data")?;
    info!(
        glyph_count = scene_chars.len(),
        curve_count = curve_data.len(),
        band_count = band_data.len(),
        "built offscreen slug buffers"
    );
    let curve_capacity = curve_data.len().max(MAX_CURVE_FLOAT4_COUNT);
    let band_capacity = band_data.len().max(MAX_BAND_UINT_COUNT);
    let (
        srv_heap,
        curve_buffer,
        band_buffer,
        transformed_glyph_inverse_buffer,
        _sprite_buffer_keepalive,
        sprite_atlas,
    ) = create_shader_resources_and_srv_with_capacities(
        &device,
        sprite_atlas,
        curve_capacity,
        band_capacity,
    )
    .wrap_err("failed to create offscreen shader resources")?;
    info!(
        curve_capacity,
        band_capacity, "allocated offscreen shader resources"
    );
    let vertex_fragments = scene_refs
        .iter()
        .map(|scene| build_scene_vertices_with_assets(scene, &sprite_atlas, &glyph_cache, &font))
        .collect::<Vec<_>>();
    let vertex_slices = vertex_fragments
        .iter()
        .map(Vec::as_slice)
        .collect::<Vec<_>>();
    let vertex_count = upload_fragment_vertices(&vertex_buffer, &vertex_slices)?;
    info!(vertex_count, "uploaded offscreen vertices");
    upload_curve_data(&curve_buffer, curve_capacity, &curve_data)?;
    upload_band_data(&band_buffer, band_capacity, &band_data)?;
    upload_transformed_glyph_inverse_data(&transformed_glyph_inverse_buffer, &scene_refs)?;
    upload_offscreen_shader_params(
        &shader_param_buffer,
        width,
        height,
        &sprite_atlas,
        &scene_refs,
    )?;
    info!("uploaded offscreen shader data");

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
        right: i32::try_from(width).unwrap_or(i32::MAX),
        bottom: i32::try_from(height).unwrap_or(i32::MAX),
    };
    let rtv_handle = unsafe { rtv_heap.GetCPUDescriptorHandleForHeapStart() };

    unsafe {
        command_list.SetDescriptorHeaps(&[Some(srv_heap.clone())]);
        command_list.SetGraphicsRootSignature(&root_signature);
        command_list
            .SetGraphicsRootConstantBufferView(0, shader_param_buffer.GetGPUVirtualAddress());
        command_list
            .SetGraphicsRootDescriptorTable(1, srv_heap.GetGPUDescriptorHandleForHeapStart());
        command_list.RSSetViewports(&[viewport]);
        command_list.RSSetScissorRects(&[scissor_rect]);
        command_list.OMSetRenderTargets(1, Some(&rtv_handle), false, None);

        let clear_color = [0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32];
        command_list.ClearRenderTargetView(rtv_handle, &clear_color, None);
        command_list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        command_list.IASetVertexBuffers(0, Some(&[vertex_buffer_view]));
        command_list.DrawInstanced(vertex_count as u32, 1, 0, 0);

        issue_transition_barrier(
            &command_list,
            &render_target,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
        );

        let mut destination =
            texture_copy_location_for_buffer(&readback_buffer, width, height, row_pitch);
        let mut source = texture_copy_location_for_subresource(&render_target, 0);
        command_list.CopyTextureRegion(&destination, 0, 0, 0, &source, None);
        release_texture_copy_location(&mut destination);
        release_texture_copy_location(&mut source);
        command_list.Close()?;
    }

    let command_lists = [Some(command_list.cast::<ID3D12CommandList>()?)];
    unsafe {
        command_queue.ExecuteCommandLists(&command_lists);
        command_queue.Signal(&fence, 1)?;
        info!("submitted offscreen command list");
        if let Some(queue) = &dxgi_info_queue {
            log_offscreen_dxgi_debug_messages(queue, "offscreen render post-submit");
        }
        if let Err(error) = device.GetDeviceRemovedReason() {
            eyre::bail!("offscreen device was removed after command submission: {error}");
        }
        if fence.GetCompletedValue() < 1 {
            let wait_started = Instant::now();
            // Offscreen verification submits a single command list, so bounded fence polling is
            // simpler and more diagnosable here than routing through a Win32 event wait.
            while fence.GetCompletedValue() < 1 {
                if let Err(error) = device.GetDeviceRemovedReason() {
                    if let Some(queue) = &dxgi_info_queue {
                        log_offscreen_dxgi_debug_messages(queue, "offscreen render device removed");
                    }
                    eyre::bail!(
                        "offscreen device was removed while waiting for completion: {error}"
                    );
                }
                if wait_started.elapsed().as_millis()
                    >= u128::from(OFFSCREEN_RENDER_FENCE_TIMEOUT_MS)
                {
                    if let Some(queue) = &dxgi_info_queue {
                        log_offscreen_dxgi_debug_messages(queue, "offscreen render fence timeout");
                    }
                    eyre::bail!(
                        "offscreen render fence timed out after {OFFSCREEN_RENDER_FENCE_TIMEOUT_MS}ms; \
                         the fixture likely stalled the command queue before readback"
                    );
                }
                thread::yield_now();
            }
        }
    }

    info!("reading back offscreen render target");
    if let Some(queue) = &dxgi_info_queue {
        log_offscreen_dxgi_debug_messages(queue, "offscreen render pre-readback");
    }
    readback_texture_to_image(&readback_buffer, width, height, row_pitch)
}

fn offscreen_uses_warp_adapter() -> bool {
    std::env::var(TEAMY_D3D12_OFFSCREEN_ADAPTER_ENV)
        .map_or(true, |value| !value.eq_ignore_ascii_case("hardware"))
}

pub(crate) fn render_frame_model_scene_snapshot(frame: &RenderFrameModel) -> String {
    let scenes = build_offscreen_frame_scenes(frame);
    let mut snapshot = String::new();
    let _ = writeln!(snapshot, "fragment_count={}", scenes.len());

    for (fragment_index, scene) in scenes.iter().enumerate() {
        let _ = writeln!(
            snapshot,
            "[fragment {fragment_index}] panels={} glyphs={} transformed_glyphs={} sprites={} overlay_panels={} overlay_glyphs={}",
            scene.panels.len(),
            scene.glyphs.len(),
            scene.transformed_glyphs.len(),
            scene.sprites.len(),
            scene.overlay_panels.len(),
            scene.overlay_glyphs.len(),
        );

        for (panel_index, panel) in scene.panels.iter().enumerate() {
            let _ = writeln!(
                snapshot,
                "panel {panel_index} effect={:?} rect={},{},{},{} color={:.3},{:.3},{:.3},{:.3} data={:.3},{:.3},{:.3},{:.3}",
                panel.effect,
                panel.rect.left,
                panel.rect.top,
                panel.rect.right,
                panel.rect.bottom,
                panel.color[0],
                panel.color[1],
                panel.color[2],
                panel.color[3],
                panel.data[0],
                panel.data[1],
                panel.data[2],
                panel.data[3],
            );
        }

        for (glyph_index, glyph) in scene.glyphs.iter().enumerate() {
            let escaped = glyph.character.escape_default().to_string();
            let _ = writeln!(
                snapshot,
                "glyph {glyph_index} char=U+{:04X} literal='{}' rect={},{},{},{} color={:.3},{:.3},{:.3},{:.3}",
                u32::from(glyph.character),
                escaped,
                glyph.rect.left,
                glyph.rect.top,
                glyph.rect.right,
                glyph.rect.bottom,
                glyph.color[0],
                glyph.color[1],
                glyph.color[2],
                glyph.color[3],
            );
        }

        for (glyph_index, glyph) in scene.transformed_glyphs.iter().enumerate() {
            let escaped = glyph.character.escape_default().to_string();
            let _ = writeln!(
                snapshot,
                "transformed_glyph {glyph_index} char=U+{:04X} literal='{}' tl={:.3},{:.3} tr={:.3},{:.3} br={:.3},{:.3} bl={:.3},{:.3} color={:.3},{:.3},{:.3},{:.3}",
                u32::from(glyph.character),
                escaped,
                glyph.corners[0][0],
                glyph.corners[0][1],
                glyph.corners[1][0],
                glyph.corners[1][1],
                glyph.corners[2][0],
                glyph.corners[2][1],
                glyph.corners[3][0],
                glyph.corners[3][1],
                glyph.color[0],
                glyph.color[1],
                glyph.color[2],
                glyph.color[3],
            );
        }

        for (sprite_index, sprite) in scene.sprites.iter().enumerate() {
            let _ = writeln!(
                snapshot,
                "sprite {sprite_index} id={:?} rect={},{},{},{} color={:.3},{:.3},{:.3},{:.3}",
                sprite.sprite,
                sprite.rect.left,
                sprite.rect.top,
                sprite.rect.right,
                sprite.rect.bottom,
                sprite.color[0],
                sprite.color[1],
                sprite.color[2],
                sprite.color[3],
            );
        }

        for (overlay_index, panel) in scene.overlay_panels.iter().enumerate() {
            let _ = writeln!(
                snapshot,
                "overlay_panel {overlay_index} effect={:?} rect={},{},{},{} color={:.3},{:.3},{:.3},{:.3} data={:.3},{:.3},{:.3},{:.3}",
                panel.effect,
                panel.rect.left,
                panel.rect.top,
                panel.rect.right,
                panel.rect.bottom,
                panel.color[0],
                panel.color[1],
                panel.color[2],
                panel.color[3],
                panel.data[0],
                panel.data[1],
                panel.data[2],
                panel.data[3],
            );
        }
        for (glyph_index, glyph) in scene.overlay_glyphs.iter().enumerate() {
            let escaped = glyph.character.escape_default().to_string();
            let _ = writeln!(
                snapshot,
                "overlay_glyph {glyph_index} char=U+{:04X} literal='{}' rect={},{},{},{} color={:.3},{:.3},{:.3},{:.3}",
                u32::from(glyph.character),
                escaped,
                glyph.rect.left,
                glyph.rect.top,
                glyph.rect.right,
                glyph.rect.bottom,
                glyph.color[0],
                glyph.color[1],
                glyph.color[2],
                glyph.color[3],
            );
        }
    }

    snapshot
}

fn build_offscreen_frame_scenes(frame: &RenderFrameModel) -> Vec<Arc<RenderScene>> {
    if let Some(scene) = frame.scene.as_ref() {
        return vec![Arc::new(scene.clone())];
    }

    let mut chrome_cache = None;
    let mut diagnostic_cache = None;
    let mut terminal_cache = None;
    let (chrome_scene, _) = chrome_scene_fragment(
        &mut chrome_cache,
        frame.layout,
        frame.title.as_deref(),
        frame.window_chrome_buttons_state,
    );
    let (diagnostic_scene, _) = diagnostic_scene_fragment(
        &mut diagnostic_cache,
        frame.layout,
        &frame.diagnostic_text,
        frame.diagnostic_selection,
        frame.diagnostic_cell_width,
        frame.diagnostic_cell_height,
    );
    let (terminal_fragments, _) = terminal_scene_fragments(
        &mut terminal_cache,
        frame.layout,
        &frame.terminal_display,
        frame.terminal_visual_state,
        frame.terminal_cell_width,
        frame.terminal_cell_height,
    );

    let mut scenes = Vec::with_capacity(2 + terminal_fragments.len());
    scenes.push(chrome_scene);
    scenes.extend(terminal_fragments);
    scenes.push(diagnostic_scene);
    scenes
}

fn upload_fragment_vertices(
    vertex_buffer: &ID3D12Resource,
    fragments: &[&[Vertex]],
) -> eyre::Result<usize> {
    let vertex_count = fragments
        .iter()
        .map(|fragment| fragment.len())
        .sum::<usize>();
    unsafe {
        let mut mapped = std::ptr::null_mut();
        vertex_buffer.Map(0, None, Some(&mut mapped))?;
        let mut write_ptr = mapped as *mut Vertex;
        for fragment in fragments {
            std::ptr::copy_nonoverlapping(fragment.as_ptr(), write_ptr, fragment.len());
            write_ptr = write_ptr.add(fragment.len());
        }
        vertex_buffer.Unmap(0, None);
    }
    Ok(vertex_count)
}

fn upload_curve_data(
    curve_buffer: &ID3D12Resource,
    curve_capacity: usize,
    curve_data: &[[f32; 4]],
) -> eyre::Result<()> {
    unsafe {
        let mut mapped = std::ptr::null_mut();
        curve_buffer.Map(0, None, Some(&mut mapped))?;
        std::ptr::write_bytes(mapped, 0, curve_capacity * std::mem::size_of::<[f32; 4]>());
        std::ptr::copy_nonoverlapping(
            curve_data.as_ptr(),
            mapped as *mut [f32; 4],
            curve_data.len(),
        );
        curve_buffer.Unmap(0, None);
    }
    Ok(())
}

fn upload_band_data(
    band_buffer: &ID3D12Resource,
    band_capacity: usize,
    band_data: &[u32],
) -> eyre::Result<()> {
    unsafe {
        let mut mapped = std::ptr::null_mut();
        band_buffer.Map(0, None, Some(&mut mapped))?;
        std::ptr::write_bytes(mapped, 0, band_capacity * std::mem::size_of::<u32>());
        std::ptr::copy_nonoverlapping(band_data.as_ptr(), mapped as *mut u32, band_data.len());
        band_buffer.Unmap(0, None);
    }
    Ok(())
}

fn build_transformed_glyph_inverse_data(scenes: &[&RenderScene]) -> Vec<[f32; 4]> {
    let mut inverse_data = vec![[0.0_f32; 4]; MAX_TRANSFORMED_GLYPH_INVERSE_FLOAT4_COUNT];

    for scene in scenes {
        for glyph in &scene.transformed_glyphs {
            let glyph_index = glyph.debug_id.max(1.0) as usize - 1;
            let coefficient_index = glyph_index.saturating_mul(2);
            if coefficient_index + 1 >= inverse_data.len() {
                continue;
            }

            let local_points = [
                [glyph.local_bounds[0], glyph.local_bounds[2]],
                [glyph.local_bounds[1], glyph.local_bounds[2]],
                [glyph.local_bounds[1], glyph.local_bounds[3]],
                [glyph.local_bounds[0], glyph.local_bounds[3]],
            ];
            let Some(inverse) = solve_inverse_homography(glyph.corners, local_points) else {
                continue;
            };
            inverse_data[coefficient_index] = [inverse[0], inverse[1], inverse[2], inverse[6]];
            inverse_data[coefficient_index + 1] = [inverse[3], inverse[4], inverse[5], inverse[7]];
        }
    }

    inverse_data
}

fn upload_transformed_glyph_inverse_data(
    transformed_glyph_inverse_buffer: &ID3D12Resource,
    scenes: &[&RenderScene],
) -> eyre::Result<()> {
    let inverse_data = build_transformed_glyph_inverse_data(scenes);
    unsafe {
        let mut mapped = std::ptr::null_mut();
        transformed_glyph_inverse_buffer.Map(0, None, Some(&mut mapped))?;
        std::ptr::copy_nonoverlapping(
            inverse_data.as_ptr(),
            mapped as *mut [f32; 4],
            inverse_data.len(),
        );
        transformed_glyph_inverse_buffer.Unmap(0, None);
    }
    Ok(())
}

fn upload_offscreen_shader_params(
    shader_param_buffer: &ID3D12Resource,
    width: u32,
    height: u32,
    sprite_atlas: &SpriteAtlas,
    scenes: &[&RenderScene],
) -> eyre::Result<()> {
    let mut params = build_shader_params(width as f32, height as f32, 0.0);
    params.transformed_text_clip_rect = scenes
        .iter()
        .find_map(|scene| scene.transformed_glyph_clip_rect)
        .map_or([-1.0, -1.0, -1.0, -1.0], |rect| {
            [
                rect.left as f32,
                rect.top as f32,
                rect.right as f32,
                rect.bottom as f32,
            ]
        });
    params.transformed_text_projection = scenes
        .iter()
        .find_map(|scene| transformed_text_projection_for_scene(scene))
        .unwrap_or([[0.0; 4]; 2]);
    params.transformed_text_inverse_homography = scenes
        .iter()
        .find_map(|scene| transformed_text_inverse_homography_for_scene(scene))
        .unwrap_or([[0.0; 4]; 2]);
    params.sprite_atlas = [
        sprite_atlas.width as f32,
        sprite_atlas.height as f32,
        0.0,
        0.0,
    ];
    unsafe {
        let mut mapped = std::ptr::null_mut();
        shader_param_buffer.Map(0, None, Some(&mut mapped))?;
        std::ptr::copy_nonoverlapping(&params, mapped as *mut ShaderParams, 1);
        shader_param_buffer.Unmap(0, None);
    }
    Ok(())
}

fn create_offscreen_render_target(
    device: &ID3D12Device,
    width: u32,
    height: u32,
) -> eyre::Result<(ID3D12Resource, ID3D12DescriptorHeap)> {
    let mut render_target = None;
    unsafe {
        device.CreateCommittedResource(
            &D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_DEFAULT,
                ..Default::default()
            },
            D3D12_HEAP_FLAG_NONE,
            &D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                Width: u64::from(width),
                Height: height,
                DepthOrArraySize: 1,
                MipLevels: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
                Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
                ..Default::default()
            },
            D3D12_RESOURCE_STATE_RENDER_TARGET,
            None,
            &mut render_target,
        )?;
    }
    let render_target = render_target.expect("offscreen render target should be initialized");
    let rtv_heap: ID3D12DescriptorHeap = unsafe {
        device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
            NumDescriptors: 1,
            ..Default::default()
        })?
    };
    unsafe {
        device.CreateRenderTargetView(
            &render_target,
            None,
            rtv_heap.GetCPUDescriptorHandleForHeapStart(),
        );
    }
    Ok((render_target, rtv_heap))
}

fn create_offscreen_readback_buffer(
    device: &ID3D12Device,
    width: u32,
    height: u32,
) -> eyre::Result<(ID3D12Resource, u32)> {
    let bytes_per_pixel = 4_u32;
    let unaligned_row_pitch = width.saturating_mul(bytes_per_pixel);
    let alignment = D3D12_TEXTURE_DATA_PITCH_ALIGNMENT;
    let row_pitch = unaligned_row_pitch.div_ceil(alignment) * alignment;
    let buffer_size = u64::from(row_pitch) * u64::from(height.max(1));
    let mut readback_buffer = None;
    unsafe {
        device.CreateCommittedResource(
            &D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_READBACK,
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
            D3D12_RESOURCE_STATE_COPY_DEST,
            None,
            &mut readback_buffer,
        )?;
    }
    Ok((
        readback_buffer.expect("offscreen readback buffer should be initialized"),
        row_pitch,
    ))
}

fn texture_copy_location_for_subresource(
    resource: &ID3D12Resource,
    subresource_index: u32,
) -> D3D12_TEXTURE_COPY_LOCATION {
    D3D12_TEXTURE_COPY_LOCATION {
        pResource: std::mem::ManuallyDrop::new(Some(resource.clone())),
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            SubresourceIndex: subresource_index,
        },
    }
}

fn texture_copy_location_for_buffer(
    resource: &ID3D12Resource,
    width: u32,
    height: u32,
    row_pitch: u32,
) -> D3D12_TEXTURE_COPY_LOCATION {
    D3D12_TEXTURE_COPY_LOCATION {
        pResource: std::mem::ManuallyDrop::new(Some(resource.clone())),
        Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                Offset: 0,
                Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    Width: width,
                    Height: height,
                    Depth: 1,
                    RowPitch: row_pitch,
                },
            },
        },
    }
}

fn release_texture_copy_location(location: &mut D3D12_TEXTURE_COPY_LOCATION) {
    unsafe {
        let resource = std::mem::ManuallyDrop::take(&mut location.pResource);
        drop(resource);
    }
}

fn readback_texture_to_image(
    readback_buffer: &ID3D12Resource,
    width: u32,
    height: u32,
    row_pitch: u32,
) -> eyre::Result<RgbaImage> {
    let mut image = RgbaImage::new(width, height);
    unsafe {
        let mut mapped = std::ptr::null_mut();
        readback_buffer.Map(0, None, Some(&mut mapped))?;
        let bytes = std::slice::from_raw_parts(
            mapped as *const u8,
            usize::try_from(u64::from(row_pitch) * u64::from(height)).unwrap_or_default(),
        );
        for y in 0..height {
            let row_offset =
                usize::try_from(u64::from(y) * u64::from(row_pitch)).unwrap_or_default();
            let row =
                &bytes[row_offset..row_offset + usize::try_from(width * 4).unwrap_or_default()];
            for x in 0..width {
                let pixel_offset = usize::try_from(x * 4).unwrap_or_default();
                let blue = row[pixel_offset];
                let green = row[pixel_offset + 1];
                let red = row[pixel_offset + 2];
                let alpha = row[pixel_offset + 3];
                image.put_pixel(x, y, Rgba([red, green, blue, alpha]));
            }
        }
        readback_buffer.Unmap(0, None);
    }
    Ok(image)
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
    let mut horizontal_bands_ascending = vec![Vec::<usize>::new(); band_count_y as usize];
    let mut vertical_bands = vec![Vec::<usize>::new(); band_count_x as usize];
    let mut vertical_bands_ascending = vec![Vec::<usize>::new(); band_count_x as usize];

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
    for (ascending_band, descending_band) in horizontal_bands_ascending
        .iter_mut()
        .zip(horizontal_bands.iter())
    {
        *ascending_band = descending_band.clone();
        ascending_band.sort_by(|lhs, rhs| {
            curve_extents[*lhs]
                .min_x
                .total_cmp(&curve_extents[*rhs].min_x)
        });
    }
    for band in &mut vertical_bands {
        band.sort_by(|lhs, rhs| {
            curve_extents[*rhs]
                .max_y
                .total_cmp(&curve_extents[*lhs].max_y)
        });
    }
    for (ascending_band, descending_band) in vertical_bands_ascending
        .iter_mut()
        .zip(vertical_bands.iter())
    {
        *ascending_band = descending_band.clone();
        ascending_band.sort_by(|lhs, rhs| {
            curve_extents[*lhs]
                .min_y
                .total_cmp(&curve_extents[*rhs].min_y)
        });
    }

    let table_start = band_data.len();
    let table_len = ((band_count_x + band_count_y) as usize) * 4;
    band_data.resize(table_start + table_len, 0);

    for (band_index, band) in horizontal_bands.iter().enumerate() {
        let ascending_band = &horizontal_bands_ascending[band_index];
        let split = choose_horizontal_band_split(
            band,
            ascending_band,
            &curve_extents,
            glyph.x_min,
            glyph.x_max,
        );
        let entry_index = table_start + (band_index * 4);
        band_data[entry_index] = band.len() as u32;
        band_data[entry_index + 1] = band_data.len() as u32;
        for curve_index in band {
            band_data.push(*curve_index as u32);
        }
        band_data[entry_index + 2] = band_data.len() as u32;
        for curve_index in ascending_band {
            band_data.push(*curve_index as u32);
        }
        band_data[entry_index + 3] = split.to_bits();
    }

    let vertical_table_start = table_start + (band_count_y as usize * 4);
    for (band_index, band) in vertical_bands.iter().enumerate() {
        let ascending_band = &vertical_bands_ascending[band_index];
        let split = choose_vertical_band_split(
            band,
            ascending_band,
            &curve_extents,
            glyph.y_min,
            glyph.y_max,
        );
        let entry_index = vertical_table_start + (band_index * 4);
        band_data[entry_index] = band.len() as u32;
        band_data[entry_index + 1] = band_data.len() as u32;
        for curve_index in band {
            band_data.push(*curve_index as u32);
        }
        band_data[entry_index + 2] = band_data.len() as u32;
        for curve_index in ascending_band {
            band_data.push(*curve_index as u32);
        }
        band_data[entry_index + 3] = split.to_bits();
    }

    (band_count_x, band_count_y, band_transform)
}

fn choose_horizontal_band_split(
    descending_band: &[usize],
    ascending_band: &[usize],
    curve_extents: &[CurveExtents],
    fallback_min: f32,
    fallback_max: f32,
) -> f32 {
    choose_band_split(
        descending_band,
        ascending_band,
        curve_extents,
        |extents| extents.max_x,
        |extents| extents.min_x,
        fallback_min,
        fallback_max,
    )
}

fn choose_vertical_band_split(
    descending_band: &[usize],
    ascending_band: &[usize],
    curve_extents: &[CurveExtents],
    fallback_min: f32,
    fallback_max: f32,
) -> f32 {
    choose_band_split(
        descending_band,
        ascending_band,
        curve_extents,
        |extents| extents.max_y,
        |extents| extents.min_y,
        fallback_min,
        fallback_max,
    )
}

fn choose_band_split(
    descending_band: &[usize],
    ascending_band: &[usize],
    curve_extents: &[CurveExtents],
    descending_value: impl Fn(CurveExtents) -> f32,
    ascending_value: impl Fn(CurveExtents) -> f32,
    fallback_min: f32,
    fallback_max: f32,
) -> f32 {
    let count = descending_band.len();
    if count == 0 {
        return (fallback_min + fallback_max) * 0.5;
    }

    let mut best_worst = count;
    let mut best_split = (fallback_min + fallback_max) * 0.5;
    let mut left_count = count;

    for (curve_offset, curve_index) in descending_band.iter().enumerate() {
        let split = descending_value(curve_extents[*curve_index]);
        let right_count = curve_offset + 1;
        while left_count > 0
            && ascending_value(curve_extents[ascending_band[left_count - 1]]) > split
        {
            left_count -= 1;
        }
        let worst = right_count.max(left_count);
        if worst < best_worst {
            best_worst = worst;
            best_split = split;
        }
    }

    best_split
}

fn load_directional_band_header(band_data: &[u32], entry_index: usize) -> DirectionalBandHeader {
    DirectionalBandHeader {
        count: band_data.get(entry_index).copied().unwrap_or_default(),
        descending_start: band_data.get(entry_index + 1).copied().unwrap_or_default(),
        ascending_start: band_data.get(entry_index + 2).copied().unwrap_or_default(),
        split: f32::from_bits(band_data.get(entry_index + 3).copied().unwrap_or_default()),
    }
}

fn horizontal_band_header_index(glyph: SlugGlyph, band_index: usize) -> usize {
    glyph.band_start as usize + (band_index * 4)
}

fn vertical_band_header_index(glyph: SlugGlyph, band_index: usize) -> usize {
    glyph.band_start as usize + (glyph.band_count_y as usize * 4) + (band_index * 4)
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

pub fn terminal_font_unicode_chars() -> eyre::Result<Vec<char>> {
    let font = load_terminal_font()?;
    let face = Face::parse(&font.font_bytes, font.face_index)
        .wrap_err("failed to parse terminal font face for glyph enumeration")?;
    Ok(collect_font_unicode_chars(&face))
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
    let offset_y = (origin_y as f32 + ((image_height as f32 - glyph_height_px) * 0.5).max(0.0)
        - 1.0)
        .max(origin_y as f32);
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

fn cpu_slug_coverage_single_sample(
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
    let horizontal_header = load_directional_band_header(
        band_data,
        horizontal_band_header_index(glyph, horizontal_band),
    );
    let horizontal_direction = if render_coord[0] < horizontal_header.split {
        CoverageRayDirection::Left
    } else {
        CoverageRayDirection::Right
    };
    let horizontal_start = match horizontal_direction {
        CoverageRayDirection::Left => horizontal_header.ascending_start as usize,
        CoverageRayDirection::Right => horizontal_header.descending_start as usize,
    };
    for offset in 0..horizontal_header.count as usize {
        let curve_index = band_data
            .get(horizontal_start + offset)
            .copied()
            .unwrap_or_default() as usize;
        let Some(curve) = curves.get(curve_index) else {
            continue;
        };
        let extents = curve_extents(*curve);
        match horizontal_direction {
            CoverageRayDirection::Left => {
                if (extents.min_x - render_coord[0]) * pixels_per_em > 0.5 {
                    break;
                }
            }
            CoverageRayDirection::Right => {
                if (extents.max_x - render_coord[0]) * pixels_per_em < -0.5 {
                    break;
                }
            }
        }
        accumulate_horizontal_curve_coverage(
            curve,
            render_coord,
            pixels_per_em,
            horizontal_direction,
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
    let vertical_header =
        load_directional_band_header(band_data, vertical_band_header_index(glyph, vertical_band));
    let vertical_direction = if render_coord[1] < vertical_header.split {
        CoverageRayDirection::Left
    } else {
        CoverageRayDirection::Right
    };
    let vertical_start = match vertical_direction {
        CoverageRayDirection::Left => vertical_header.ascending_start as usize,
        CoverageRayDirection::Right => vertical_header.descending_start as usize,
    };
    for offset in 0..vertical_header.count as usize {
        let curve_index = band_data
            .get(vertical_start + offset)
            .copied()
            .unwrap_or_default() as usize;
        let Some(curve) = curves.get(curve_index) else {
            continue;
        };
        let extents = curve_extents(*curve);
        match vertical_direction {
            CoverageRayDirection::Left => {
                if (extents.min_y - render_coord[1]) * pixels_per_em > 0.5 {
                    break;
                }
            }
            CoverageRayDirection::Right => {
                if (extents.max_y - render_coord[1]) * pixels_per_em < -0.5 {
                    break;
                }
            }
        }
        accumulate_vertical_curve_coverage(
            curve,
            render_coord,
            pixels_per_em,
            vertical_direction,
            &mut ycov,
            &mut ywgt,
        );
    }

    calc_coverage(xcov, ycov, xwgt, ywgt)
}

fn cpu_slug_coverage(
    render_coord: [f32; 2],
    pixels_per_em: f32,
    curves: &[QuadraticCurve],
    band_data: &[u32],
    glyph: SlugGlyph,
) -> f32 {
    let sample_step = 0.25 / pixels_per_em.max(1.0 / 65536.0);
    let sample_offsets = [
        [-sample_step, -sample_step],
        [sample_step, -sample_step],
        [-sample_step, sample_step],
        [sample_step, sample_step],
    ];
    let mut coverage = 0.0;
    for offset in sample_offsets {
        coverage += cpu_slug_coverage_single_sample(
            [render_coord[0] + offset[0], render_coord[1] + offset[1]],
            pixels_per_em,
            curves,
            band_data,
            glyph,
        );
    }
    coverage * 0.25
}

#[cfg(test)]
fn cpu_slug_coverage_all_curves_single_sample(
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
    let start = usize::try_from(glyph.curve_start).unwrap_or_default();
    let end = start + usize::try_from(glyph.curve_count).unwrap_or_default();
    let horizontal_band = band_index(
        render_coord[1],
        glyph.band_transform[1],
        glyph.band_transform[3],
        glyph.band_count_y,
    ) as usize;
    let horizontal_header = load_directional_band_header(
        band_data,
        horizontal_band_header_index(glyph, horizontal_band),
    );
    let horizontal_direction = if render_coord[0] < horizontal_header.split {
        CoverageRayDirection::Left
    } else {
        CoverageRayDirection::Right
    };
    let vertical_band = band_index(
        render_coord[0],
        glyph.band_transform[0],
        glyph.band_transform[2],
        glyph.band_count_x,
    ) as usize;
    let vertical_header =
        load_directional_band_header(band_data, vertical_band_header_index(glyph, vertical_band));
    let vertical_direction = if render_coord[1] < vertical_header.split {
        CoverageRayDirection::Left
    } else {
        CoverageRayDirection::Right
    };

    for curve in curves.iter().skip(start).take(end.saturating_sub(start)) {
        accumulate_horizontal_curve_coverage(
            curve,
            render_coord,
            pixels_per_em,
            horizontal_direction,
            &mut xcov,
            &mut xwgt,
        );
        accumulate_vertical_curve_coverage(
            curve,
            render_coord,
            pixels_per_em,
            vertical_direction,
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
    band_data: &[u32],
    glyph: SlugGlyph,
) -> f32 {
    let sample_step = 0.25 / pixels_per_em.max(1.0 / 65536.0);
    let sample_offsets = [
        [-sample_step, -sample_step],
        [sample_step, -sample_step],
        [-sample_step, sample_step],
        [sample_step, sample_step],
    ];
    let mut coverage = 0.0;
    for offset in sample_offsets {
        coverage += cpu_slug_coverage_all_curves_single_sample(
            [render_coord[0] + offset[0], render_coord[1] + offset[1]],
            pixels_per_em,
            curves,
            band_data,
            glyph,
        );
    }
    coverage * 0.25
}

fn is_degenerate_quadratic(curve: &QuadraticCurve) -> bool {
    let epsilon = 1.0 / 1024.0;
    let ax = curve.p0[0] - (curve.p1[0] * 2.0) + curve.p2[0];
    let ay = curve.p0[1] - (curve.p1[1] * 2.0) + curve.p2[1];
    ax.abs() <= epsilon && ay.abs() <= epsilon
}

fn should_use_degenerate_line_fallback(curve: &QuadraticCurve) -> bool {
    is_degenerate_quadratic(curve)
}

fn apply_degenerate_horizontal_coverage(
    curve: &QuadraticCurve,
    render_coord: [f32; 2],
    pixels_per_em: f32,
    direction: CoverageRayDirection,
    xcov: &mut f32,
    xwgt: &mut f32,
) {
    let p0 = [
        curve.p0[0] - render_coord[0],
        curve.p0[1] - render_coord[1] + SLUG_HORIZONTAL_COVERAGE_EPSILON,
    ];
    let p1 = [
        curve.p2[0] - render_coord[0],
        curve.p2[1] - render_coord[1] + SLUG_HORIZONTAL_COVERAGE_EPSILON,
    ];
    if let Some(intersection_x) = horizontal_line_intersection(p0, p1) {
        let signed_distance = intersection_x * pixels_per_em;
        let sample = match direction {
            CoverageRayDirection::Left => saturate(0.5 - signed_distance),
            CoverageRayDirection::Right => saturate(signed_distance + 0.5),
        };
        if p1[1] > p0[1] {
            *xcov += sample;
        } else {
            *xcov -= sample;
        }
        *xwgt = (*xwgt).max(saturate(1.0 - signed_distance.abs() * 2.0));
    }
}

fn apply_degenerate_vertical_coverage(
    curve: &QuadraticCurve,
    render_coord: [f32; 2],
    pixels_per_em: f32,
    direction: CoverageRayDirection,
    ycov: &mut f32,
    ywgt: &mut f32,
) {
    let p0 = [curve.p0[0] - render_coord[0], curve.p0[1] - render_coord[1]];
    let p1 = [curve.p2[0] - render_coord[0], curve.p2[1] - render_coord[1]];
    if let Some(intersection_y) = vertical_line_intersection(p0, p1) {
        let signed_distance = intersection_y * pixels_per_em;
        let sample = match direction {
            CoverageRayDirection::Left => saturate(0.5 - signed_distance),
            CoverageRayDirection::Right => saturate(signed_distance + 0.5),
        };
        if p1[0] > p0[0] {
            *ycov += sample;
        } else {
            *ycov -= sample;
        }
        *ywgt = (*ywgt).max(saturate(1.0 - signed_distance.abs() * 2.0));
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
    direction: CoverageRayDirection,
    xcov: &mut f32,
    xwgt: &mut f32,
) {
    if should_use_degenerate_line_fallback(curve) {
        apply_degenerate_horizontal_coverage(
            curve,
            render_coord,
            pixels_per_em,
            direction,
            xcov,
            xwgt,
        );
        return;
    }

    let p12 = [
        curve.p0[0] - render_coord[0],
        curve.p0[1] - render_coord[1] + SLUG_HORIZONTAL_COVERAGE_EPSILON,
        curve.p1[0] - render_coord[0],
        curve.p1[1] - render_coord[1] + SLUG_HORIZONTAL_COVERAGE_EPSILON,
    ];
    let p3 = [
        curve.p2[0] - render_coord[0],
        curve.p2[1] - render_coord[1] + SLUG_HORIZONTAL_COVERAGE_EPSILON,
    ];
    let hcode = calc_root_code(p12[1], p12[3], p3[1]);
    if hcode == 0 {
        return;
    }

    let hr = solve_horiz_poly(p12, p3);
    if (hcode & 1) != 0 {
        let signed_distance = hr[0] * pixels_per_em;
        let sample = match direction {
            CoverageRayDirection::Left => saturate(0.5 - signed_distance),
            CoverageRayDirection::Right => saturate(signed_distance + 0.5),
        };
        *xcov += sample;
        *xwgt = (*xwgt).max(saturate(1.0 - signed_distance.abs() * 2.0));
    }
    if hcode > 1 {
        let signed_distance = hr[1] * pixels_per_em;
        let sample = match direction {
            CoverageRayDirection::Left => saturate(0.5 - signed_distance),
            CoverageRayDirection::Right => saturate(signed_distance + 0.5),
        };
        *xcov -= sample;
        *xwgt = (*xwgt).max(saturate(1.0 - signed_distance.abs() * 2.0));
    }
}

fn accumulate_vertical_curve_coverage(
    curve: &QuadraticCurve,
    render_coord: [f32; 2],
    pixels_per_em: f32,
    direction: CoverageRayDirection,
    ycov: &mut f32,
    ywgt: &mut f32,
) {
    if should_use_degenerate_line_fallback(curve) {
        apply_degenerate_vertical_coverage(
            curve,
            render_coord,
            pixels_per_em,
            direction,
            ycov,
            ywgt,
        );
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
        let signed_distance = vr[0] * pixels_per_em;
        let sample = match direction {
            CoverageRayDirection::Left => saturate(0.5 - signed_distance),
            CoverageRayDirection::Right => saturate(signed_distance + 0.5),
        };
        *ycov -= sample;
        *ywgt = (*ywgt).max(saturate(1.0 - signed_distance.abs() * 2.0));
    }
    if vcode > 1 {
        let signed_distance = vr[1] * pixels_per_em;
        let sample = match direction {
            CoverageRayDirection::Left => saturate(0.5 - signed_distance),
            CoverageRayDirection::Right => saturate(signed_distance + 0.5),
        };
        *ycov += sample;
        *ywgt = (*ywgt).max(saturate(1.0 - signed_distance.abs() * 2.0));
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

fn maybe_spawn_terminal_font_prewarm()
-> eyre::Result<Option<thread::JoinHandle<eyre::Result<Arc<LoadedTerminalFont>>>>> {
    if TERMINAL_FONT_CACHE.get().is_some() {
        return Ok(None);
    }

    thread::Builder::new()
        .name("teamy-d3d12-font-prewarm".to_owned())
        .spawn(|| info_span!("load_terminal_font").in_scope(cached_terminal_font))
        .map(Some)
        .map_err(|error| eyre::eyre!("failed to spawn terminal font prewarm: {error}"))
}

fn maybe_spawn_sprite_atlas_prewarm()
-> eyre::Result<Option<thread::JoinHandle<eyre::Result<Arc<SpriteAtlas>>>>> {
    if SPRITE_ATLAS_CACHE.get().is_some() {
        return Ok(None);
    }

    thread::Builder::new()
        .name("teamy-d3d12-sprite-atlas-prewarm".to_owned())
        .spawn(|| info_span!("build_sprite_atlas").in_scope(cached_sprite_atlas))
        .map(Some)
        .map_err(|error| eyre::eyre!("failed to spawn sprite atlas prewarm: {error}"))
}

fn maybe_spawn_compiled_shader_prewarm()
-> eyre::Result<Option<thread::JoinHandle<eyre::Result<()>>>> {
    if COMPILED_SHADER_CACHE.get().is_some() {
        return Ok(None);
    }

    thread::Builder::new()
        .name("teamy-d3d12-shader-prewarm".to_owned())
        .spawn(|| {
            info_span!("compile_pipeline_shaders")
                .in_scope(|| cached_compiled_shaders().map(|_| ()))
        })
        .map(Some)
        .map_err(|error| eyre::eyre!("failed to spawn compiled shader prewarm: {error}"))
}

fn await_renderer_startup_task<T, F>(
    handle: Option<thread::JoinHandle<eyre::Result<T>>>,
    fallback: F,
    task_name: &str,
) -> eyre::Result<T>
where
    F: FnOnce() -> eyre::Result<T>,
{
    match handle {
        Some(handle) => match handle.join() {
            Ok(result) => result.wrap_err_with(|| format!("{task_name} failed")),
            Err(_panic) => Err(eyre::eyre!("{task_name} panicked")),
        },
        None => fallback(),
    }
}

fn create_device() -> eyre::Result<(IDXGIFactory4, ID3D12Device, Option<IDXGIInfoQueue>)> {
    create_device_with_adapter(false)
}

fn create_device_with_adapter(
    use_warp_adapter: bool,
) -> eyre::Result<(IDXGIFactory4, ID3D12Device, Option<IDXGIInfoQueue>)> {
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
    let adapter = if use_warp_adapter {
        let adapter: IDXGIAdapter1 = unsafe { dxgi_factory.EnumWarpAdapter() }?;
        adapter
    } else {
        get_hardware_adapter(&dxgi_factory)?
    };

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
    width: u32,
    height: u32,
) -> eyre::Result<IDXGISwapChain3> {
    let factory: IDXGIFactory2 = factory.cast()?;
    let description = composition_swap_chain_description(width, height);
    let swap_chain: IDXGISwapChain1 =
        unsafe { factory.CreateSwapChainForComposition(command_queue, &description, None)? };
    Ok(swap_chain.cast()?)
}

fn composition_swap_chain_description(width: u32, height: u32) -> DXGI_SWAP_CHAIN_DESC1 {
    DXGI_SWAP_CHAIN_DESC1 {
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
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
        Flags: DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0 as u32,
    }
}

fn attach_swap_chain_to_window(
    hwnd: HWND,
    swap_chain: &IDXGISwapChain3,
) -> eyre::Result<(
    IDCompositionDevice,
    IDCompositionTarget,
    IDCompositionVisual,
)> {
    let dcomp_device: IDCompositionDevice =
        unsafe { DCompositionCreateDevice::<_, IDCompositionDevice>(None::<&IDXGIDevice>) }?;
    let dcomp_target = unsafe { dcomp_device.CreateTargetForHwnd(hwnd, true) }?;
    let dcomp_visual = unsafe { dcomp_device.CreateVisual() }?;

    unsafe {
        dcomp_visual.SetContent(swap_chain)?;
        dcomp_target.SetRoot(&dcomp_visual)?;
        dcomp_device.Commit()?;
    }

    Ok((dcomp_device, dcomp_target, dcomp_visual))
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
        NumDescriptors: 4,
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
    let shaders = cached_compiled_shaders()?;

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
            SemanticName: s!("LOCALBOUNDS"),
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            AlignedByteOffset: 100,
            ..Default::default()
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("VIEWPORT"),
            Format: DXGI_FORMAT_R32G32_FLOAT,
            AlignedByteOffset: 116,
            ..Default::default()
        },
    ];

    let blend_target = D3D12_RENDER_TARGET_BLEND_DESC {
        BlendEnable: TRUE,
        LogicOpEnable: false.into(),
        SrcBlend: D3D12_BLEND_ONE,
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
        VS: shader_bytecode_slice(&shaders.vertex),
        PS: shader_bytecode_slice(&shaders.pixel),
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

fn cached_compiled_shaders() -> eyre::Result<&'static CompiledShaders> {
    COMPILED_SHADER_CACHE
        .get_or_init(|| compile_shaders_for_cache().map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| eyre::eyre!(error.clone()))
}

fn compile_shaders_for_cache() -> eyre::Result<CompiledShaders> {
    compile_embedded_shaders(shader_compile_flags())
}

#[derive(Debug)]
struct IncludedShaderSource {
    _bytes: Box<[u8]>,
    path: String,
}

#[derive(Debug)]
struct ShaderIncludeHandler {
    embedded_sources: HashMap<&'static str, &'static str>,
    root_path: &'static str,
    sources: Mutex<HashMap<usize, IncludedShaderSource>>,
}

#[derive(Debug)]
struct CompiledShaders {
    vertex: Vec<u8>,
    pixel: Vec<u8>,
}

impl ShaderIncludeHandler {
    fn new(root_path: &'static str) -> Self {
        Self {
            embedded_sources: embedded_shader_sources(),
            root_path,
            sources: Mutex::new(HashMap::new()),
        }
    }

    fn resolve_parent_path(&self, parent_data: *const c_void) -> String {
        if parent_data.is_null() {
            return self.root_path.to_owned();
        }

        self.sources
            .lock()
            .expect("shader include cache should not be poisoned")
            .get(&(parent_data as usize))
            .map_or_else(|| self.root_path.to_owned(), |source| source.path.clone())
    }

    fn resolve_include_path(&self, file_name: &str, parent_data: *const c_void) -> String {
        let parent_path = self.resolve_parent_path(parent_data);
        normalize_shader_virtual_path(
            Path::new(&parent_path)
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(file_name),
        )
    }

    fn load_embedded_source(&self, path: &str) -> windows::core::Result<&'static str> {
        self.embedded_sources.get(path).copied().ok_or_else(|| {
            shader_include_error(format!(
                "failed to resolve embedded shader include `{path}`"
            ))
        })
    }
}

impl ID3DInclude_Impl for ShaderIncludeHandler {
    fn Open(
        &self,
        _includetype: D3D_INCLUDE_TYPE,
        pfilename: &PCSTR,
        pparentdata: *const c_void,
        ppdata: *mut *mut c_void,
        pbytes: *mut u32,
    ) -> windows::core::Result<()> {
        if ppdata.is_null() || pbytes.is_null() {
            return Err(shader_include_error(
                "shader include output pointers were unexpectedly null",
            ));
        }

        let file_name = shader_include_file_name(pfilename)?;
        let include_path = self.resolve_include_path(file_name, pparentdata);
        let bytes = self
            .load_embedded_source(&include_path)?
            .as_bytes()
            .to_vec();
        let byte_len = u32::try_from(bytes.len()).map_err(|_conversion_error| {
            shader_include_error(format!(
                "shader include `{include_path}` exceeded the D3D compiler size limit"
            ))
        })?;
        let bytes = bytes.into_boxed_slice();
        let pointer = bytes.as_ptr() as *mut c_void;

        self.sources
            .lock()
            .expect("shader include cache should not be poisoned")
            .insert(
                pointer as usize,
                IncludedShaderSource {
                    _bytes: bytes,
                    path: include_path,
                },
            );

        unsafe {
            *ppdata = pointer;
            *pbytes = byte_len;
        }

        Ok(())
    }

    fn Close(&self, pdata: *const c_void) -> windows::core::Result<()> {
        if pdata.is_null() {
            return Ok(());
        }

        self.sources
            .lock()
            .expect("shader include cache should not be poisoned")
            .remove(&(pdata as usize));
        Ok(())
    }
}

fn shader_include_file_name(file_name: &PCSTR) -> windows::core::Result<&str> {
    if file_name.0.is_null() {
        return Err(shader_include_error(
            "shader include file name pointer was unexpectedly null",
        ));
    }

    unsafe { CStr::from_ptr(file_name.0 as *const i8) }
        .to_str()
        .map_err(|error| shader_include_error(format!("invalid shader include file name: {error}")))
}

fn shader_include_error(message: impl Into<String>) -> Error {
    Error::new(E_FAIL, message.into())
}

fn shader_compile_flags() -> u32 {
    if cfg!(debug_assertions) {
        D3DCOMPILE_DEBUG | D3DCOMPILE_SKIP_OPTIMIZATION
    } else {
        0
    }
}

fn compile_embedded_shaders(flags: u32) -> eyre::Result<CompiledShaders> {
    let vertex_shader = compile_shader(
        WINDOWS_PANEL_SHADERS_PATH,
        WINDOWS_PANEL_SHADERS_SOURCE,
        s!("VSMain"),
        s!("vs_5_0"),
        flags,
    )?;
    let pixel_shader = compile_shader(
        WINDOWS_PANEL_SHADERS_PATH,
        WINDOWS_PANEL_SHADERS_SOURCE,
        s!("PSMain"),
        s!("ps_5_0"),
        flags,
    )?;

    Ok(CompiledShaders {
        vertex: shader_blob_bytes(&vertex_shader),
        pixel: shader_blob_bytes(&pixel_shader),
    })
}

fn compile_shader(
    source_path: &'static str,
    source_text: &'static str,
    entry_point: PCSTR,
    target: PCSTR,
    flags: u32,
) -> eyre::Result<ID3DBlob> {
    let mut shader = None;
    let mut error = None;
    let include_handler = ShaderIncludeHandler::new(source_path);
    let include = ID3DInclude::new(&include_handler);
    let source_name =
        CString::new(source_path).expect("embedded shader path should not contain NUL");
    unsafe {
        D3DCompile(
            source_text.as_ptr() as *const c_void,
            source_text.len(),
            PCSTR(source_name.as_ptr() as *const u8),
            None,
            &*include,
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

fn shader_blob_bytes(shader: &ID3DBlob) -> Vec<u8> {
    unsafe {
        std::slice::from_raw_parts(
            shader.GetBufferPointer() as *const u8,
            shader.GetBufferSize(),
        )
    }
    .to_vec()
}

fn normalize_shader_virtual_path(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().replace('\\', "/")
}

fn embedded_shader_sources() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        (WINDOWS_PANEL_SHADERS_PATH, WINDOWS_PANEL_SHADERS_SOURCE),
        (WINDOWS_CHROME_SHADERS_PATH, WINDOWS_CHROME_SHADERS_SOURCE),
    ])
}

fn shader_bytecode_slice(shader: &[u8]) -> D3D12_SHADER_BYTECODE {
    D3D12_SHADER_BYTECODE {
        pShaderBytecode: shader.as_ptr() as *const c_void,
        BytecodeLength: shader.len(),
    }
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
        transformed_text_clip_rect: [-1.0, -1.0, -1.0, -1.0],
        transformed_text_debug_hover: [0.0; 4],
        transformed_text_projection: [[0.0; 4]; 2],
        transformed_text_inverse_homography: [[0.0; 4]; 2],
        sprite_atlas: [1.0, 1.0, 0.0, 0.0],
    }
}

fn transformed_text_projection_for_scene(scene: &RenderScene) -> Option<[[f32; 4]; 2]> {
    let basis = scene.transformed_text_plane_basis?;
    let (yaw_sin, yaw_cos) = basis.yaw_radians.sin_cos();
    let (pitch_sin, pitch_cos) = basis.pitch_radians.sin_cos();
    Some([
        [
            basis.screen_center[0],
            basis.screen_center[1],
            basis.camera_distance,
            basis.near_plane_distance,
        ],
        [yaw_sin, yaw_cos, pitch_sin, pitch_cos],
    ])
}

fn solve_inverse_homography(
    screen_points: [[f32; 2]; 4],
    local_points: [[f32; 2]; 4],
) -> Option<[f32; 8]> {
    let mut augmented = [[0.0f32; 9]; 8];
    for (point_index, (screen_point, local_point)) in
        screen_points.into_iter().zip(local_points).enumerate()
    {
        let row = point_index * 2;
        let x = screen_point[0];
        let y = screen_point[1];
        let u = local_point[0];
        let v = local_point[1];
        augmented[row] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        augmented[row + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
    }

    for pivot_index in 0..8 {
        let (best_row, best_value) = (pivot_index..8)
            .map(|row| (row, augmented[row][pivot_index].abs()))
            .max_by(|left, right| {
                left.1
                    .partial_cmp(&right.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })?;
        if best_value <= 1.0 / 65536.0 {
            return None;
        }
        if best_row != pivot_index {
            augmented.swap(best_row, pivot_index);
        }

        let pivot = augmented[pivot_index][pivot_index];
        for value in augmented[pivot_index].iter_mut().skip(pivot_index) {
            *value /= pivot;
        }

        for row in 0..8 {
            if row == pivot_index {
                continue;
            }
            let factor = augmented[row][pivot_index];
            if factor.abs() <= f32::EPSILON {
                continue;
            }
            let pivot_row = augmented[pivot_index];
            for (column, value) in augmented[row].iter_mut().enumerate().skip(pivot_index) {
                *value -= factor * pivot_row[column];
            }
        }
    }

    Some(std::array::from_fn(|index| augmented[index][8]))
}

fn transformed_text_inverse_homography_for_scene(scene: &RenderScene) -> Option<[[f32; 4]; 2]> {
    let basis = scene.transformed_text_plane_basis?;
    let inverse = solve_inverse_homography(basis.screen_corners, basis.local_corners)?;
    Some([
        [inverse[0], inverse[1], inverse[2], inverse[6]],
        [inverse[3], inverse[4], inverse[5], inverse[7]],
    ])
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

fn build_sprite_atlas() -> SpriteAtlas {
    let width = SPRITE_SLOT_SIZE * SPRITE_ATLAS_COLUMNS;
    let height = SPRITE_SLOT_SIZE * SPRITE_ATLAS_ROWS;
    let mut atlas = RgbaImage::new(width, height);

    let terminal = blit_sprite_into_slot(&mut atlas, 0, &generate_terminal_sprite());
    let storage = blit_sprite_into_slot(&mut atlas, 1, &generate_storage_sprite());
    let audio = blit_sprite_into_slot(&mut atlas, 2, &generate_audio_sprite());
    let windows_audio = blit_sprite_into_slot(&mut atlas, 3, &generate_windows_audio_sprite());
    let file_audio = blit_sprite_into_slot(&mut atlas, 4, &generate_file_audio_sprite());
    let back = blit_sprite_into_slot(&mut atlas, 5, &generate_back_sprite());
    let transcription = blit_sprite_into_slot(&mut atlas, 6, &generate_transcription_sprite());
    // windowing[impl cursor-gallery.stock-os-cursors]
    // windowing[impl virtual-cursor.sdf-shader-roadmap]
    let cursor_arrow =
        blit_sprite_into_slot(&mut atlas, 7, &generate_stock_cursor_sprite(IDC_ARROW));
    let cursor_hand = blit_sprite_into_slot(&mut atlas, 8, &generate_stock_cursor_sprite(IDC_HAND));
    let cursor_ibeam =
        blit_sprite_into_slot(&mut atlas, 9, &generate_stock_cursor_sprite(IDC_IBEAM));
    let cursor_cross =
        blit_sprite_into_slot(&mut atlas, 10, &generate_stock_cursor_sprite(IDC_CROSS));
    let cursor_wait =
        blit_sprite_into_slot(&mut atlas, 11, &generate_stock_cursor_sprite(IDC_WAIT));
    let cursor_size_all =
        blit_sprite_into_slot(&mut atlas, 12, &generate_stock_cursor_sprite(IDC_SIZEALL));
    let cursor_help =
        blit_sprite_into_slot(&mut atlas, 13, &generate_stock_cursor_sprite(IDC_HELP));

    SpriteAtlas {
        width,
        height,
        pixels: atlas.pixels().map(|pixel| pack_rgba8(pixel.0)).collect(),
        terminal,
        storage,
        audio,
        windows_audio,
        file_audio,
        back,
        transcription,
        cursor_arrow,
        cursor_hand,
        cursor_ibeam,
        cursor_cross,
        cursor_wait,
        cursor_size_all,
        cursor_help,
    }
}

fn cached_sprite_atlas() -> eyre::Result<Arc<SpriteAtlas>> {
    SPRITE_ATLAS_CACHE
        .get_or_init(|| Ok(Arc::new(build_sprite_atlas())))
        .as_ref()
        .map(Arc::clone)
        .map_err(|error| eyre::eyre!(error.clone()))
}

fn blit_sprite_into_slot(
    atlas: &mut RgbaImage,
    slot_index: u32,
    sprite: &RgbaImage,
) -> AtlasSprite {
    let slot_x = (slot_index % SPRITE_ATLAS_COLUMNS) * SPRITE_SLOT_SIZE;
    let slot_y = (slot_index / SPRITE_ATLAS_COLUMNS) * SPRITE_SLOT_SIZE;
    let fitted = fit_sprite_to_target(sprite, SPRITE_TARGET_SIZE);
    let sprite_x = slot_x + ((SPRITE_SLOT_SIZE - fitted.width()) / 2);
    let sprite_y = slot_y + ((SPRITE_SLOT_SIZE - fitted.height()) / 2);

    for (y, row) in fitted.rows().enumerate() {
        for (x, pixel) in row.enumerate() {
            atlas.put_pixel(
                sprite_x + u32::try_from(x).unwrap_or_default(),
                sprite_y + u32::try_from(y).unwrap_or_default(),
                *pixel,
            );
        }
    }

    AtlasSprite {
        uv_left: sprite_x as f32 / atlas.width() as f32,
        uv_top: sprite_y as f32 / atlas.height() as f32,
        uv_right: (sprite_x + fitted.width()) as f32 / atlas.width() as f32,
        uv_bottom: (sprite_y + fitted.height()) as f32 / atlas.height() as f32,
    }
}

fn generate_stock_cursor_sprite(cursor_name: PCWSTR) -> RgbaImage {
    windows_stock_cursor_sprite(cursor_name).unwrap_or_else(generate_fallback_cursor_sprite)
}

fn windows_stock_cursor_sprite(cursor_name: PCWSTR) -> Option<RgbaImage> {
    let cursor = unsafe { LoadCursorW(None, cursor_name).ok()? };
    unsafe { draw_icon_to_rgba_image(HICON(cursor.0), SPRITE_TARGET_SIZE).ok() }
}

fn generate_fallback_cursor_sprite() -> RgbaImage {
    let mut image = RgbaImage::new(SPRITE_TARGET_SIZE, SPRITE_TARGET_SIZE);
    fill_cursor_triangle(&mut image, [250, 250, 255, 255]);
    stroke_cursor_triangle(&mut image, [30, 36, 48, 255]);
    image
}

fn fit_sprite_to_target(sprite: &RgbaImage, target_size: u32) -> RgbaImage {
    let width = sprite.width().max(1);
    let height = sprite.height().max(1);
    let scale = (target_size as f32 / width as f32).min(target_size as f32 / height as f32);
    let target_width = ((width as f32 * scale).round() as u32).max(1);
    let target_height = ((height as f32 * scale).round() as u32).max(1);
    resize(sprite, target_width, target_height, FilterType::Lanczos3)
}

fn generate_audio_sprite() -> RgbaImage {
    // audio[impl gui.windows-icon-sprite]
    windows_microphone_icon_sprite().unwrap_or_else(generate_fallback_audio_sprite)
}

fn generate_terminal_sprite() -> RgbaImage {
    let mut image = RgbaImage::new(SPRITE_TARGET_SIZE, SPRITE_TARGET_SIZE);
    fill_rect(&mut image, 30, 44, 226, 190, [30, 42, 58, 255]);
    fill_rect(&mut image, 42, 56, 214, 178, [10, 16, 24, 255]);
    fill_rect(&mut image, 62, 88, 82, 108, [96, 220, 255, 255]);
    fill_rect(&mut image, 82, 108, 110, 128, [96, 220, 255, 255]);
    fill_rect(&mut image, 122, 128, 188, 144, [96, 220, 255, 255]);
    fill_rect(&mut image, 88, 202, 168, 218, [64, 82, 108, 255]);
    fill_rect(&mut image, 64, 218, 192, 232, [44, 58, 78, 255]);
    image
}

fn generate_storage_sprite() -> RgbaImage {
    let mut image = RgbaImage::new(SPRITE_TARGET_SIZE, SPRITE_TARGET_SIZE);
    for offset in [0_u32, 58, 116] {
        let top = 42 + offset;
        let bottom = top + 44;
        fill_rect(&mut image, 36, top, 220, bottom, [74, 120, 92, 255]);
        fill_rect(
            &mut image,
            48,
            top + 10,
            208,
            bottom - 10,
            [18, 32, 24, 255],
        );
        fill_rect(
            &mut image,
            64,
            top + 16,
            84,
            bottom - 16,
            [124, 220, 162, 255],
        );
        fill_rect(
            &mut image,
            178,
            top + 16,
            194,
            bottom - 16,
            [96, 164, 122, 255],
        );
    }
    fill_rect(&mut image, 56, 206, 200, 222, [56, 84, 68, 255]);
    image
}

fn generate_back_sprite() -> RgbaImage {
    let mut image = RgbaImage::new(SPRITE_TARGET_SIZE, SPRITE_TARGET_SIZE);
    fill_rect(&mut image, 62, 118, 198, 142, [226, 236, 248, 255]);
    fill_rect(&mut image, 62, 118, 110, 166, [226, 236, 248, 255]);
    fill_rect(&mut image, 62, 94, 110, 142, [226, 236, 248, 255]);
    fill_rect(&mut image, 38, 118, 86, 142, [166, 206, 255, 255]);
    fill_rect(&mut image, 62, 94, 86, 118, [166, 206, 255, 255]);
    fill_rect(&mut image, 62, 142, 86, 166, [166, 206, 255, 255]);
    image
}

fn generate_transcription_sprite() -> RgbaImage {
    let mut image = RgbaImage::new(SPRITE_TARGET_SIZE, SPRITE_TARGET_SIZE);
    fill_rect(&mut image, 42, 58, 214, 178, [46, 38, 66, 255]);
    fill_rect(&mut image, 58, 74, 198, 162, [20, 16, 34, 255]);
    fill_rect(&mut image, 74, 94, 110, 110, [248, 208, 124, 255]);
    fill_rect(&mut image, 122, 94, 182, 110, [248, 208, 124, 255]);
    fill_rect(&mut image, 74, 126, 162, 142, [196, 146, 255, 255]);
    fill_rect(&mut image, 74, 158, 138, 174, [120, 220, 255, 255]);
    fill_rect(&mut image, 106, 178, 142, 206, [46, 38, 66, 255]);
    fill_rect(&mut image, 122, 174, 154, 190, [20, 16, 34, 255]);
    image
}

fn generate_fallback_audio_sprite() -> RgbaImage {
    let mut image = RgbaImage::new(SPRITE_TARGET_SIZE, SPRITE_TARGET_SIZE);
    fill_circle(&mut image, 88.0, 176.0, 36.0, [245, 199, 96, 255]);
    fill_circle(&mut image, 158.0, 160.0, 34.0, [245, 199, 96, 255]);
    fill_rect(&mut image, 150, 64, 182, 168, [245, 199, 96, 255]);
    fill_rect(&mut image, 178, 64, 210, 104, [245, 199, 96, 255]);
    stroke_ring(&mut image, 168.0, 136.0, 66.0, 8.0, [92, 206, 255, 220]);
    stroke_ring(&mut image, 168.0, 136.0, 92.0, 8.0, [92, 206, 255, 180]);
    image
}

#[expect(
    clippy::undocumented_unsafe_blocks,
    reason = "loading and drawing a stock Windows icon requires Win32 handle calls"
)]
fn windows_microphone_icon_sprite() -> Option<RgbaImage> {
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_owned());
    let dll_path = format!("{system_root}\\system32\\mmres.dll");
    let dll_path = dll_path.easy_pcwstr().ok()?;
    let module = unsafe { LoadLibraryW(dll_path.as_ref()).ok()? };
    let _module_guard = ModuleGuard(module);
    let resource = int_resource_pcwstr(3012);
    let icon_handle = unsafe {
        LoadImageW(
            Some(module.into()),
            resource,
            IMAGE_ICON,
            i32::try_from(SPRITE_TARGET_SIZE).ok()?,
            i32::try_from(SPRITE_TARGET_SIZE).ok()?,
            IMAGE_FLAGS::default(),
        )
        .ok()?
    };
    let icon = HICON(icon_handle.0);
    let _icon_guard = IconGuard(icon);
    unsafe { draw_icon_to_rgba_image(icon, SPRITE_TARGET_SIZE).ok() }
}

#[expect(
    clippy::undocumented_unsafe_blocks,
    reason = "the DIB section exposes a temporary pixel buffer owned by GDI for icon drawing"
)]
unsafe fn draw_icon_to_rgba_image(icon: HICON, size: u32) -> eyre::Result<RgbaImage> {
    let size_i32 = i32::try_from(size).wrap_err("icon target size did not fit in i32")?;
    let screen_dc = unsafe { GetDC(None) };
    if screen_dc.is_invalid() {
        eyre::bail!("failed to acquire screen DC for icon rendering")
    }
    let _screen_dc_guard = ReleaseDcGuard(screen_dc);
    let memory_dc = unsafe { CreateCompatibleDC(Some(screen_dc)) };
    if memory_dc.is_invalid() {
        eyre::bail!("failed to create memory DC for icon rendering")
    }
    let _memory_dc_guard = DeleteDcGuard(memory_dc);

    let mut bitmap_info = BITMAPINFO::default();
    bitmap_info.bmiHeader.biSize = std::mem::size_of_val(&bitmap_info.bmiHeader) as u32;
    bitmap_info.bmiHeader.biWidth = size_i32;
    bitmap_info.bmiHeader.biHeight = -size_i32;
    bitmap_info.bmiHeader.biPlanes = 1;
    bitmap_info.bmiHeader.biBitCount = 32;
    bitmap_info.bmiHeader.biCompression = BI_RGB.0;
    let mut bits = std::ptr::null_mut();
    let bitmap = unsafe {
        CreateDIBSection(
            Some(memory_dc),
            &raw const bitmap_info,
            DIB_RGB_COLORS,
            &raw mut bits,
            None,
            0,
        )?
    };
    let _bitmap_guard = BitmapGuard(bitmap);
    let old_object = unsafe { SelectObject(memory_dc, bitmap.into()) };
    let _select_guard = SelectObjectGuard {
        dc: memory_dc,
        old_object,
    };
    unsafe {
        DrawIconEx(
            memory_dc, 0, 0, icon, size_i32, size_i32, 0, None, DI_NORMAL,
        )?
    };

    let byte_len = usize::try_from(size * size * 4).wrap_err("icon byte length overflowed")?;
    let bgra = unsafe { std::slice::from_raw_parts(bits.cast::<u8>(), byte_len) };
    let mut rgba = vec![0_u8; byte_len];
    for (source, destination) in bgra.chunks_exact(4).zip(rgba.chunks_exact_mut(4)) {
        destination[0] = source[2];
        destination[1] = source[1];
        destination[2] = source[0];
        destination[3] = source[3];
    }
    ImageBuffer::from_vec(size, size, rgba).ok_or_else(|| eyre::eyre!("failed to build icon image"))
}

struct ModuleGuard(windows::Win32::Foundation::HMODULE);

impl Drop for ModuleGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe { _ = FreeLibrary(self.0) };
        }
    }
}

struct IconGuard(HICON);

impl Drop for IconGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe { _ = DestroyIcon(self.0) };
        }
    }
}

struct ReleaseDcGuard(HDC);

impl Drop for ReleaseDcGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe { _ = ReleaseDC(None, self.0) };
        }
    }
}

struct DeleteDcGuard(HDC);

impl Drop for DeleteDcGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe { _ = DeleteDC(self.0) };
        }
    }
}

struct BitmapGuard(HBITMAP);

impl Drop for BitmapGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe { _ = DeleteObject(self.0.into()) };
        }
    }
}

struct SelectObjectGuard {
    dc: HDC,
    old_object: HGDIOBJ,
}

impl Drop for SelectObjectGuard {
    fn drop(&mut self) {
        if !self.dc.is_invalid() && !self.old_object.is_invalid() {
            unsafe { _ = SelectObject(self.dc, self.old_object) };
        }
    }
}

fn generate_windows_audio_sprite() -> RgbaImage {
    let mut image = RgbaImage::new(SPRITE_TARGET_SIZE, SPRITE_TARGET_SIZE);
    fill_rect(&mut image, 36, 42, 118, 124, [97, 195, 255, 255]);
    fill_rect(&mut image, 132, 36, 220, 124, [97, 195, 255, 255]);
    fill_rect(&mut image, 36, 138, 118, 220, [36, 119, 252, 255]);
    fill_rect(&mut image, 132, 132, 220, 220, [36, 119, 252, 255]);
    fill_rect(&mut image, 94, 76, 116, 220, [8, 20, 36, 230]);
    fill_rect(&mut image, 36, 118, 220, 140, [8, 20, 36, 230]);
    image
}

fn generate_file_audio_sprite() -> RgbaImage {
    let mut image = RgbaImage::new(SPRITE_TARGET_SIZE, SPRITE_TARGET_SIZE);
    fill_rect(&mut image, 56, 28, 196, 224, [242, 244, 250, 255]);
    fill_rect(&mut image, 164, 28, 220, 84, [201, 220, 255, 255]);
    fill_rect(&mut image, 92, 88, 176, 104, [113, 149, 220, 255]);
    fill_rect(&mut image, 92, 122, 184, 138, [113, 149, 220, 255]);
    fill_rect(&mut image, 92, 156, 160, 172, [113, 149, 220, 255]);
    image
}

fn fill_rect(image: &mut RgbaImage, left: u32, top: u32, right: u32, bottom: u32, color: [u8; 4]) {
    for y in top.min(image.height())..bottom.min(image.height()) {
        for x in left.min(image.width())..right.min(image.width()) {
            image.put_pixel(x, y, Rgba(color));
        }
    }
}

fn fill_cursor_triangle(image: &mut RgbaImage, color: [u8; 4]) {
    let points = [(28.0, 18.0), (28.0, 214.0), (174.0, 142.0)];
    for pixel_y in 0..image.height() {
        for pixel_x in 0..image.width() {
            let point = (pixel_x as f32 + 0.5, pixel_y as f32 + 0.5);
            if point_in_triangle(point, points) {
                image.put_pixel(pixel_x, pixel_y, Rgba(color));
            }
        }
    }
}

fn stroke_cursor_triangle(image: &mut RgbaImage, color: [u8; 4]) {
    let points = [(28.0, 18.0), (28.0, 214.0), (174.0, 142.0)];
    for pixel_y in 0..image.height() {
        for pixel_x in 0..image.width() {
            let point = (pixel_x as f32 + 0.5, pixel_y as f32 + 0.5);
            let near_edge = points
                .iter()
                .zip(points.iter().cycle().skip(1))
                .any(|(start, end)| distance_to_segment(point, *start, *end) <= 5.0);
            if near_edge {
                image.put_pixel(pixel_x, pixel_y, Rgba(color));
            }
        }
    }
}

fn point_in_triangle(point: (f32, f32), points: [(f32, f32); 3]) -> bool {
    let area = triangle_area(points[0], points[1], points[2]);
    let area_a = triangle_area(point, points[1], points[2]);
    let area_b = triangle_area(points[0], point, points[2]);
    let area_c = triangle_area(points[0], points[1], point);
    ((area_a + area_b + area_c) - area).abs() <= 0.5
}

fn triangle_area(first: (f32, f32), second: (f32, f32), third: (f32, f32)) -> f32 {
    ((first.0 * (second.1 - third.1))
        + (second.0 * (third.1 - first.1))
        + (third.0 * (first.1 - second.1)))
        .abs()
        * 0.5
}

fn distance_to_segment(point: (f32, f32), start: (f32, f32), end: (f32, f32)) -> f32 {
    let segment_x = end.0 - start.0;
    let segment_y = end.1 - start.1;
    let length_sq = (segment_x * segment_x) + (segment_y * segment_y);
    if length_sq <= f32::EPSILON {
        let dx = point.0 - start.0;
        let dy = point.1 - start.1;
        return ((dx * dx) + (dy * dy)).sqrt();
    }
    let projection =
        (((point.0 - start.0) * segment_x) + ((point.1 - start.1) * segment_y)) / length_sq;
    let clamped = projection.clamp(0.0, 1.0);
    let closest = (start.0 + segment_x * clamped, start.1 + segment_y * clamped);
    let dx = point.0 - closest.0;
    let dy = point.1 - closest.1;
    ((dx * dx) + (dy * dy)).sqrt()
}

fn fill_circle(image: &mut RgbaImage, center_x: f32, center_y: f32, radius: f32, color: [u8; 4]) {
    let left = (center_x - radius).floor().max(0.0) as u32;
    let top = (center_y - radius).floor().max(0.0) as u32;
    let right = (center_x + radius).ceil().min(image.width() as f32) as u32;
    let bottom = (center_y + radius).ceil().min(image.height() as f32) as u32;
    let radius_sq = radius * radius;

    for y in top..bottom {
        for x in left..right {
            let dx = x as f32 + 0.5 - center_x;
            let dy = y as f32 + 0.5 - center_y;
            if (dx * dx) + (dy * dy) <= radius_sq {
                image.put_pixel(x, y, Rgba(color));
            }
        }
    }
}

fn stroke_ring(
    image: &mut RgbaImage,
    center_x: f32,
    center_y: f32,
    radius: f32,
    thickness: f32,
    color: [u8; 4],
) {
    let left = (center_x - radius - thickness).floor().max(0.0) as u32;
    let top = (center_y - radius - thickness).floor().max(0.0) as u32;
    let right = (center_x + radius + thickness)
        .ceil()
        .min(image.width() as f32) as u32;
    let bottom = (center_y + radius + thickness)
        .ceil()
        .min(image.height() as f32) as u32;
    let outer_sq = (radius + thickness) * (radius + thickness);
    let inner_sq = (radius - thickness).max(0.0) * (radius - thickness).max(0.0);

    for y in top..bottom {
        for x in left..right {
            let dx = x as f32 + 0.5 - center_x;
            let dy = y as f32 + 0.5 - center_y;
            let distance_sq = (dx * dx) + (dy * dy);
            if distance_sq <= outer_sq && distance_sq >= inner_sq {
                image.put_pixel(x, y, Rgba(color));
            }
        }
    }
}

fn pack_rgba8(color: [u8; 4]) -> u32 {
    u32::from(color[0])
        | (u32::from(color[1]) << 8)
        | (u32::from(color[2]) << 16)
        | (u32::from(color[3]) << 24)
}

fn create_shader_resources_and_srv(
    device: &ID3D12Device,
    sprite_atlas: Arc<SpriteAtlas>,
) -> eyre::Result<(
    ID3D12DescriptorHeap,
    ID3D12Resource,
    ID3D12Resource,
    ID3D12Resource,
    ID3D12Resource,
    Arc<SpriteAtlas>,
)> {
    create_shader_resources_and_srv_with_capacities(
        device,
        sprite_atlas,
        MAX_CURVE_FLOAT4_COUNT,
        MAX_BAND_UINT_COUNT,
    )
}

fn create_shader_resources_and_srv_with_capacities(
    device: &ID3D12Device,
    sprite_atlas: Arc<SpriteAtlas>,
    curve_capacity: usize,
    band_capacity: usize,
) -> eyre::Result<(
    ID3D12DescriptorHeap,
    ID3D12Resource,
    ID3D12Resource,
    ID3D12Resource,
    ID3D12Resource,
    Arc<SpriteAtlas>,
)> {
    let curve_data = vec![[0.0_f32; 4]; curve_capacity];
    let byte_len = (curve_data.len() * std::mem::size_of::<[f32; 4]>()) as u64;
    let band_data = vec![0_u32; band_capacity];
    let band_byte_len = (band_data.len() * std::mem::size_of::<u32>()) as u64;
    let transformed_glyph_inverse_data =
        vec![[0.0_f32; 4]; MAX_TRANSFORMED_GLYPH_INVERSE_FLOAT4_COUNT];
    let transformed_glyph_inverse_byte_len =
        (transformed_glyph_inverse_data.len() * std::mem::size_of::<[f32; 4]>()) as u64;
    let sprite_byte_len = (sprite_atlas.pixels.len() * std::mem::size_of::<u32>()) as u64;

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

    let mut transformed_glyph_inverse_buffer = None;
    unsafe {
        device.CreateCommittedResource(
            &D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_UPLOAD,
                ..Default::default()
            },
            D3D12_HEAP_FLAG_NONE,
            &D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                Width: transformed_glyph_inverse_byte_len,
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
            &mut transformed_glyph_inverse_buffer,
        )?
    };
    let transformed_glyph_inverse_buffer: ID3D12Resource = transformed_glyph_inverse_buffer
        .expect("transformed glyph inverse buffer should be initialized");

    let mut sprite_buffer = None;
    unsafe {
        device.CreateCommittedResource(
            &D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_UPLOAD,
                ..Default::default()
            },
            D3D12_HEAP_FLAG_NONE,
            &D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                Width: sprite_byte_len,
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
            &mut sprite_buffer,
        )?
    };
    let sprite_buffer: ID3D12Resource =
        sprite_buffer.expect("sprite atlas buffer should be initialized");

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

        let mut transformed_mapped = std::ptr::null_mut();
        transformed_glyph_inverse_buffer.Map(0, None, Some(&mut transformed_mapped))?;
        std::ptr::copy_nonoverlapping(
            transformed_glyph_inverse_data.as_ptr(),
            transformed_mapped as *mut [f32; 4],
            transformed_glyph_inverse_data.len(),
        );
        transformed_glyph_inverse_buffer.Unmap(0, None);

        let mut sprite_mapped = std::ptr::null_mut();
        sprite_buffer.Map(0, None, Some(&mut sprite_mapped))?;
        std::ptr::copy_nonoverlapping(
            sprite_atlas.pixels.as_ptr(),
            sprite_mapped as *mut u32,
            sprite_atlas.pixels.len(),
        );
        sprite_buffer.Unmap(0, None);
    }

    let srv_heap: ID3D12DescriptorHeap = unsafe {
        device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            NumDescriptors: 4,
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

    let sprite_desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_R32_UINT,
        ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
            Buffer: D3D12_BUFFER_SRV {
                FirstElement: 0,
                NumElements: u32::try_from(sprite_atlas.pixels.len()).unwrap_or(u32::MAX),
                StructureByteStride: 0,
                Flags: D3D12_BUFFER_SRV_FLAG_NONE,
            },
        },
    };

    let transformed_glyph_inverse_desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
        ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
            Buffer: D3D12_BUFFER_SRV {
                FirstElement: 0,
                NumElements: transformed_glyph_inverse_data.len() as u32,
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
        device.CreateShaderResourceView(
            &sprite_buffer,
            Some(&sprite_desc),
            D3D12_CPU_DESCRIPTOR_HANDLE {
                ptr: heap_start.ptr + (descriptor_size * 2),
            },
        );
        device.CreateShaderResourceView(
            &transformed_glyph_inverse_buffer,
            Some(&transformed_glyph_inverse_desc),
            D3D12_CPU_DESCRIPTOR_HANDLE {
                ptr: heap_start.ptr + (descriptor_size * 3),
            },
        );
    }

    Ok((
        srv_heap,
        curve_buffer,
        band_buffer,
        transformed_glyph_inverse_buffer,
        sprite_buffer,
        sprite_atlas,
    ))
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
        local_bounds: [0.0; 4],
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
        local_bounds: [0.0; 4],
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
        local_bounds: [0.0; 4],
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
        local_bounds: [0.0; 4],
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

fn append_transformed_text_quad(
    vertices: &mut Vec<Vertex>,
    corners: [[f32; 2]; 4],
    corner_w: [f32; 4],
    local_bounds: [f32; 4],
    color: [f32; 4],
    glyph: SlugGlyph,
    debug_id: f32,
) {
    if vertices.len() + 6 > MAX_VERTEX_COUNT {
        return;
    }

    let glyph_data = [
        glyph.curve_start as f32,
        glyph.curve_count as f32,
        glyph.band_count_x.saturating_sub(1) as f32,
        glyph.band_count_y.saturating_sub(1) as f32,
    ];
    let banding = glyph.band_transform;
    let jacobian = [glyph.x_min, glyph.x_max, glyph.y_max, glyph.y_min];
    let effect = PanelEffect::Text as u32 as f32;
    let glyph_index = glyph.band_start as f32;

    let top_left = Vertex {
        position: [corners[0][0], corners[0][1], corner_w[0]],
        color,
        uv: [glyph.x_min, glyph.y_max],
        effect,
        glyph: glyph_index,
        glyph_data,
        banding,
        normal: [-1.0, 1.0],
        jacobian,
        local_bounds,
        _padding: [debug_id, 1.0],
    };
    let top_right = Vertex {
        position: [corners[1][0], corners[1][1], corner_w[1]],
        color,
        uv: [glyph.x_max, glyph.y_max],
        effect,
        glyph: glyph_index,
        glyph_data,
        banding,
        normal: [1.0, 1.0],
        jacobian,
        local_bounds,
        _padding: [debug_id, 1.0],
    };
    let bottom_right = Vertex {
        position: [corners[2][0], corners[2][1], corner_w[2]],
        color,
        uv: [glyph.x_max, glyph.y_min],
        effect,
        glyph: glyph_index,
        glyph_data,
        banding,
        normal: [1.0, -1.0],
        jacobian,
        local_bounds,
        _padding: [debug_id, 1.0],
    };
    let bottom_left = Vertex {
        position: [corners[3][0], corners[3][1], corner_w[3]],
        color,
        uv: [glyph.x_min, glyph.y_min],
        effect,
        glyph: glyph_index,
        glyph_data,
        banding,
        normal: [-1.0, -1.0],
        jacobian,
        local_bounds,
        _padding: [debug_id, 1.0],
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

fn append_transformed_panel_quad(
    vertices: &mut Vec<Vertex>,
    corners: [[f32; 2]; 4],
    color: [f32; 4],
    effect: PanelEffect,
    data: [f32; 4],
) {
    if vertices.len() + 6 > MAX_VERTEX_COUNT {
        return;
    }

    let [
        top_left_corner,
        top_right_corner,
        bottom_right_corner,
        bottom_left_corner,
    ] = corners;
    let effect = effect as u32 as f32;
    let top_left = Vertex {
        position: [top_left_corner[0], top_left_corner[1], 0.0],
        color,
        uv: [0.0, 0.0],
        effect,
        glyph: 0.0,
        glyph_data: data,
        banding: [0.0; 4],
        normal: [0.0; 2],
        jacobian: [0.0; 4],
        local_bounds: [0.0; 4],
        _padding: [0.0; 2],
    };
    let top_right = Vertex {
        position: [top_right_corner[0], top_right_corner[1], 0.0],
        color,
        uv: [1.0, 0.0],
        effect,
        glyph: 0.0,
        glyph_data: data,
        banding: [0.0; 4],
        normal: [0.0; 2],
        jacobian: [0.0; 4],
        local_bounds: [0.0; 4],
        _padding: [0.0; 2],
    };
    let bottom_right = Vertex {
        position: [bottom_right_corner[0], bottom_right_corner[1], 0.0],
        color,
        uv: [1.0, 1.0],
        effect,
        glyph: 0.0,
        glyph_data: data,
        banding: [0.0; 4],
        normal: [0.0; 2],
        jacobian: [0.0; 4],
        local_bounds: [0.0; 4],
        _padding: [0.0; 2],
    };
    let bottom_left = Vertex {
        position: [bottom_left_corner[0], bottom_left_corner[1], 0.0],
        color,
        uv: [0.0, 1.0],
        effect,
        glyph: 0.0,
        glyph_data: data,
        banding: [0.0; 4],
        normal: [0.0; 2],
        jacobian: [0.0; 4],
        local_bounds: [0.0; 4],
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

#[cfg(test)]
fn append_rect(
    vertices: &mut Vec<Vertex>,
    rect: RECT,
    color: [f32; 4],
    effect: u32,
    glyph_index: u32,
) {
    append_rect_with_data(
        vertices,
        rect,
        color,
        effect,
        glyph_index,
        [0.0, 0.0, 1.0, 1.0],
        [0.0; 4],
    );
}

fn append_rect_with_data(
    vertices: &mut Vec<Vertex>,
    rect: RECT,
    color: [f32; 4],
    effect: u32,
    glyph_index: u32,
    uv_rect: [f32; 4],
    data: [f32; 4],
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
    let [uv_left, uv_top, uv_right, uv_bottom] = uv_rect;

    let top_left = Vertex {
        position: [left, top, 0.0],
        color,
        uv: [uv_left, uv_top],
        effect,
        glyph,
        glyph_data: data,
        banding: [0.0; 4],
        normal: [0.0; 2],
        jacobian: [0.0; 4],
        local_bounds: [0.0; 4],
        _padding: [0.0; 2],
    };
    let top_right = Vertex {
        position: [right, top, 0.0],
        color,
        uv: [uv_right, uv_top],
        effect,
        glyph,
        glyph_data: data,
        banding: [0.0; 4],
        normal: [0.0; 2],
        jacobian: [0.0; 4],
        local_bounds: [0.0; 4],
        _padding: [0.0; 2],
    };
    let bottom_right = Vertex {
        position: [right, bottom, 0.0],
        color,
        uv: [uv_right, uv_bottom],
        effect,
        glyph,
        glyph_data: data,
        banding: [0.0; 4],
        normal: [0.0; 2],
        jacobian: [0.0; 4],
        local_bounds: [0.0; 4],
        _padding: [0.0; 2],
    };
    let bottom_left = Vertex {
        position: [left, bottom, 0.0],
        color,
        uv: [uv_left, uv_bottom],
        effect,
        glyph,
        glyph_data: data,
        banding: [0.0; 4],
        normal: [0.0; 2],
        jacobian: [0.0; 4],
        local_bounds: [0.0; 4],
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
#[expect(
    clippy::single_range_in_vec_init,
    reason = "the tests intentionally compare slice shapes built from range literals"
)]
#[expect(
    clippy::struct_field_names,
    reason = "test-only helper structs count segment categories with explicit names"
)]
mod tests {
    use super::{
        CachedSceneVertices, FALLBACK_GLYPH, PanelEffect, RenderScene, Vertex,
        WindowChromeButtonsState, append_rect, append_slug_band_data, build_panel_scene,
        build_shader_params, can_reuse_cached_scene_vertices, collect_scene_chars,
        compile_embedded_shaders, composition_swap_chain_description, cpu_slug_coverage,
        cpu_slug_coverage_all_curves, dirty_fragment_ranges, extract_glyph_curves,
        fragment_ranges_match, fragment_vertex_ranges, load_snapshot_glyph, load_terminal_font,
        preferred_title_bar_color, push_centered_text, push_glyph, push_overlay_panel, push_panel,
        push_text_block, push_title_text, render_frame_model_offscreen_image,
        render_snapshot_glyph_into_image, shader_compile_flags, solve_inverse_homography,
        terminal_scrollbar_geometry, window_garden_shader_data,
    };
    use crate::app::render_verification::{
        build_driver_crash_repro_transformed_text_frame, build_reference_transformed_text_frame,
        build_reference_zero_angle_transformed_text_frame,
        build_reference_zero_angle_transformed_text_layout,
    };
    use crate::app::spatial::ClientRect;
    use crate::app::windows_terminal::TerminalDisplayScrollbar;
    use crate::app::windows_terminal::TerminalLayout;
    use eyre::WrapErr;
    use image::RgbaImage;
    use ttf_parser::{Face, OutlineBuilder};
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Dxgi::Common::DXGI_ALPHA_MODE_PREMULTIPLIED;
    use windows::Win32::Graphics::Dxgi::{
        DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
    };

    #[test]
    fn push_text_block_emits_visible_glyphs() {
        let mut scene = RenderScene {
            panels: Vec::new(),
            glyphs: Vec::new(),
            transformed_glyphs: Vec::new(),
            transformed_glyph_clip_rect: None,
            transformed_text_plane_basis: None,
            sprites: Vec::new(),
            overlay_panels: Vec::new(),
            overlay_transformed_panels: Vec::new(),
            overlay_glyphs: Vec::new(),
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
    fn composition_swap_chain_uses_premultiplied_alpha() {
        let description = composition_swap_chain_description(1280, 720);

        assert_eq!(description.AlphaMode, DXGI_ALPHA_MODE_PREMULTIPLIED);
        assert_eq!(description.SwapEffect, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL);
        assert_eq!(
            description.Flags,
            DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0 as u32,
        );
    }

    #[test]
    fn embedded_hlsl_compiles_vertex_and_pixel_entry_points() -> eyre::Result<()> {
        let shaders = compile_embedded_shaders(shader_compile_flags())?;

        assert!(
            !shaders.vertex.is_empty(),
            "vertex shader bytecode should not be empty"
        );
        assert!(
            !shaders.pixel.is_empty(),
            "pixel shader bytecode should not be empty"
        );

        Ok(())
    }

    #[test]
    fn inverse_homography_maps_projected_quad_back_to_local_space() {
        let projected = [
            [144.0, 92.0],
            [222.0, 108.0],
            [210.0, 178.0],
            [128.0, 166.0],
        ];
        let local = [[-40.0, -16.0], [24.0, -16.0], [24.0, 40.0], [-40.0, 40.0]];

        let coefficients =
            solve_inverse_homography(projected, local).expect("homography should solve");

        for (screen_point, local_point) in projected.into_iter().zip(local) {
            let denominator =
                (coefficients[6] * screen_point[0]) + (coefficients[7] * screen_point[1]) + 1.0;
            let mapped_x = ((coefficients[0] * screen_point[0])
                + (coefficients[1] * screen_point[1])
                + coefficients[2])
                / denominator;
            let mapped_y = ((coefficients[3] * screen_point[0])
                + (coefficients[4] * screen_point[1])
                + coefficients[5])
                / denominator;

            assert!((mapped_x - local_point[0]).abs() < 0.01);
            assert!((mapped_y - local_point[1]).abs() < 0.01);
        }
    }

    #[test]
    fn zero_angle_transformed_text_matches_cpu_glyph_reference() -> eyre::Result<()> {
        let reference_layout = build_reference_zero_angle_transformed_text_layout();
        let reference = render_reference_text_layout_cpu(&reference_layout)?;
        let transformed = render_frame_model_offscreen_image(
            &build_reference_zero_angle_transformed_text_frame(),
        )?;

        assert_eq!(reference.dimensions(), transformed.dimensions());

        let mut mismatch_count = 0_usize;
        let mut first_mismatch = None;
        for y in 0..reference.height() {
            for x in 0..reference.width() {
                let left = reference.get_pixel(x, y);
                let right = transformed.get_pixel(x, y);
                if rgba_matches_with_tolerance(*left, *right, 1) {
                    continue;
                }
                mismatch_count += 1;
                first_mismatch.get_or_insert((x, y, *left, *right));
            }
        }

        let mismatch_summary = first_mismatch.map_or_else(
            || "no mismatches".to_owned(),
            |(x, y, left, right)| {
                format!(
                    "first mismatch at ({x}, {y}): plain={:?} transformed={:?}",
                    left.0, right.0
                )
            },
        );

        assert_eq!(
            mismatch_count, 0,
            "zero-angle transformed text should exactly match the CPU glyph reference; mismatched pixels={mismatch_count}; {mismatch_summary}"
        );

        Ok(())
    }

    #[test]
    fn rotated_transformed_text_matches_cpu_glyph_reference() -> eyre::Result<()> {
        let reference_layout =
            crate::app::render_verification::build_reference_transformed_text_layout();
        let reference = render_reference_text_layout_cpu(&reference_layout)?;
        let transformed =
            render_frame_model_offscreen_image(&build_reference_transformed_text_frame())?;

        assert_eq!(reference.dimensions(), transformed.dimensions());

        let mut mismatch_count = 0_usize;
        let mut first_mismatch = None;
        for y in 0..reference.height() {
            for x in 0..reference.width() {
                let left = reference.get_pixel(x, y);
                let right = transformed.get_pixel(x, y);
                if rgba_matches_with_tolerance(*left, *right, 1) {
                    continue;
                }
                mismatch_count += 1;
                first_mismatch.get_or_insert((x, y, *left, *right));
            }
        }

        let mismatch_summary = first_mismatch.map_or_else(
            || "no mismatches".to_owned(),
            |(x, y, left, right)| {
                format!(
                    "first mismatch at ({x}, {y}): plain={:?} transformed={:?}",
                    left.0, right.0
                )
            },
        );

        assert_eq!(
            mismatch_count, 0,
            "rotated transformed text should match the CPU glyph reference; mismatched pixels={mismatch_count}; {mismatch_summary}"
        );

        Ok(())
    }

    #[test]
    fn strong_yaw_zoomed_transformed_text_matches_cpu_glyph_reference() -> eyre::Result<()> {
        const STRONG_YAW_TOLERANCE: u8 = 2;
        const STRONG_YAW_MAX_MISMATCH_PIXELS: usize = 16;

        let reference_layout =
            crate::app::render_verification::build_reference_text_layout_with_config(
                TerminalLayout {
                    client_width: 1040,
                    client_height: 680,
                    cell_width: 8,
                    cell_height: 16,
                    diagnostic_panel_visible: false,
                },
                "Lorem ipsum dolor sit amet, consectetur adipiscing elit.\nSed do eiusmod tempor incididunt ut labore et dolore magna aliqua.",
                320.0,
                -1.1,
                0.0,
            );
        let reference = render_reference_text_layout_cpu(&reference_layout)?;
        let transformed = render_frame_model_offscreen_image(&reference_layout.frame())?;

        assert_eq!(reference.dimensions(), transformed.dimensions());

        let mut mismatch_count = 0_usize;
        let mut first_mismatch = None;
        for y in 0..reference.height() {
            for x in 0..reference.width() {
                let left = reference.get_pixel(x, y);
                let right = transformed.get_pixel(x, y);
                if rgba_matches_with_tolerance(*left, *right, STRONG_YAW_TOLERANCE) {
                    continue;
                }
                mismatch_count += 1;
                first_mismatch.get_or_insert((x, y, *left, *right));
            }
        }

        let mismatch_summary = first_mismatch.map_or_else(
            || "no mismatches".to_owned(),
            |(x, y, left, right)| {
                format!(
                    "first mismatch at ({x}, {y}): plain={:?} transformed={:?}",
                    left.0, right.0
                )
            },
        );

        assert!(
            mismatch_count <= STRONG_YAW_MAX_MISMATCH_PIXELS,
            "strong-yaw zoomed transformed text should stay close to the CPU glyph reference; mismatched pixels={mismatch_count}; {mismatch_summary}"
        );

        Ok(())
    }

    #[test]
    fn near_identity_transformed_text_offscreen_render_completes() -> eyre::Result<()> {
        let image =
            render_frame_model_offscreen_image(&build_driver_crash_repro_transformed_text_frame())?;

        let visible_pixel_count = image.pixels().filter(|pixel| pixel.0[3] > 0).count();
        assert!(
            visible_pixel_count > 0,
            "near-identity transformed text repro should still render visible pixels"
        );

        Ok(())
    }

    #[test]
    fn rotated_transformed_text_offscreen_render_completes() -> eyre::Result<()> {
        let image = render_frame_model_offscreen_image(&build_reference_transformed_text_frame())?;

        let visible_pixel_count = image.pixels().filter(|pixel| pixel.0[3] > 0).count();
        assert!(
            visible_pixel_count > 0,
            "rotated transformed text fixture should still render visible pixels"
        );

        Ok(())
    }

    #[test]
    fn large_lorem_strong_yaw_transformed_text_offscreen_render_completes() -> eyre::Result<()> {
        let reference_layout = crate::app::render_verification::build_reference_text_layout_with_config(
            TerminalLayout {
                client_width: 1040,
                client_height: 680,
                cell_width: 8,
                cell_height: 16,
                diagnostic_panel_visible: false,
            },
            concat!(
                "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Quis ipsum suspendisse ultrices gravida dictum fusce ut placerat orci nulla pellentesque dignissim enim sit amet venenatis urna cursus eget nunc scelerisque viverra mauris in aliquam sem fringilla ut morbi tincidunt augue interdum velit.\n\n",
                "Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Nunc pulvinar sapien et ligula ullamcorper malesuada proin libero nunc consequat interdum varius sit amet mattis vulputate enim nulla aliquet porttitor lacus luctus accumsan tortor posuere ac ut consequat semper viverra nam libero justo laoreet sit amet.\n\n",
                "Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Integer feugiat scelerisque varius morbi enim nunc faucibus a pellentesque sit amet porttitor eget dolor morbi non arcu risus quis varius quam quisque id diam vel quam elementum pulvinar etiam non quam lacus suspendisse faucibus interdum posuere lorem.\n\n",
                "Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. Mauris ultrices eros in cursus turpis massa tincidunt dui ut ornare lectus sit amet est placerat in egestas erat imperdiet sed euismod nisi porta lorem mollis aliquam ut porttitor leo a diam sollicitudin tempor id eu nisl nunc mi ipsum faucibus vitae aliquet nec ullamcorper.\n\n",
                "Habitant morbi tristique senectus et netus et malesuada fames ac turpis egestas sed tempus urna et pharetra pharetra massa massa ultricies mi quis hendrerit dolor magna eget est lorem ipsum dolor sit amet consectetur adipiscing elit duis tristique sollicitudin nibh sit amet commodo nulla facilisi nullam vehicula ipsum a arcu cursus vitae congue mauris.\n\n",
                "Amet justo donec enim diam vulputate ut pharetra sit amet aliquam id diam maecenas ultricies mi eget mauris pharetra et ultrices neque ornare aenean euismod elementum nisi quis eleifend quam adipiscing vitae proin sagittis nisl rhoncus mattis rhoncus urna neque viverra justo nec ultrices dui sapien eget mi proin sed libero enim sed faucibus turpis in eu mi bibendum neque."
            ),
            320.0,
            -1.1,
            0.0,
        );
        let image = render_frame_model_offscreen_image(&reference_layout.frame())?;

        let visible_pixel_count = image.pixels().filter(|pixel| pixel.0[3] > 0).count();
        assert!(
            visible_pixel_count > 0,
            "large strong-yaw transformed text fixture should still render visible pixels"
        );

        Ok(())
    }

    fn rgba_matches_with_tolerance(
        left: image::Rgba<u8>,
        right: image::Rgba<u8>,
        tolerance: u8,
    ) -> bool {
        left.0
            .into_iter()
            .zip(right.0)
            .all(|(lhs, rhs)| lhs.abs_diff(rhs) <= tolerance)
    }

    fn render_reference_text_layout_cpu(
        reference_layout: &crate::app::render_verification::ReferenceTextLayout,
    ) -> eyre::Result<RgbaImage> {
        let mut image = RgbaImage::new(
            reference_layout.layout.client_width as u32,
            reference_layout.layout.client_height as u32,
        );
        for pixel in image.pixels_mut() {
            *pixel = image::Rgba([0, 0, 0, 255]);
        }

        let font = load_terminal_font()?;
        let face = Face::parse(&font.font_bytes, font.face_index)?;
        for glyph_quad in &reference_layout.transformed_scene.transformed_glyphs {
            let (curves, band_data, glyph) =
                load_snapshot_glyph(&font, &face, glyph_quad.character)?;
            let local_points = [
                [glyph_quad.local_bounds[0], glyph_quad.local_bounds[2]],
                [glyph_quad.local_bounds[1], glyph_quad.local_bounds[2]],
                [glyph_quad.local_bounds[1], glyph_quad.local_bounds[3]],
                [glyph_quad.local_bounds[0], glyph_quad.local_bounds[3]],
            ];
            let inverse = solve_inverse_homography(glyph_quad.corners, local_points)
                .expect("reference transformed glyph should produce an inverse homography");
            let min_x = glyph_quad
                .corners
                .iter()
                .map(|corner| corner[0])
                .fold(f32::INFINITY, f32::min)
                .floor()
                .max(0.0) as u32;
            let max_x = glyph_quad
                .corners
                .iter()
                .map(|corner| corner[0])
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil()
                .min(image.width() as f32) as u32;
            let min_y = glyph_quad
                .corners
                .iter()
                .map(|corner| corner[1])
                .fold(f32::INFINITY, f32::min)
                .floor()
                .max(0.0) as u32;
            let max_y = glyph_quad
                .corners
                .iter()
                .map(|corner| corner[1])
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil()
                .min(image.height() as f32) as u32;

            for y in min_y..max_y {
                for x in min_x..max_x {
                    let sample = [x as f32 + 0.5, y as f32 + 0.5];
                    let local = apply_inverse_homography_point(inverse, sample);
                    if local[0] < glyph_quad.local_bounds[0]
                        || local[0] > glyph_quad.local_bounds[1]
                        || local[1] < glyph_quad.local_bounds[2]
                        || local[1] > glyph_quad.local_bounds[3]
                    {
                        continue;
                    }

                    let render_coord = [
                        remap_range(
                            local[0],
                            glyph_quad.local_bounds[0],
                            glyph_quad.local_bounds[1],
                            glyph.x_min,
                            glyph.x_max,
                        ),
                        remap_range(
                            local[1],
                            glyph_quad.local_bounds[2],
                            glyph_quad.local_bounds[3],
                            glyph.y_max,
                            glyph.y_min,
                        ),
                    ];
                    let sample_step_x =
                        apply_inverse_homography_point(inverse, [sample[0] + 1.0, sample[1]]);
                    let sample_step_y =
                        apply_inverse_homography_point(inverse, [sample[0], sample[1] + 1.0]);
                    let render_coord_step_x = [
                        remap_range(
                            sample_step_x[0],
                            glyph_quad.local_bounds[0],
                            glyph_quad.local_bounds[1],
                            glyph.x_min,
                            glyph.x_max,
                        ),
                        remap_range(
                            sample_step_x[1],
                            glyph_quad.local_bounds[2],
                            glyph_quad.local_bounds[3],
                            glyph.y_max,
                            glyph.y_min,
                        ),
                    ];
                    let render_coord_step_y = [
                        remap_range(
                            sample_step_y[0],
                            glyph_quad.local_bounds[0],
                            glyph_quad.local_bounds[1],
                            glyph.x_min,
                            glyph.x_max,
                        ),
                        remap_range(
                            sample_step_y[1],
                            glyph_quad.local_bounds[2],
                            glyph_quad.local_bounds[3],
                            glyph.y_max,
                            glyph.y_min,
                        ),
                    ];
                    let fwidth = [
                        (render_coord_step_x[0] - render_coord[0]).abs()
                            + (render_coord_step_y[0] - render_coord[0]).abs(),
                        (render_coord_step_x[1] - render_coord[1]).abs()
                            + (render_coord_step_y[1] - render_coord[1]).abs(),
                    ];
                    let pixels_per_em = [
                        1.0 / fwidth[0].max(1.0 / 65536.0),
                        1.0 / fwidth[1].max(1.0 / 65536.0),
                    ];
                    let coverage = cpu_slug_coverage_anisotropic(
                        render_coord,
                        pixels_per_em,
                        &curves,
                        &band_data,
                        glyph,
                    );
                    if coverage <= 0.0 {
                        continue;
                    }
                    let value = (coverage * 255.0).round().clamp(0.0, 255.0) as u8;
                    image.put_pixel(x, y, image::Rgba([value, value, value, 255]));
                }
            }
        }

        Ok(image)
    }

    fn apply_inverse_homography_point(transform: [f32; 8], screen: [f32; 2]) -> [f32; 2] {
        let denominator = (transform[6] * screen[0]) + (transform[7] * screen[1]) + 1.0;
        [
            ((transform[0] * screen[0]) + (transform[1] * screen[1]) + transform[2]) / denominator,
            ((transform[3] * screen[0]) + (transform[4] * screen[1]) + transform[5]) / denominator,
        ]
    }

    fn remap_range(
        value: f32,
        source_min: f32,
        source_max: f32,
        target_min: f32,
        target_max: f32,
    ) -> f32 {
        let source_span = (source_max - source_min).max(1.0 / 65536.0);
        let t = (value - source_min) / source_span;
        target_min + ((target_max - target_min) * t)
    }

    fn cpu_slug_coverage_anisotropic_single_sample(
        render_coord: [f32; 2],
        pixels_per_em: [f32; 2],
        curves: &[super::QuadraticCurve],
        band_data: &[u32],
        glyph: super::SlugGlyph,
    ) -> f32 {
        if glyph.curve_count == 0 {
            return 0.0;
        }

        let mut xcov: f32 = 0.0;
        let mut ycov: f32 = 0.0;
        let mut xwgt: f32 = 0.0;
        let mut ywgt: f32 = 0.0;
        let horizontal_band = super::band_index(
            render_coord[1],
            glyph.band_transform[1],
            glyph.band_transform[3],
            glyph.band_count_y,
        ) as usize;
        let horizontal_header = super::load_directional_band_header(
            band_data,
            super::horizontal_band_header_index(glyph, horizontal_band),
        );
        let horizontal_direction = if render_coord[0] < horizontal_header.split {
            super::CoverageRayDirection::Left
        } else {
            super::CoverageRayDirection::Right
        };
        let horizontal_start = match horizontal_direction {
            super::CoverageRayDirection::Left => horizontal_header.ascending_start as usize,
            super::CoverageRayDirection::Right => horizontal_header.descending_start as usize,
        };
        for offset in 0..horizontal_header.count as usize {
            let curve_index = band_data
                .get(horizontal_start + offset)
                .copied()
                .unwrap_or_default() as usize;
            let Some(curve) = curves.get(curve_index) else {
                continue;
            };
            let extents = super::curve_extents(*curve);
            match horizontal_direction {
                super::CoverageRayDirection::Left => {
                    if (extents.min_x - render_coord[0]) * pixels_per_em[0] > 0.5 {
                        break;
                    }
                }
                super::CoverageRayDirection::Right => {
                    if (extents.max_x - render_coord[0]) * pixels_per_em[0] < -0.5 {
                        break;
                    }
                }
            }
            super::accumulate_horizontal_curve_coverage(
                curve,
                render_coord,
                pixels_per_em[0],
                horizontal_direction,
                &mut xcov,
                &mut xwgt,
            );
        }

        let vertical_band = super::band_index(
            render_coord[0],
            glyph.band_transform[0],
            glyph.band_transform[2],
            glyph.band_count_x,
        ) as usize;
        let vertical_header = super::load_directional_band_header(
            band_data,
            super::vertical_band_header_index(glyph, vertical_band),
        );
        let vertical_direction = if render_coord[1] < vertical_header.split {
            super::CoverageRayDirection::Left
        } else {
            super::CoverageRayDirection::Right
        };
        let vertical_start = match vertical_direction {
            super::CoverageRayDirection::Left => vertical_header.ascending_start as usize,
            super::CoverageRayDirection::Right => vertical_header.descending_start as usize,
        };
        for offset in 0..vertical_header.count as usize {
            let curve_index = band_data
                .get(vertical_start + offset)
                .copied()
                .unwrap_or_default() as usize;
            let Some(curve) = curves.get(curve_index) else {
                continue;
            };
            let extents = super::curve_extents(*curve);
            match vertical_direction {
                super::CoverageRayDirection::Left => {
                    if (extents.min_y - render_coord[1]) * pixels_per_em[1] > 0.5 {
                        break;
                    }
                }
                super::CoverageRayDirection::Right => {
                    if (extents.max_y - render_coord[1]) * pixels_per_em[1] < -0.5 {
                        break;
                    }
                }
            }
            super::accumulate_vertical_curve_coverage(
                curve,
                render_coord,
                pixels_per_em[1],
                vertical_direction,
                &mut ycov,
                &mut ywgt,
            );
        }

        super::calc_coverage(xcov, ycov, xwgt, ywgt)
    }

    fn cpu_slug_coverage_anisotropic(
        render_coord: [f32; 2],
        pixels_per_em: [f32; 2],
        curves: &[super::QuadraticCurve],
        band_data: &[u32],
        glyph: super::SlugGlyph,
    ) -> f32 {
        let sample_step = [
            0.25 / pixels_per_em[0].max(1.0 / 65536.0),
            0.25 / pixels_per_em[1].max(1.0 / 65536.0),
        ];
        let sample_offsets = [
            [-sample_step[0], -sample_step[1]],
            [sample_step[0], -sample_step[1]],
            [-sample_step[0], sample_step[1]],
            [sample_step[0], sample_step[1]],
        ];
        let mut coverage = 0.0;
        for offset in sample_offsets {
            coverage += cpu_slug_coverage_anisotropic_single_sample(
                [render_coord[0] + offset[0], render_coord[1] + offset[1]],
                pixels_per_em,
                curves,
                band_data,
                glyph,
            );
        }
        coverage * 0.25
    }

    #[test]
    fn push_centered_text_places_a_glyph() {
        let mut scene = RenderScene {
            panels: Vec::new(),
            glyphs: Vec::new(),
            transformed_glyphs: Vec::new(),
            transformed_glyph_clip_rect: None,
            transformed_text_plane_basis: None,
            sprites: Vec::new(),
            overlay_panels: Vec::new(),
            overlay_transformed_panels: Vec::new(),
            overlay_glyphs: Vec::new(),
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
    fn push_title_text_uses_most_of_title_bar_height() {
        let mut scene = RenderScene {
            panels: Vec::new(),
            glyphs: Vec::new(),
            transformed_glyphs: Vec::new(),
            transformed_glyph_clip_rect: None,
            transformed_text_plane_basis: None,
            sprites: Vec::new(),
            overlay_panels: Vec::new(),
            overlay_transformed_panels: Vec::new(),
            overlay_glyphs: Vec::new(),
        };
        push_title_text(
            &mut scene,
            RECT {
                left: 0,
                top: 0,
                right: 360,
                bottom: 52,
            },
            "self-test",
            [1.0, 1.0, 1.0, 1.0],
        );

        let glyph_top = scene
            .glyphs
            .iter()
            .map(|glyph| glyph.rect.top)
            .min()
            .expect("title text should emit glyphs");
        let glyph_bottom = scene
            .glyphs
            .iter()
            .map(|glyph| glyph.rect.bottom)
            .max()
            .expect("title text should emit glyphs");

        assert!(
            glyph_bottom - glyph_top >= 32,
            "title text should use most of the 52px title bar instead of looking vertically compressed"
        );
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
    fn cached_scene_vertices_are_not_reused_when_callers_disable_reuse() {
        let cached_vertices = CachedSceneVertices {
            glyph_cache_generation: 4,
            vertices: Vec::new(),
        };

        assert!(!can_reuse_cached_scene_vertices(
            false,
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
            local_bounds: [0.0; 4],
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
            local_bounds: [0.0; 4],
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
            diagnostic_panel_visible: true,
        };

        let scene = build_panel_scene(layout, WindowChromeButtonsState::default());
        let terminal_panel_count = scene
            .panels
            .iter()
            .filter(|panel| matches!(panel.effect, PanelEffect::TerminalPanel))
            .count();

        assert_eq!(terminal_panel_count, 1);
    }

    // behavior[verify window.appearance.backgrounds.system-accent-half-transparent]
    // os[verify os.windows.rendering.direct3d12]
    #[test]
    fn build_panel_scene_keeps_blue_background_half_transparent() {
        let layout = TerminalLayout {
            client_width: 1040,
            client_height: 680,
            cell_width: 8,
            cell_height: 16,
            diagnostic_panel_visible: true,
        };

        let scene = build_panel_scene(layout, WindowChromeButtonsState::default());
        let blue_panel = scene
            .panels
            .iter()
            .find(|panel| matches!(panel.effect, PanelEffect::BlueBackground))
            .expect("blue background panel should exist");

        assert_eq!(blue_panel.color[3], 0.5);
    }

    #[test]
    fn build_panel_scene_limits_blue_background_to_content_frame() {
        let layout = TerminalLayout {
            client_width: 1040,
            client_height: 680,
            cell_width: 8,
            cell_height: 16,
            diagnostic_panel_visible: true,
        };

        let scene = build_panel_scene(layout, WindowChromeButtonsState::default());
        let blue_panel = scene
            .panels
            .iter()
            .find(|panel| matches!(panel.effect, PanelEffect::BlueBackground))
            .expect("blue background panel should exist");

        assert_eq!(blue_panel.rect, layout.content_frame_rect().to_win32_rect());
    }

    // windowing[verify garden-band.outward]
    #[test]
    fn build_panel_scene_emits_a_dedicated_garden_frame_surface() {
        let layout = TerminalLayout {
            client_width: 1040,
            client_height: 680,
            cell_width: 8,
            cell_height: 16,
            diagnostic_panel_visible: true,
        };

        let scene = build_panel_scene(layout, WindowChromeButtonsState::default());
        let garden_index = scene
            .panels
            .iter()
            .position(|panel| matches!(panel.effect, PanelEffect::GardenFrame))
            .expect("garden frame panel should exist");
        let title_index = scene
            .panels
            .iter()
            .position(|panel| matches!(panel.effect, PanelEffect::TitleBar))
            .expect("title bar panel should exist");
        let garden_panel = &scene.panels[garden_index];
        let expected_shader_data = window_garden_shader_data(layout);

        assert_eq!(garden_panel.rect, layout.full_client_rect().to_win32_rect());
        assert!(garden_index < title_index);
        for (actual, expected) in garden_panel.data.iter().zip(expected_shader_data.iter()) {
            assert!((actual - expected).abs() < 0.0001);
        }
    }

    // behavior[verify window.appearance.chrome]
    #[test]
    fn build_panel_scene_includes_title_bar_panel() {
        let layout = TerminalLayout {
            client_width: 1040,
            client_height: 680,
            cell_width: 8,
            cell_height: 16,
            diagnostic_panel_visible: true,
        };

        let scene = build_panel_scene(layout, WindowChromeButtonsState::default());
        let title_panel_count = scene
            .panels
            .iter()
            .filter(|panel| matches!(panel.effect, PanelEffect::TitleBar))
            .count();

        assert_eq!(title_panel_count, 1);
    }

    #[test]
    fn build_panel_scene_uses_accent_color_for_focused_title_bar() {
        let layout = TerminalLayout {
            client_width: 1040,
            client_height: 680,
            cell_width: 8,
            cell_height: 16,
            diagnostic_panel_visible: true,
        };

        let scene = build_panel_scene(
            layout,
            WindowChromeButtonsState {
                focused: true,
                ..WindowChromeButtonsState::default()
            },
        );
        let title_panel = scene
            .panels
            .iter()
            .find(|panel| matches!(panel.effect, PanelEffect::TitleBar))
            .expect("title bar panel should exist");

        assert_eq!(title_panel.color, preferred_title_bar_color(true));
    }

    #[test]
    fn build_panel_scene_uses_unfocused_title_bar_color_when_inactive() {
        let layout = TerminalLayout {
            client_width: 1040,
            client_height: 680,
            cell_width: 8,
            cell_height: 16,
            diagnostic_panel_visible: true,
        };

        let scene = build_panel_scene(layout, WindowChromeButtonsState::default());
        let title_panel = scene
            .panels
            .iter()
            .find(|panel| matches!(panel.effect, PanelEffect::TitleBar))
            .expect("title bar panel should exist");

        assert_eq!(title_panel.color, preferred_title_bar_color(false));
    }

    #[test]
    fn build_panel_scene_assigns_shader_chrome_effects_without_glyph_icons() {
        let layout = TerminalLayout {
            client_width: 1040,
            client_height: 680,
            cell_width: 8,
            cell_height: 16,
            diagnostic_panel_visible: true,
        };

        let scene = build_panel_scene(layout, WindowChromeButtonsState::default());
        let pin_button_count = scene
            .panels
            .iter()
            .filter(|panel| matches!(panel.effect, PanelEffect::WindowChromePin))
            .count();
        let latency_button_count = scene
            .panels
            .iter()
            .filter(|panel| matches!(panel.effect, PanelEffect::WindowChromeLatency))
            .count();
        let diagnostics_button_count = scene
            .panels
            .iter()
            .filter(|panel| matches!(panel.effect, PanelEffect::WindowChromeDiagnostics))
            .count();
        let minimize_button_count = scene
            .panels
            .iter()
            .filter(|panel| matches!(panel.effect, PanelEffect::WindowChromeMinimize))
            .count();
        let maximize_button_count = scene
            .panels
            .iter()
            .filter(|panel| matches!(panel.effect, PanelEffect::WindowChromeMaximize))
            .count();
        let restore_button_count = scene
            .panels
            .iter()
            .filter(|panel| matches!(panel.effect, PanelEffect::WindowChromeRestore))
            .count();
        let close_button_count = scene
            .panels
            .iter()
            .filter(|panel| matches!(panel.effect, PanelEffect::WindowChromeClose))
            .count();

        assert_eq!(pin_button_count, 1);
        assert_eq!(latency_button_count, 0);
        assert_eq!(diagnostics_button_count, 1);
        assert_eq!(minimize_button_count, 1);
        assert_eq!(maximize_button_count, 1);
        assert_eq!(restore_button_count, 0);
        assert_eq!(close_button_count, 1);
        assert!(scene.glyphs.is_empty());
    }

    #[test]
    fn build_panel_scene_uses_restore_effect_when_window_is_maximized() {
        let layout = TerminalLayout {
            client_width: 1040,
            client_height: 680,
            cell_width: 8,
            cell_height: 16,
            diagnostic_panel_visible: true,
        };

        let scene = build_panel_scene(
            layout,
            WindowChromeButtonsState {
                maximized: true,
                ..WindowChromeButtonsState::default()
            },
        );
        let maximize_button_count = scene
            .panels
            .iter()
            .filter(|panel| matches!(panel.effect, PanelEffect::WindowChromeMaximize))
            .count();
        let restore_button_count = scene
            .panels
            .iter()
            .filter(|panel| matches!(panel.effect, PanelEffect::WindowChromeRestore))
            .count();

        assert_eq!(maximize_button_count, 0);
        assert_eq!(restore_button_count, 1);
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
            transformed_glyphs: Vec::new(),
            transformed_glyph_clip_rect: None,
            transformed_text_plane_basis: None,
            sprites: Vec::new(),
            overlay_panels: Vec::new(),
            overlay_transformed_panels: Vec::new(),
            overlay_glyphs: Vec::new(),
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
            transformed_glyphs: Vec::new(),
            transformed_glyph_clip_rect: None,
            transformed_text_plane_basis: None,
            sprites: Vec::new(),
            overlay_panels: Vec::new(),
            overlay_transformed_panels: Vec::new(),
            overlay_glyphs: Vec::new(),
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
    fn terminal_scrollbar_geometry_clamps_min_thumb_height_to_track_height() {
        let geometry = terminal_scrollbar_geometry(
            ClientRect::new(0, 0, 16, 21),
            TerminalDisplayScrollbar {
                total: 100,
                offset: 40,
                visible: 24,
            },
        )
        .expect("short scrollbar track should still produce geometry");

        assert_eq!(geometry.thumb_height, 21);
        assert_eq!(geometry.thumb_rect.height(), 21);
        assert_eq!(geometry.travel, 0);
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

        assert_eq!(
            count_connected_components(&image, 8),
            1,
            "slash should render as one connected component"
        );
        Ok(())
    }

    #[test]
    #[ignore = "fontdue comparison is diagnostic-only while render verification uses D3D12 output"]
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
    #[cfg_attr(
        not(feature = "font-snapshot-tests"),
        ignore = "expensive font snapshot artifacts are opt-in; run with --features font-snapshot-tests"
    )]
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
    #[cfg_attr(
        not(feature = "font-snapshot-tests"),
        ignore = "expensive font snapshot artifacts are opt-in; run with --features font-snapshot-tests"
    )]
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
        write_fontdue_reference_png('g', 256, 512, 512, &output_dir.join("g-fontdue-256.png"))?;

        assert!(output_dir.join("slash-fontdue-256.png").exists());
        assert!(output_dir.join("b-fontdue-256.png").exists());
        assert!(output_dir.join("r-fontdue-256.png").exists());
        assert!(output_dir.join("g-fontdue-256.png").exists());
        Ok(())
    }

    #[test]
    #[cfg_attr(
        not(feature = "font-snapshot-tests"),
        ignore = "expensive font snapshot artifacts are opt-in; run with --features font-snapshot-tests"
    )]
    fn fontdue_comparison_diffs_write_debug_artifacts() -> eyre::Result<()> {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let output_dir = manifest_dir
            .join("target")
            .join("test-artifacts")
            .join("slug");
        let font = load_terminal_font()?;
        let face = Face::parse(&font.font_bytes, font.face_index)?;

        for character in ['/', 'b', 'r', 'g'] {
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
        assert!(output_dir.join("g-slug-fontdue-diff.png").exists());
        Ok(())
    }

    #[test]
    #[cfg_attr(
        not(feature = "font-snapshot-tests"),
        ignore = "expensive font snapshot artifacts are opt-in; run with --features font-snapshot-tests"
    )]
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

        for character in ['b', 'r', 'g', '6'] {
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
                    let full =
                        cpu_slug_coverage_all_curves(sample, 16.0, &curves, &band_data, glyph);
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

    fn snapshot_render_coord_for_pixel(
        font: &super::LoadedTerminalFont,
        glyph: super::SlugGlyph,
        font_size_px: u32,
        image_width: u32,
        image_height: u32,
        pixel_x: u32,
        pixel_y: u32,
    ) -> ([f32; 2], f32) {
        let font_height_units = font.units_per_em.max(1.0);
        let scale = font_size_px as f32 / font_height_units;
        let uv_pad_x = super::SLUG_GLYPH_DILATION_PX / scale;
        let uv_pad_y = super::SLUG_GLYPH_DILATION_PX / scale;
        let glyph_width_px =
            (((glyph.x_max - glyph.x_min) + (uv_pad_x * 2.0)).max(1.0) * scale).max(1.0);
        let glyph_height_px =
            (((glyph.y_max - glyph.y_min) + (uv_pad_y * 2.0)).max(1.0) * scale).max(1.0);
        let offset_x = ((image_width as f32 - glyph_width_px) * 0.5).max(0.0);
        let offset_y = (((image_height as f32 - glyph_height_px) * 0.5).max(0.0) - 1.0).max(0.0);
        let render_x_min = glyph.x_min - uv_pad_x;
        let render_y_max = glyph.y_max + uv_pad_y;
        let sample_x = pixel_x as f32 + 0.5;
        let sample_y = pixel_y as f32 + 0.5;
        (
            [
                render_x_min + ((sample_x - offset_x) / scale),
                render_y_max - ((sample_y - offset_y) / scale),
            ],
            scale,
        )
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
                let left = lhs.get_pixel(x, y);
                let right = rhs.get_pixel(x, y);
                let delta = [0, 1, 2, 3]
                    .into_iter()
                    .map(|index| {
                        (i16::from(left[index]) - i16::from(right[index])).unsigned_abs() as u8
                    })
                    .max()
                    .unwrap_or_default();
                image.put_pixel(x, y, image::Rgba([delta, delta, delta, 255]));
            }
        }
        image
    }

    fn opaque_vertical_hole(image: &RgbaImage, min_run: u32) -> Option<(u32, u32, u32)> {
        for x in 1..image.width().saturating_sub(1) {
            let mut run_start = None;
            for y in 0..image.height() {
                let left = image.get_pixel(x - 1, y)[0] >= 250;
                let center = image.get_pixel(x, y)[0] <= 5;
                let right = image.get_pixel(x + 1, y)[0] >= 250;
                if left && center && right {
                    run_start.get_or_insert(y);
                    continue;
                }

                if let Some(start) = run_start.take()
                    && y - start >= min_run
                {
                    return Some((x, start, y - 1));
                }
            }

            if let Some(start) = run_start
                && image.height() - start >= min_run
            {
                return Some((x, start, image.height() - 1));
            }
        }

        None
    }

    #[test]
    fn fontdue_diff_marks_opaque_black_vs_opaque_white_pixels() {
        let mut slug = RgbaImage::new(1, 1);
        slug.put_pixel(0, 0, image::Rgba([0, 0, 0, 255]));

        let mut fontdue = RgbaImage::new(1, 1);
        fontdue.put_pixel(0, 0, image::Rgba([255, 255, 255, 255]));

        let diff = render_alpha_diff(&slug, &fontdue);

        assert!(
            diff.get_pixel(0, 0)[0] > 0,
            "opaque black slug mistakes must show up in the diagnostic diff"
        );
    }

    #[test]
    fn r_snapshot_has_no_opaque_vertical_hole_inside_the_stem() -> eyre::Result<()> {
        let font = load_terminal_font()?;
        let face = Face::parse(&font.font_bytes, font.face_index)?;
        let glyph_id = face
            .glyph_index('r')
            .expect("diagnostic glyph should exist in terminal font");
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

        let image = render_test_glyph(&font, &curves, &band_data, glyph, 256, 512, 512);

        let hole = opaque_vertical_hole(&image, 8);
        assert!(
            hole.is_none(),
            "the r snapshot should not contain an internal opaque black stem hole: {hole:?}"
        );

        Ok(())
    }

    #[test]
    fn r_top_hook_pixels_match_full_curve_walk() -> eyre::Result<()> {
        let font = load_terminal_font()?;
        let face = Face::parse(&font.font_bytes, font.face_index)?;
        let glyph_id = face
            .glyph_index('r')
            .expect("diagnostic glyph should exist in terminal font");
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

        for (pixel_x, pixel_y) in [(239, 220), (240, 220), (241, 220)] {
            let (sample, scale) =
                snapshot_render_coord_for_pixel(&font, glyph, 256, 512, 512, pixel_x, pixel_y);
            let banded = cpu_slug_coverage(sample, scale, &curves, &band_data, glyph);
            let full = cpu_slug_coverage_all_curves(sample, scale, &curves, &band_data, glyph);
            assert!(
                full > 0.99,
                "full curve walk should fill the r top hook pixel ({pixel_x}, {pixel_y}), got {full}"
            );
            assert!(
                (banded - full).abs() <= 0.0001,
                "banded coverage should match full walk at r top hook pixel ({pixel_x}, {pixel_y}): banded={banded} full={full}"
            );
        }

        Ok(())
    }

    #[test]
    fn r_snapshot_keeps_outer_base_pixels_on_the_cut_row() -> eyre::Result<()> {
        let font = load_terminal_font()?;
        let face = Face::parse(&font.font_bytes, font.face_index)?;
        let glyph_id = face
            .glyph_index('r')
            .expect("diagnostic glyph should exist in terminal font");
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
        let image = render_test_glyph(&font, &curves, &band_data, glyph, 256, 512, 512);
        let fontdue = render_fontdue_reference_glyph('r', 256, 512, 512)?;

        for (pixel_x, pixel_y) in [(190, 299), (270, 299)] {
            assert!(
                fontdue.get_pixel(pixel_x, pixel_y)[0] > 0,
                "fontdue reference should cover the r base cut pixel ({pixel_x}, {pixel_y})"
            );
            assert!(
                image.get_pixel(pixel_x, pixel_y)[0] > 0,
                "slug snapshot should keep the outer r base pixel on the cut row ({pixel_x}, {pixel_y})"
            );
        }

        Ok(())
    }

    #[test]
    fn full_terminal_unicode_set_builds_slug_buffers() -> eyre::Result<()> {
        let chars = super::terminal_font_unicode_chars()?;
        let font = load_terminal_font()?;
        let (curve_data, band_data, glyph_cache) = super::build_slug_curve_buffer(&font, &chars)?;

        assert!(
            !chars.is_empty(),
            "expected terminal font to expose unicode glyphs"
        );
        assert!(
            !curve_data.is_empty(),
            "expected curve data for the full terminal unicode glyph set"
        );
        assert!(
            !band_data.is_empty(),
            "expected band data for the full terminal unicode glyph set"
        );
        assert_eq!(
            glyph_cache.len(),
            chars.len(),
            "expected a glyph cache entry for every enumerated terminal glyph"
        );

        Ok(())
    }

    #[test]
    fn r_snapshot_keeps_mid_stem_pixels_on_the_endpoint_row() -> eyre::Result<()> {
        let font = load_terminal_font()?;
        let face = Face::parse(&font.font_bytes, font.face_index)?;
        let glyph_id = face
            .glyph_index('r')
            .expect("diagnostic glyph should exist in terminal font");
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

        let image = render_test_glyph(&font, &curves, &band_data, glyph, 256, 512, 512);
        let fontdue = render_fontdue_reference_glyph('r', 256, 512, 512)?;

        for (pixel_x, pixel_y) in [(220, 265), (230, 265), (240, 265)] {
            assert!(
                fontdue.get_pixel(pixel_x, pixel_y)[0] > 0,
                "fontdue reference should cover the r endpoint-row pixel ({pixel_x}, {pixel_y})"
            );
            assert!(
                image.get_pixel(pixel_x, pixel_y)[0] > 0,
                "slug snapshot should keep the r endpoint-row pixel ({pixel_x}, {pixel_y})"
            );
        }

        Ok(())
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
