mod d3d12_resources;
mod d3d12_smoke;
mod d3d12_sprite_atlas;
mod d3d12_srv;
mod d3d12_text_pipeline;
mod d3d12_text_renderer_host;
mod d3d12_text_renderer_proxy;
mod d3d12_text_renderer_resources;
mod d3d12_upload;
mod scene;
mod scene_cache;
mod scene_packet;
mod scene_upload;
mod scene_upload_batch;
mod scene_vertices;
mod text_atlas;

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use eyre::{Result, WrapErr, eyre};
use linkme::distributed_slice;
use teamy_studio_event_core::{
    EventDefinition, EventDefinitionId, EventLogIntent, PublishedEvent, WritableArena,
};
use teamy_studio_registration_core::{
    EVENT_DEFINITION_REGISTRATIONS, EventDefinitionRegistration, registration_provenance,
};
use teamy_studio_timeline_core::{CanonicalTimeKey, ConstructedTimeline, TriggerRuntime};
use tracing::warn;
use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CS_DBLCLKS, CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect,
    GetSystemMetrics, GetWindowRect, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT,
    HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, IDC_ARROW, KillTimer, LoadCursorW,
    RegisterClassExW, SM_CXSCREEN, SM_CYSCREEN, SW_MAXIMIZE, SW_MINIMIZE, SW_SHOW, SW_SHOWNA,
    SetCursor, SetTimer, SetWindowPos, ShowWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WM_DESTROY,
    WM_DPICHANGED, WM_ERASEBKGND, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCACTIVATE,
    WM_NCCALCSIZE, WM_NCHITTEST, WM_NCPAINT, WM_PAINT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_TIMER,
    WNDCLASSEXW, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW,
    WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_THICKFRAME, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

pub use d3d12_resources::{
    SceneUploadResources, create_scene_upload_buffers, create_scene_vertex_buffer,
    create_shader_param_buffer, curve_buffer_size_bytes, scene_vertex_buffer_size_bytes,
    transformed_glyph_inverse_buffer_size_bytes,
};
pub use d3d12_smoke::create_text_renderer_host_for_scene;
pub use d3d12_srv::{TextShaderResourceSet, create_text_shader_resource_set};
pub use d3d12_text_renderer_host::{TextRendererHost, create_text_renderer_device};
pub use d3d12_text_renderer_proxy::TextRendererThreadProxy;
pub use d3d12_text_renderer_resources::{TextRendererResources, create_text_renderer_resources};
pub use d3d12_upload::{
    upload_band_data, upload_cached_fragment_vertices, upload_curve_data, upload_vertex_ranges,
};
pub use scene::{
    ButtonVisualState, GlyphQuad, PanelEffect, PanelRect, RenderScene, ShellSceneLayout, SpriteId,
    SpriteQuad, WindowChromeButtonsState, preferred_background_color, preferred_title_bar_color,
    push_centered_text, push_panel, push_panel_with_data, push_sprite, push_text_block,
    push_title_text, push_window_chrome_buttons, push_window_garden_frame, scale_for_dpi,
};
pub use scene_cache::{dirty_fragment_ranges, fragment_ranges_match, fragment_vertex_ranges};
pub use scene_packet::{PreparedRenderScene, prepare_render_scene};
pub use scene_upload::{
    ShaderResourceCapacities, ensure_shader_resource_capacities, padded_band_upload_data,
    padded_curve_upload_data, vertex_byte_range,
};
pub use scene_upload_batch::{PreparedSceneUploadBatch, build_prepared_scene_upload_batch};
pub use scene_vertices::{SceneVertex, build_scene_vertices, build_scene_vertices_with_text_atlas};
pub use text_atlas::{
    SceneTextAtlas, build_scene_text_atlas, build_scene_text_atlas_from_fragments,
    collect_scene_characters,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem, GetDpiForWindow,
    SetProcessDpiAwarenessContext,
};

const INITIAL_WINDOW_WIDTH: i32 = 1280;
const INITIAL_WINDOW_HEIGHT: i32 = 820;
const STANDARD_WINDOW_CLASS_NAME: PCWSTR = w!("TeamyStudioShellWindow");
const TOOL_WINDOW_CLASS_NAME: PCWSTR = w!("TeamyStudioShellToolWindow");
const SHELL_RENDER_TIMER_ID: usize = 1;
const SHELL_RENDER_INTERVAL_MS: u32 = 16;

static STANDARD_WINDOW_CLASS_REGISTERED: OnceLock<()> = OnceLock::new();
static TOOL_WINDOW_CLASS_REGISTERED: OnceLock<()> = OnceLock::new();
static DPI_AWARENESS_INITIALIZED: OnceLock<()> = OnceLock::new();
static SHELL_WINDOW_STATES: OnceLock<Mutex<HashMap<isize, NativeShellWindowState>>> =
    OnceLock::new();

thread_local! {
    static SHELL_TEXT_RENDERER_HOSTS: RefCell<HashMap<isize, TextRendererHost>> = RefCell::new(HashMap::new());
}

#[derive(Clone, Copy, Debug)]
pub struct FeatureWindowSceneContext {
    pub title: &'static str,
    pub layout: ShellSceneLayout,
    pub dpi: u32,
    pub chrome: WindowChromeButtonsState,
    pub cursor_client_point: Option<POINT>,
}

#[derive(Clone, Copy)]
pub struct FeatureWindowSceneRegistration {
    pub title: &'static str,
    pub build_scene: fn(FeatureWindowSceneContext) -> RenderScene,
    pub cursor_for_point: Option<fn(FeatureWindowSceneContext, POINT) -> Option<PCWSTR>>,
    pub on_left_click: Option<fn(FeatureWindowSceneContext, POINT) -> bool>,
    pub on_mouse_wheel: Option<fn(FeatureWindowSceneContext, POINT, i16) -> bool>,
    pub on_right_button_down: Option<fn(FeatureWindowSceneContext, POINT) -> bool>,
    pub on_right_drag: Option<fn(FeatureWindowSceneContext, POINT) -> bool>,
    pub on_right_button_up: Option<fn(FeatureWindowSceneContext, POINT) -> bool>,
    pub on_frame_tick: Option<fn(FeatureWindowSceneContext) -> bool>,
}

#[distributed_slice]
pub static FEATURE_WINDOW_SCENE_REGISTRATIONS: [FeatureWindowSceneRegistration];

#[derive(Clone, Copy, Debug)]
struct NativeShellWindowState {
    title: &'static str,
    dpi: u32,
    chrome: WindowChromeButtonsState,
    cursor_client_point: Option<POINT>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LogicalWindowId(u64);

impl LogicalWindowId {
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresentPolicy {
    Composed,
    LowLatencyHwnd,
    LateLatchedPointer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowChromeKind {
    // os[impl window.appearance.os-chrome-none]
    Standard,
    Tool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowVisibility {
    Show,
    ShowNoActivate,
    Hidden,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowActivationPolicy {
    Foreground,
    NoActivate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitialWindowCommand {
    Show,
    ShowNoActivate,
    Hidden,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RendererHostMode {
    Composed,
    LowLatencyHwnd,
    LateLatchedPointer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeWindowStylePlan {
    pub app_window: bool,
    pub tool_window: bool,
    pub no_activate: bool,
    pub no_redirection_bitmap: bool,
    pub popup: bool,
    pub thick_frame: bool,
    pub minimize_box: bool,
    pub maximize_box: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeWindowHostPlan {
    pub style: NativeWindowStylePlan,
    pub initial_command: InitialWindowCommand,
    pub bring_to_front: bool,
}

#[must_use]
pub fn native_window_ex_style(plan: NativeWindowHostPlan) -> WINDOW_EX_STYLE {
    let mut style = WINDOW_EX_STYLE(0);
    if plan.style.app_window {
        style |= WS_EX_APPWINDOW;
    }
    if plan.style.tool_window {
        style |= WS_EX_TOOLWINDOW;
    }
    if plan.style.no_activate {
        style |= WS_EX_NOACTIVATE;
    }
    if plan.style.no_redirection_bitmap {
        style |= WS_EX_NOREDIRECTIONBITMAP;
    }
    style
}

#[must_use]
pub fn native_window_style(plan: NativeWindowHostPlan) -> WINDOW_STYLE {
    let mut style = WINDOW_STYLE(0);
    if !matches!(plan.initial_command, InitialWindowCommand::Hidden) {
        style |= WS_VISIBLE;
    }
    if plan.style.popup {
        style |= WS_POPUP;
    }
    if plan.style.thick_frame {
        style |= WS_THICKFRAME;
    }
    if plan.style.minimize_box {
        style |= WS_MINIMIZEBOX;
    }
    if plan.style.maximize_box {
        style |= WS_MAXIMIZEBOX;
    }
    style
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct NativeWindowHandle(*mut core::ffi::c_void);

impl NativeWindowHandle {
    #[must_use]
    pub fn raw(self) -> isize {
        self.0 as isize
    }

    #[must_use]
    pub const fn is_valid(self) -> bool {
        !self.0.is_null()
    }

    #[must_use]
    const fn as_hwnd(self) -> HWND {
        HWND(self.0)
    }
}

unsafe extern "system" fn shell_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCALCSIZE => LRESULT(0),
        WM_NCACTIVATE => LRESULT(1),
        WM_NCPAINT => LRESULT(0),
        WM_NCHITTEST => shell_non_client_hit_test(hwnd, lparam),
        WM_ERASEBKGND => LRESULT(1),
        WM_DPICHANGED => handle_shell_dpi_changed(hwnd, wparam, lparam),
        WM_MOUSEMOVE => {
            let _ = handle_shell_mouse_move(hwnd, lparam);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == SHELL_RENDER_TIMER_ID => {
            let _ = handle_shell_frame_tick(hwnd);
            let _ = drive_shell_render_path(hwnd);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let _ = handle_shell_left_button_up(hwnd, lparam);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let _ = handle_shell_mouse_wheel(hwnd, wparam, lparam);
            LRESULT(0)
        }
        WM_RBUTTONDOWN => {
            let _ = handle_shell_right_button_down(hwnd, lparam);
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let _ = handle_shell_right_button_up(hwnd, lparam);
            LRESULT(0)
        }
        WM_PAINT => paint_shell_window(hwnd),
        WM_DESTROY => {
            destroy_shell_window_render_state(hwnd);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn shell_window_states() -> &'static Mutex<HashMap<isize, NativeShellWindowState>> {
    SHELL_WINDOW_STATES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn feature_window_scene_registration(
    title: &'static str,
) -> Option<&'static FeatureWindowSceneRegistration> {
    FEATURE_WINDOW_SCENE_REGISTRATIONS
        .iter()
        .find(|registration| registration.title == title)
}

fn shell_window_layout(hwnd: HWND, dpi: u32) -> Result<ShellSceneLayout> {
    let mut client_rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut client_rect) }
        .wrap_err("failed to get shell window client rect")?;
    Ok(ShellSceneLayout::for_main_menu_with_dpi(
        client_rect.right - client_rect.left,
        client_rect.bottom - client_rect.top,
        dpi,
    ))
}

fn rect_contains(rect: RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

#[derive(Clone, Copy, Debug)]
enum ShellChromeClickAction {
    Pin,
    Diagnostics,
    Latency,
    Minimize,
    MaximizeRestore,
    Close,
}

fn shell_chrome_click_action(
    layout: ShellSceneLayout,
    point: POINT,
) -> Option<ShellChromeClickAction> {
    [
        (layout.pin_button_rect, ShellChromeClickAction::Pin),
        (
            layout.diagnostics_button_rect,
            ShellChromeClickAction::Diagnostics,
        ),
        (layout.latency_button_rect, ShellChromeClickAction::Latency),
        (
            layout.minimize_button_rect,
            ShellChromeClickAction::Minimize,
        ),
        (
            layout.maximize_restore_button_rect,
            ShellChromeClickAction::MaximizeRestore,
        ),
        (layout.close_button_rect, ShellChromeClickAction::Close),
    ]
    .into_iter()
    .find_map(|(rect, action)| rect_contains(rect, point.x, point.y).then_some(action))
}

fn button_visual_state_for_rect(
    rect: RECT,
    cursor_client_point: Option<POINT>,
) -> ButtonVisualState {
    let hovered = cursor_client_point.is_some_and(|point| rect_contains(rect, point.x, point.y));
    ButtonVisualState {
        hover_near: if hovered { 1.0 } else { 0.0 },
        hovered,
        active: hovered,
        ..Default::default()
    }
}

fn chrome_state_with_pointer(
    mut chrome: WindowChromeButtonsState,
    layout: ShellSceneLayout,
    cursor_client_point: Option<POINT>,
) -> WindowChromeButtonsState {
    chrome.pin = button_visual_state_for_rect(layout.pin_button_rect, cursor_client_point);
    chrome.diagnostics =
        button_visual_state_for_rect(layout.diagnostics_button_rect, cursor_client_point);
    chrome.latency = button_visual_state_for_rect(layout.latency_button_rect, cursor_client_point);
    chrome.minimize =
        button_visual_state_for_rect(layout.minimize_button_rect, cursor_client_point);
    chrome.maximize_restore =
        button_visual_state_for_rect(layout.maximize_restore_button_rect, cursor_client_point);
    chrome.close = button_visual_state_for_rect(layout.close_button_rect, cursor_client_point);
    chrome
}

fn build_live_shell_window_scene(hwnd: HWND) -> Result<Option<RenderScene>> {
    let state = shell_window_states()
        .lock()
        .expect("shell window state should not be poisoned")
        .get(&(hwnd.0 as isize))
        .copied();
    let Some(state) = state else {
        return Ok(None);
    };
    let Some(registration) = feature_window_scene_registration(state.title) else {
        return Ok(None);
    };
    let layout = shell_window_layout(hwnd, state.dpi)?;
    Ok(Some((registration.build_scene)(
        FeatureWindowSceneContext {
            title: state.title,
            layout,
            dpi: state.dpi,
            chrome: chrome_state_with_pointer(state.chrome, layout, state.cursor_client_point),
            cursor_client_point: state.cursor_client_point,
        },
    )))
}

fn shell_window_scene_context(hwnd: HWND) -> Result<Option<FeatureWindowSceneContext>> {
    let state = shell_window_states()
        .lock()
        .expect("shell window state should not be poisoned")
        .get(&(hwnd.0 as isize))
        .copied();
    let Some(state) = state else {
        return Ok(None);
    };
    let layout = shell_window_layout(hwnd, state.dpi)?;
    Ok(Some(FeatureWindowSceneContext {
        title: state.title,
        layout,
        dpi: state.dpi,
        chrome: chrome_state_with_pointer(state.chrome, layout, state.cursor_client_point),
        cursor_client_point: state.cursor_client_point,
    }))
}

fn update_shell_window_cursor(hwnd: HWND, point: POINT) -> Result<()> {
    let Some(context) = shell_window_scene_context(hwnd)? else {
        return Ok(());
    };
    let Some(registration) = feature_window_scene_registration(context.title) else {
        return Ok(());
    };
    let cursor = registration
        .cursor_for_point
        .and_then(|resolver| resolver(context, point))
        .unwrap_or(IDC_ARROW);
    if let Ok(cursor_handle) = unsafe { LoadCursorW(Some(HINSTANCE::default()), cursor) } {
        unsafe { SetCursor(Some(cursor_handle)) };
    }
    Ok(())
}

fn initialize_shell_window_render_state(hwnd: HWND) -> Result<()> {
    let scene = build_live_shell_window_scene(hwnd)?;
    let Some(scene) = scene else {
        return Ok(());
    };
    SHELL_TEXT_RENDERER_HOSTS.with(|text_renderer_hosts| {
        text_renderer_hosts.borrow_mut().insert(
            hwnd.0 as isize,
            create_text_renderer_host_for_scene(hwnd, &scene)?,
        );
        Ok::<(), eyre::Report>(())
    })?;
    unsafe {
        let _ = SetTimer(
            Some(hwnd),
            SHELL_RENDER_TIMER_ID,
            SHELL_RENDER_INTERVAL_MS,
            None,
        );
    }
    configure_shell_window_chrome(hwnd)?;
    Ok(())
}

fn destroy_shell_window_render_state(hwnd: HWND) {
    unsafe {
        let _ = KillTimer(Some(hwnd), SHELL_RENDER_TIMER_ID);
    }
    shell_window_states()
        .lock()
        .expect("shell window state should not be poisoned")
        .remove(&(hwnd.0 as isize));
    SHELL_TEXT_RENDERER_HOSTS.with(|text_renderer_hosts| {
        text_renderer_hosts.borrow_mut().remove(&(hwnd.0 as isize));
    });
}

fn configure_shell_window_chrome(hwnd: HWND) -> Result<()> {
    let border_color = DWMWA_COLOR_NONE;
    if let Err(error) = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            (&raw const border_color).cast(),
            u32::try_from(std::mem::size_of_val(&border_color)).unwrap_or(u32::MAX),
        )
    } {
        warn!(error = %error, "shell window DWM border-color override unavailable");
    }

    let corner_preference = DWMWCP_DONOTROUND;
    if let Err(error) = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&raw const corner_preference).cast(),
            u32::try_from(std::mem::size_of_val(&corner_preference)).unwrap_or(u32::MAX),
        )
    } {
        warn!(error = %error, "shell window DWM corner override unavailable");
    }

    Ok(())
}

fn paint_shell_window(hwnd: HWND) -> LRESULT {
    let mut paint = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
    if !hdc.0.is_null() {
        unsafe {
            let _ = EndPaint(hwnd, &paint);
        };
    }
    LRESULT(0)
}

fn drive_shell_render_path(hwnd: HWND) -> Result<()> {
    let scene = build_live_shell_window_scene(hwnd)?;
    let Some(scene) = scene else {
        return Ok(());
    };
    let mut client_rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut client_rect) }
        .wrap_err("failed to get shell window client rect for render")?;
    let width = (client_rect.right - client_rect.left).max(0) as u32;
    let height = (client_rect.bottom - client_rect.top).max(0) as u32;
    SHELL_TEXT_RENDERER_HOSTS.with(|text_renderer_hosts| {
        let mut hosts = text_renderer_hosts.borrow_mut();
        let Some(host) = hosts.get_mut(&(hwnd.0 as isize)) else {
            return Ok::<(), eyre::Report>(());
        };
        if host.width != width || host.height != height {
            host.resize_swap_chain(width.max(1), height.max(1))?;
        }
        host.upload_scene(&scene)?;
        host.present_scene_frame([0.0, 0.0, 0.0, 0.0])
    })
}

fn handle_shell_mouse_move(hwnd: HWND, lparam: LPARAM) -> Result<()> {
    let point = POINT {
        x: (lparam.0 & 0xFFFF) as i16 as i32,
        y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
    };
    if let Some(state) = shell_window_states()
        .lock()
        .expect("shell window state should not be poisoned")
        .get_mut(&(hwnd.0 as isize))
    {
        state.cursor_client_point = Some(point);
    }
    if let Some(context) = shell_window_scene_context(hwnd)?
        && let Some(registration) = feature_window_scene_registration(context.title)
        && let Some(handler) = registration.on_right_drag
        && handler(context, point)
    {
        update_shell_window_cursor(hwnd, point)?;
        return drive_shell_render_path(hwnd);
    }
    update_shell_window_cursor(hwnd, point)?;
    drive_shell_render_path(hwnd)
}

fn handle_shell_mouse_wheel(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> Result<()> {
    let point = POINT {
        x: (lparam.0 & 0xFFFF) as i16 as i32,
        y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
    };
    let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;

    if let Some(state) = shell_window_states()
        .lock()
        .expect("shell window state should not be poisoned")
        .get_mut(&(hwnd.0 as isize))
    {
        state.cursor_client_point = Some(point);
    }

    if let Some(context) = shell_window_scene_context(hwnd)?
        && let Some(registration) = feature_window_scene_registration(context.title)
        && let Some(handler) = registration.on_mouse_wheel
        && handler(context, point, delta)
    {
        update_shell_window_cursor(hwnd, point)?;
        return drive_shell_render_path(hwnd);
    }

    Ok(())
}

fn handle_shell_right_button_down(hwnd: HWND, lparam: LPARAM) -> Result<()> {
    let point = POINT {
        x: (lparam.0 & 0xFFFF) as i16 as i32,
        y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
    };
    let handled = shell_window_scene_context(hwnd)?
        .and_then(|context| {
            feature_window_scene_registration(context.title).and_then(|registration| {
                registration
                    .on_right_button_down
                    .map(|handler| handler(context, point))
            })
        })
        .unwrap_or(false);
    if handled {
        update_shell_window_cursor(hwnd, point)?;
        drive_shell_render_path(hwnd)?;
    }
    Ok(())
}

fn handle_shell_right_button_up(hwnd: HWND, lparam: LPARAM) -> Result<()> {
    let point = POINT {
        x: (lparam.0 & 0xFFFF) as i16 as i32,
        y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
    };
    let handled = shell_window_scene_context(hwnd)?
        .and_then(|context| {
            feature_window_scene_registration(context.title).and_then(|registration| {
                registration
                    .on_right_button_up
                    .map(|handler| handler(context, point))
            })
        })
        .unwrap_or(false);
    if handled {
        update_shell_window_cursor(hwnd, point)?;
        drive_shell_render_path(hwnd)?;
    }
    Ok(())
}

fn handle_shell_frame_tick(hwnd: HWND) -> Result<()> {
    let Some(context) = shell_window_scene_context(hwnd)? else {
        return Ok(());
    };
    let Some(registration) = feature_window_scene_registration(context.title) else {
        return Ok(());
    };
    if let Some(handler) = registration.on_frame_tick {
        let _ = handler(context);
    }
    Ok(())
}

fn handle_shell_left_button_up(hwnd: HWND, lparam: LPARAM) -> Result<()> {
    let point = POINT {
        x: (lparam.0 & 0xFFFF) as i16 as i32,
        y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
    };
    let dpi = shell_window_states()
        .lock()
        .expect("shell window state should not be poisoned")
        .get(&(hwnd.0 as isize))
        .map_or_else(system_dpi, |state| state.dpi);
    let layout = shell_window_layout(hwnd, dpi)?;
    if let Some(action) = shell_chrome_click_action(layout, point) {
        match action {
            ShellChromeClickAction::Close => unsafe {
                let _ = DestroyWindow(hwnd);
            },
            ShellChromeClickAction::Minimize => unsafe {
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            },
            ShellChromeClickAction::MaximizeRestore => unsafe {
                let _ = ShowWindow(hwnd, SW_MAXIMIZE);
            },
            ShellChromeClickAction::Pin
            | ShellChromeClickAction::Diagnostics
            | ShellChromeClickAction::Latency => {}
        }
        return drive_shell_render_path(hwnd);
    }

    let Some(context) = shell_window_scene_context(hwnd)? else {
        return Ok(());
    };

    if let Some(registration) = feature_window_scene_registration(context.title)
        && let Some(handler) = registration.on_left_click
        && handler(context, point)
    {
        update_shell_window_cursor(hwnd, point)?;
        return drive_shell_render_path(hwnd);
    }

    Ok(())
}

fn shell_non_client_hit_test(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let x = (lparam.0 & 0xFFFF) as i16 as i32;
    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
    let mut window_rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut window_rect) }.is_err() {
        return LRESULT(HTCLIENT as isize);
    }
    let width = window_rect.right - window_rect.left;
    let height = window_rect.bottom - window_rect.top;
    let layout = ShellSceneLayout::for_main_menu_with_dpi(width, height, window_dpi(hwnd));
    let border = scale_for_dpi(8, window_dpi(hwnd));
    let local_x = x - window_rect.left;
    let local_y = y - window_rect.top;

    let left = local_x < border;
    let right = local_x >= width - border;
    let top = local_y < border;
    let bottom = local_y >= height - border;

    if top && left {
        return LRESULT(HTTOPLEFT as isize);
    }
    if top && right {
        return LRESULT(HTTOPRIGHT as isize);
    }
    if bottom && left {
        return LRESULT(HTBOTTOMLEFT as isize);
    }
    if bottom && right {
        return LRESULT(HTBOTTOMRIGHT as isize);
    }
    if left {
        return LRESULT(HTLEFT as isize);
    }
    if right {
        return LRESULT(HTRIGHT as isize);
    }
    if top {
        return LRESULT(HTTOP as isize);
    }
    if bottom {
        return LRESULT(HTBOTTOM as isize);
    }

    if local_y >= layout.title_bar_rect.top
        && local_y < layout.title_bar_rect.bottom
        && shell_chrome_click_action(
            layout,
            POINT {
                x: local_x,
                y: local_y,
            },
        )
        .is_none()
    {
        return LRESULT(HTCAPTION as isize);
    }

    LRESULT(HTCLIENT as isize)
}

fn handle_shell_dpi_changed(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let dpi = u32::try_from(wparam.0 & 0xFFFF).unwrap_or_else(|_| window_dpi(hwnd));
    if let Some(state) = shell_window_states()
        .lock()
        .expect("shell window state should not be poisoned")
        .get_mut(&(hwnd.0 as isize))
    {
        state.dpi = dpi;
    }
    let suggested_rect = unsafe { &*(lparam.0 as *const RECT) };
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            suggested_rect.left,
            suggested_rect.top,
            suggested_rect.right - suggested_rect.left,
            suggested_rect.bottom - suggested_rect.top,
            Default::default(),
        );
    }
    let _ = drive_shell_render_path(hwnd);
    LRESULT(0)
}

fn shell_window_class_name(chrome_kind: WindowChromeKind) -> PCWSTR {
    match chrome_kind {
        WindowChromeKind::Standard => STANDARD_WINDOW_CLASS_NAME,
        WindowChromeKind::Tool => TOOL_WINDOW_CLASS_NAME,
    }
}

fn ensure_shell_window_class_registered(chrome_kind: WindowChromeKind) -> Result<()> {
    let once = match chrome_kind {
        WindowChromeKind::Standard => &STANDARD_WINDOW_CLASS_REGISTERED,
        WindowChromeKind::Tool => &TOOL_WINDOW_CLASS_REGISTERED,
    };

    if once.get().is_none() {
        let instance = unsafe { GetModuleHandleW(None) }.wrap_err("failed to get module handle")?;
        let cursor = unsafe { LoadCursorW(Some(HINSTANCE::default()), IDC_ARROW) }
            .wrap_err("failed to load arrow cursor")?;
        let class = WNDCLASSEXW {
            cbSize: u32::try_from(std::mem::size_of::<WNDCLASSEXW>())
                .expect("WNDCLASSEXW size must fit in u32"),
            style: CS_DBLCLKS,
            hInstance: instance.into(),
            lpszClassName: shell_window_class_name(chrome_kind),
            lpfnWndProc: Some(shell_window_proc),
            hCursor: cursor,
            ..Default::default()
        };

        let atom = unsafe { RegisterClassExW(&class) };
        if atom == 0 {
            return Err(eyre::eyre!("failed to register shell window class"));
        }

        let _ = once.set(());
    }

    Ok(())
}

fn centered_window_origin() -> (i32, i32) {
    let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let screen_height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    (
        (screen_width - INITIAL_WINDOW_WIDTH) / 2,
        (screen_height - INITIAL_WINDOW_HEIGHT) / 2,
    )
}

fn wide_null_terminated(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn initialize_dpi_awareness() {
    let _ = DPI_AWARENESS_INITIALIZED.get_or_init(|| unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    });
}

#[must_use]
pub fn system_dpi() -> u32 {
    let dpi = unsafe { GetDpiForSystem() };
    if dpi == 0 { 96 } else { dpi }
}

#[must_use]
pub fn window_dpi(hwnd: HWND) -> u32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { system_dpi() } else { dpi }
}

/// Create a native Win32 window for the provided shell request.
///
/// # Errors
///
/// Returns an error if the shell window class cannot be registered or the
/// window cannot be created with the request's native host plan.
pub fn create_native_window(request: &WindowCreateRequest) -> Result<NativeWindowHandle> {
    ensure_shell_window_class_registered(request.host_options.chrome_kind)?;

    let instance = unsafe { GetModuleHandleW(None) }.wrap_err("failed to get module handle")?;
    let native_host_plan = request
        .host_options
        .native_host_plan(request.present_policy);
    let ex_style = native_window_ex_style(native_host_plan);
    let style = native_window_style(native_host_plan);
    let (x, y) = centered_window_origin();
    let title = wide_null_terminated(request.title);

    let hwnd = unsafe {
        CreateWindowExW(
            ex_style,
            shell_window_class_name(request.host_options.chrome_kind),
            PCWSTR(title.as_ptr()),
            style,
            x,
            y,
            INITIAL_WINDOW_WIDTH,
            INITIAL_WINDOW_HEIGHT,
            None,
            None,
            Some(instance.into()),
            Some(std::ptr::null()),
        )
    }
    .wrap_err("failed to create shell window")?;

    shell_window_states()
        .lock()
        .expect("shell window state should not be poisoned")
        .insert(
            hwnd.0 as isize,
            NativeShellWindowState {
                title: request.title,
                dpi: window_dpi(hwnd),
                chrome: WindowChromeButtonsState {
                    focused: true,
                    ..Default::default()
                },
                cursor_client_point: None,
            },
        );

    if matches!(request.present_policy, PresentPolicy::Composed)
        && feature_window_scene_registration(request.title).is_some()
    {
        initialize_shell_window_render_state(hwnd)?;
    }

    match native_host_plan.initial_command {
        InitialWindowCommand::Show => {
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOW);
            };
        }
        InitialWindowCommand::ShowNoActivate => {
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOWNA);
            };
        }
        InitialWindowCommand::Hidden => {}
    }

    if native_host_plan.bring_to_front {
        unsafe { BringWindowToTop(hwnd) }.wrap_err("failed to bring shell window to top")?;
    }

    Ok(NativeWindowHandle(hwnd.0))
}

/// Destroy a previously created native shell window.
///
/// # Errors
///
/// Returns an error if the native handle is invalid or the OS rejects the
/// destroy request.
pub fn destroy_native_window(handle: NativeWindowHandle) -> Result<()> {
    if !handle.is_valid() {
        return Err(eyre::eyre!(
            "cannot destroy an invalid native shell window handle"
        ));
    }

    unsafe { DestroyWindow(handle.as_hwnd()) }.wrap_err("failed to destroy shell window")?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowHostOptions {
    pub chrome_kind: WindowChromeKind,
    pub visibility: WindowVisibility,
    pub activation: WindowActivationPolicy,
}

impl WindowHostOptions {
    #[must_use]
    // windowing[impl launcher.startup.foreground]
    pub const fn standard_foreground() -> Self {
        Self {
            chrome_kind: WindowChromeKind::Standard,
            visibility: WindowVisibility::Show,
            activation: WindowActivationPolicy::Foreground,
        }
    }

    #[must_use]
    // timeline[impl playground.hover-detail-no-activate]
    pub const fn tool_no_activate() -> Self {
        Self {
            chrome_kind: WindowChromeKind::Tool,
            visibility: WindowVisibility::ShowNoActivate,
            activation: WindowActivationPolicy::NoActivate,
        }
    }

    #[must_use]
    pub const fn hidden_tool() -> Self {
        Self {
            chrome_kind: WindowChromeKind::Tool,
            visibility: WindowVisibility::Hidden,
            activation: WindowActivationPolicy::NoActivate,
        }
    }

    #[must_use]
    pub const fn native_host_plan(self, present_policy: PresentPolicy) -> NativeWindowHostPlan {
        let tool_window = matches!(self.chrome_kind, WindowChromeKind::Tool);
        let no_activate = matches!(self.activation, WindowActivationPolicy::NoActivate);
        let no_redirection_bitmap = !matches!(present_policy, PresentPolicy::Composed);

        NativeWindowHostPlan {
            style: NativeWindowStylePlan {
                app_window: !tool_window,
                tool_window,
                no_activate,
                no_redirection_bitmap,
                popup: true,
                thick_frame: true,
                minimize_box: !tool_window,
                maximize_box: !tool_window,
            },
            initial_command: match self.visibility {
                WindowVisibility::Show => InitialWindowCommand::Show,
                WindowVisibility::ShowNoActivate => InitialWindowCommand::ShowNoActivate,
                WindowVisibility::Hidden => InitialWindowCommand::Hidden,
            },
            bring_to_front: matches!(self.activation, WindowActivationPolicy::Foreground)
                && matches!(self.visibility, WindowVisibility::Show),
        }
    }
}

impl Default for WindowHostOptions {
    fn default() -> Self {
        Self::standard_foreground()
    }
}

impl PresentPolicy {
    #[must_use]
    pub const fn renderer_host_mode(self) -> RendererHostMode {
        match self {
            Self::Composed => RendererHostMode::Composed,
            Self::LowLatencyHwnd => RendererHostMode::LowLatencyHwnd,
            Self::LateLatchedPointer => RendererHostMode::LateLatchedPointer,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowCreateRequest {
    pub logical_window_id: LogicalWindowId,
    pub title: &'static str,
    pub present_policy: PresentPolicy,
    pub host_options: WindowHostOptions,
}

pub static WINDOW_CREATE_REQUEST_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x31; 16]),
    schema_name: "teamy_studio.shell.window_create_request",
    schema_version: 1,
    log_intent: EventLogIntent::NONE,
};

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static WINDOW_CREATE_REQUEST_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &WINDOW_CREATE_REQUEST_EVENT_DEFINITION,
        provenance: registration_provenance!(),
    };

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowCreatedEvent {
    pub logical_window_id: LogicalWindowId,
    pub title: &'static str,
    pub present_policy: PresentPolicy,
    pub host_options: WindowHostOptions,
}

pub static WINDOW_CREATED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x32; 16]),
    schema_name: "teamy_studio.shell.window_created",
    schema_version: 1,
    log_intent: EventLogIntent::NONE,
};

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static WINDOW_CREATED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &WINDOW_CREATED_EVENT_DEFINITION,
        provenance: registration_provenance!(),
    };

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellWindowState {
    pub logical_window_id: LogicalWindowId,
    pub title: &'static str,
    pub present_policy: PresentPolicy,
    pub host_options: WindowHostOptions,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HostedWindowId(u64);

impl HostedWindowId {
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostedWindowRecord {
    pub hosted_window_id: HostedWindowId,
    pub logical_window_id: LogicalWindowId,
    pub title: &'static str,
    pub present_policy: PresentPolicy,
    pub host_options: WindowHostOptions,
    pub native_host_plan: NativeWindowHostPlan,
    pub renderer_host_mode: RendererHostMode,
    pub native_window_handle: Option<NativeWindowHandle>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ShellHostMode {
    #[default]
    Simulated,
    Native,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FeatureWindowHostScaffold {
    next_hosted_window_id: u64,
    hosted_windows: Vec<HostedWindowRecord>,
}

impl FeatureWindowHostScaffold {
    fn push_hosted_window_record(
        &mut self,
        request: &WindowCreateRequest,
        native_window_handle: Option<NativeWindowHandle>,
    ) {
        self.next_hosted_window_id += 1;
        self.hosted_windows.push(HostedWindowRecord {
            hosted_window_id: HostedWindowId(self.next_hosted_window_id),
            logical_window_id: request.logical_window_id,
            title: request.title,
            present_policy: request.present_policy,
            host_options: request.host_options,
            native_host_plan: request
                .host_options
                .native_host_plan(request.present_policy),
            renderer_host_mode: request.present_policy.renderer_host_mode(),
            native_window_handle,
        });
    }

    pub fn create_window(&mut self, request: &WindowCreateRequest) -> WindowCreatedEvent {
        self.push_hosted_window_record(request, None);

        WindowCreatedEvent {
            logical_window_id: request.logical_window_id,
            title: request.title,
            present_policy: request.present_policy,
            host_options: request.host_options,
        }
    }

    /// Create a real native Win32 window and record its handle in the host scaffold.
    ///
    /// # Errors
    ///
    /// Returns an error if the shell window cannot be created natively.
    pub fn create_native_window(
        &mut self,
        request: &WindowCreateRequest,
    ) -> Result<WindowCreatedEvent> {
        let native_window_handle = create_native_window(request)?;
        self.push_hosted_window_record(request, Some(native_window_handle));
        Ok(WindowCreatedEvent {
            logical_window_id: request.logical_window_id,
            title: request.title,
            present_policy: request.present_policy,
            host_options: request.host_options,
        })
    }

    #[must_use]
    pub fn hosted_windows(&self) -> &[HostedWindowRecord] {
        &self.hosted_windows
    }

    #[must_use]
    pub fn handle_published_event(&mut self, event: &PublishedEvent) -> Vec<PublishedEvent> {
        event
            .downcast_ref::<WindowCreateRequest>()
            .map(|request| self.create_window(request))
            .map(|created_window| {
                PublishedEvent::new(&WINDOW_CREATED_EVENT_DEFINITION, created_window)
            })
            .into_iter()
            .collect()
    }

    /// Handle a published event by materializing matching requests as native windows.
    ///
    /// # Errors
    ///
    /// Returns an error if a matching request cannot be created as a native window.
    pub fn handle_published_event_natively(
        &mut self,
        event: &PublishedEvent,
    ) -> Result<Vec<PublishedEvent>> {
        event
            .downcast_ref::<WindowCreateRequest>()
            .map(|request| self.create_native_window(request))
            .transpose()
            .map(|created_window| {
                created_window
                    .map(|created_window| {
                        vec![PublishedEvent::new(
                            &WINDOW_CREATED_EVENT_DEFINITION,
                            created_window,
                        )]
                    })
                    .unwrap_or_default()
            })
    }

    /// Destroy any native windows currently recorded by this scaffold.
    ///
    /// # Errors
    ///
    /// Returns an error if one of the recorded native windows cannot be destroyed.
    pub fn destroy_native_windows(&mut self) -> Result<()> {
        for hosted_window in &mut self.hosted_windows {
            if let Some(handle) = hosted_window.native_window_handle.take() {
                destroy_native_window(handle)?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ShellState {
    pending_requests: Vec<WindowCreateRequest>,
    windows: Vec<ShellWindowState>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellRuntime {
    host_mode: ShellHostMode,
    host_runtime: TriggerRuntime,
    host_scaffold: FeatureWindowHostScaffold,
    state: ShellState,
}

impl Default for ShellRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellState {
    pub fn apply_create_request(&mut self, request: &WindowCreateRequest) {
        self.pending_requests.push(request.clone());
    }

    pub fn apply_window_created(&mut self, created_window: &WindowCreatedEvent) {
        self.pending_requests
            .retain(|request| request.logical_window_id != created_window.logical_window_id);
        self.windows.push(ShellWindowState {
            logical_window_id: created_window.logical_window_id,
            title: created_window.title,
            present_policy: created_window.present_policy,
            host_options: created_window.host_options,
        });
    }

    #[must_use]
    pub fn pending_requests(&self) -> &[WindowCreateRequest] {
        &self.pending_requests
    }

    #[must_use]
    pub fn windows(&self) -> &[ShellWindowState] {
        &self.windows
    }
}

impl ShellRuntime {
    #[must_use]
    pub fn new() -> Self {
        Self::new_with_host_mode(ShellHostMode::Simulated)
    }

    #[must_use]
    pub fn new_with_native_hosting() -> Self {
        Self::new_with_host_mode(ShellHostMode::Native)
    }

    #[must_use]
    pub fn new_with_host_mode(host_mode: ShellHostMode) -> Self {
        Self {
            host_mode,
            host_runtime: TriggerRuntime::default(),
            host_scaffold: FeatureWindowHostScaffold::default(),
            state: ShellState::default(),
        }
    }

    pub fn apply_published_event(&mut self, event: &PublishedEvent) {
        apply_published_event(&mut self.state, event);
    }

    /// Run the shell host stage over unseen timeline events.
    ///
    /// # Errors
    ///
    /// Returns an error if native hosting is enabled and one of the unseen
    /// window-create requests cannot be materialized as a native window.
    pub fn run_host_stage(
        &mut self,
        timeline: &mut ConstructedTimeline<PublishedEvent>,
        time_key: CanonicalTimeKey,
    ) -> Result<usize> {
        let mut emitted_events = Vec::new();
        self.host_runtime
            .pump_unseen(timeline, |_, event| match self.host_mode {
                ShellHostMode::Simulated => {
                    emitted_events.extend(self.host_scaffold.handle_published_event(event));
                    Ok::<(), eyre::Report>(())
                }
                ShellHostMode::Native => {
                    emitted_events
                        .extend(self.host_scaffold.handle_published_event_natively(event)?);
                    Ok::<(), eyre::Report>(())
                }
            })?;

        if emitted_events.is_empty() {
            return Ok(0);
        }

        for event in &emitted_events {
            self.apply_published_event(event);
        }

        let mut epoch = WritableArena::new("teamy_studio.shell");
        for event in emitted_events {
            epoch.push(event);
        }
        timeline.ingest(time_key, epoch.seal());
        Ok(1)
    }

    #[must_use]
    pub fn state(&self) -> &ShellState {
        &self.state
    }

    #[must_use]
    pub fn hosted_windows(&self) -> &[HostedWindowRecord] {
        self.host_scaffold.hosted_windows()
    }

    #[must_use]
    pub fn host_mode(&self) -> ShellHostMode {
        self.host_mode
    }

    /// Enable native hosting on a pristine shell runtime before any window
    /// requests or hosted windows exist.
    ///
    /// # Errors
    ///
    /// Returns an error if shell state or hosted windows have already been
    /// materialized, because switching host mode after that point would make
    /// runtime ownership ambiguous.
    pub fn enable_native_hosting_for_pristine_runtime(&mut self) -> Result<()> {
        if self.host_mode == ShellHostMode::Native {
            return Ok(());
        }

        if !self.state.pending_requests().is_empty()
            || !self.state.windows().is_empty()
            || !self.hosted_windows().is_empty()
        {
            return Err(eyre!(
                "cannot enable native hosting after shell runtime has observed or materialized windows"
            ));
        }

        self.host_mode = ShellHostMode::Native;
        Ok(())
    }

    /// Destroy any native windows currently owned by the shell runtime.
    ///
    /// # Errors
    ///
    /// Returns an error if one of the runtime's native windows cannot be destroyed.
    pub fn destroy_native_windows(&mut self) -> Result<()> {
        self.host_scaffold.destroy_native_windows()
    }

    #[must_use]
    pub fn into_parts(self) -> (FeatureWindowHostScaffold, ShellState) {
        (self.host_scaffold, self.state)
    }
}

#[must_use]
pub fn create_window(request: &WindowCreateRequest) -> WindowCreatedEvent {
    let mut scaffold = FeatureWindowHostScaffold::default();
    scaffold.create_window(request)
}

pub fn apply_published_event(state: &mut ShellState, event: &PublishedEvent) {
    if let Some(request) = event.downcast_ref::<WindowCreateRequest>() {
        state.apply_create_request(request);
    }
    if let Some(created_window) = event.downcast_ref::<WindowCreatedEvent>() {
        state.apply_window_created(created_window);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FeatureWindowHostScaffold, InitialWindowCommand, LogicalWindowId, NativeWindowHostPlan,
        NativeWindowStylePlan, PresentPolicy, RendererHostMode, ShellHostMode, ShellRuntime,
        ShellState, WindowActivationPolicy, WindowChromeKind, WindowCreateRequest,
        WindowCreatedEvent, WindowHostOptions, WindowVisibility, apply_published_event,
        create_window, destroy_native_window, native_window_ex_style, native_window_style,
    };
    use super::{WINDOW_CREATE_REQUEST_EVENT_DEFINITION, WINDOW_CREATED_EVENT_DEFINITION};
    use teamy_studio_event_core::{PublishedEvent, WritableArena};
    use teamy_studio_timeline_core::{CanonicalTimeKey, ConstructedTimeline};
    use windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE;
    use windows::Win32::UI::WindowsAndMessaging::{
        WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW,
        WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_THICKFRAME, WS_VISIBLE,
    };

    #[test]
    fn create_window_preserves_request_identity() {
        let request = WindowCreateRequest {
            logical_window_id: LogicalWindowId::new(99),
            title: "Cursor Gallery",
            present_policy: PresentPolicy::LowLatencyHwnd,
            host_options: WindowHostOptions::standard_foreground(),
        };

        let created = create_window(&request);

        assert_eq!(
            created,
            WindowCreatedEvent {
                logical_window_id: LogicalWindowId::new(99),
                title: "Cursor Gallery",
                present_policy: PresentPolicy::LowLatencyHwnd,
                host_options: WindowHostOptions::standard_foreground(),
            }
        );
    }

    #[test]
    fn host_scaffold_tracks_hosted_windows_separately_from_public_events() {
        let first_request = WindowCreateRequest {
            logical_window_id: LogicalWindowId::new(99),
            title: "Cursor Gallery",
            present_policy: PresentPolicy::LowLatencyHwnd,
            host_options: WindowHostOptions::standard_foreground(),
        };
        let second_request = WindowCreateRequest {
            logical_window_id: LogicalWindowId::new(100),
            title: "Timeline Inspector",
            present_policy: PresentPolicy::Composed,
            host_options: WindowHostOptions::tool_no_activate(),
        };
        let mut scaffold = FeatureWindowHostScaffold::default();

        let first_created = scaffold.create_window(&first_request);
        let second_created = scaffold.create_window(&second_request);

        assert_eq!(first_created.logical_window_id.raw(), 99);
        assert_eq!(second_created.logical_window_id.raw(), 100);
        assert_eq!(scaffold.hosted_windows().len(), 2);
        assert_eq!(scaffold.hosted_windows()[0].hosted_window_id.raw(), 1);
        assert_eq!(scaffold.hosted_windows()[1].hosted_window_id.raw(), 2);
        assert_eq!(
            scaffold.hosted_windows()[1].host_options,
            WindowHostOptions::tool_no_activate()
        );
        assert_eq!(
            scaffold.hosted_windows()[1].native_host_plan,
            NativeWindowHostPlan {
                style: NativeWindowStylePlan {
                    app_window: false,
                    tool_window: true,
                    no_activate: true,
                    no_redirection_bitmap: false,
                    popup: true,
                    thick_frame: true,
                    minimize_box: false,
                    maximize_box: false,
                },
                initial_command: InitialWindowCommand::ShowNoActivate,
                bring_to_front: false,
            }
        );
        assert_eq!(
            scaffold.hosted_windows()[0].renderer_host_mode,
            RendererHostMode::LowLatencyHwnd
        );
        assert_eq!(scaffold.hosted_windows()[0].native_window_handle, None);
    }

    #[test]
    fn host_options_materialize_native_host_plan() {
        assert_eq!(
            WindowHostOptions::standard_foreground()
                .native_host_plan(PresentPolicy::LowLatencyHwnd),
            NativeWindowHostPlan {
                style: NativeWindowStylePlan {
                    app_window: true,
                    tool_window: false,
                    no_activate: false,
                    no_redirection_bitmap: true,
                    popup: true,
                    thick_frame: true,
                    minimize_box: true,
                    maximize_box: true,
                },
                initial_command: InitialWindowCommand::Show,
                bring_to_front: true,
            }
        );
        assert_eq!(
            WindowHostOptions::hidden_tool()
                .native_host_plan(PresentPolicy::Composed)
                .initial_command,
            InitialWindowCommand::Hidden
        );
    }

    #[test]
    fn native_host_plan_translates_to_win32_styles() {
        let launcher_plan = WindowHostOptions::standard_foreground()
            .native_host_plan(PresentPolicy::LowLatencyHwnd);
        let detail_plan =
            WindowHostOptions::tool_no_activate().native_host_plan(PresentPolicy::LowLatencyHwnd);

        let launcher_ex_style = native_window_ex_style(launcher_plan);
        let launcher_style = native_window_style(launcher_plan);
        let detail_ex_style = native_window_ex_style(detail_plan);
        let detail_style = native_window_style(detail_plan);

        assert_eq!(launcher_ex_style & WS_EX_APPWINDOW, WS_EX_APPWINDOW);
        assert_eq!(
            launcher_ex_style & WS_EX_NOREDIRECTIONBITMAP,
            WS_EX_NOREDIRECTIONBITMAP
        );
        assert_eq!(launcher_style & WS_VISIBLE, WS_VISIBLE);
        assert_eq!(launcher_style & WS_POPUP, WS_POPUP);
        assert_eq!(launcher_style & WS_THICKFRAME, WS_THICKFRAME);
        assert_eq!(launcher_style & WS_MINIMIZEBOX, WS_MINIMIZEBOX);
        assert_eq!(launcher_style & WS_MAXIMIZEBOX, WS_MAXIMIZEBOX);

        assert_eq!(detail_ex_style & WS_EX_TOOLWINDOW, WS_EX_TOOLWINDOW);
        assert_eq!(detail_ex_style & WS_EX_NOACTIVATE, WS_EX_NOACTIVATE);
        assert_eq!(
            detail_ex_style & WS_EX_NOREDIRECTIONBITMAP,
            WS_EX_NOREDIRECTIONBITMAP
        );
        assert_eq!(detail_style & WS_VISIBLE, WS_VISIBLE);
        assert_eq!(detail_style & WS_POPUP, WS_POPUP);
        assert_eq!(detail_style & WS_THICKFRAME, WS_THICKFRAME);
    }

    #[test]
    fn composed_native_host_plan_keeps_redirection_bitmap_enabled() {
        let plan =
            WindowHostOptions::standard_foreground().native_host_plan(PresentPolicy::Composed);
        let ex_style = native_window_ex_style(plan);

        assert_eq!(ex_style & WS_EX_NOREDIRECTIONBITMAP, WINDOW_EX_STYLE(0));
    }

    #[test]
    fn native_window_lifecycle_supports_hidden_tool_windows() {
        let request = WindowCreateRequest {
            logical_window_id: LogicalWindowId::new(77),
            title: "Hidden Tool Window",
            present_policy: PresentPolicy::Composed,
            host_options: WindowHostOptions::hidden_tool(),
        };
        let mut scaffold = FeatureWindowHostScaffold::default();

        let created = scaffold
            .create_native_window(&request)
            .expect("native hidden tool window should be creatable");
        let native_window_handle = scaffold.hosted_windows()[0]
            .native_window_handle
            .expect("native window handle should be recorded");

        assert_eq!(created.logical_window_id.raw(), 77);
        assert!(native_window_handle.is_valid());
        assert_eq!(
            scaffold.hosted_windows()[0]
                .native_host_plan
                .initial_command,
            InitialWindowCommand::Hidden
        );

        destroy_native_window(native_window_handle)
            .expect("native hidden tool window should be destroyable");
    }

    #[test]
    fn shell_runtime_can_be_configured_for_native_hosting() {
        let runtime = ShellRuntime::new_with_native_hosting();

        assert_eq!(runtime.host_mode(), ShellHostMode::Native);
    }

    #[test]
    fn host_scaffold_converts_create_requests_into_created_events() {
        let request = WindowCreateRequest {
            logical_window_id: LogicalWindowId::new(99),
            title: "Cursor Gallery",
            present_policy: PresentPolicy::LowLatencyHwnd,
            host_options: WindowHostOptions::standard_foreground(),
        };
        let mut scaffold = FeatureWindowHostScaffold::default();

        let emitted = scaffold.handle_published_event(&PublishedEvent::new(
            &WINDOW_CREATE_REQUEST_EVENT_DEFINITION,
            request,
        ));

        assert_eq!(emitted.len(), 1);
        assert_eq!(scaffold.hosted_windows().len(), 1);
        assert_eq!(
            emitted[0].definition().id,
            WINDOW_CREATED_EVENT_DEFINITION.id
        );
    }

    #[test]
    fn shell_state_tracks_requests_and_created_windows() {
        let request = WindowCreateRequest {
            logical_window_id: LogicalWindowId::new(99),
            title: "Cursor Gallery",
            present_policy: PresentPolicy::LowLatencyHwnd,
            host_options: WindowHostOptions::standard_foreground(),
        };
        let created = WindowCreatedEvent {
            logical_window_id: LogicalWindowId::new(99),
            title: "Cursor Gallery",
            present_policy: PresentPolicy::LowLatencyHwnd,
            host_options: WindowHostOptions::standard_foreground(),
        };
        let mut state = ShellState::default();

        apply_published_event(
            &mut state,
            &PublishedEvent::new(&WINDOW_CREATE_REQUEST_EVENT_DEFINITION, request.clone()),
        );
        assert_eq!(state.pending_requests(), &[request]);
        assert!(state.windows().is_empty());

        apply_published_event(
            &mut state,
            &PublishedEvent::new(&WINDOW_CREATED_EVENT_DEFINITION, created),
        );
        assert!(state.pending_requests().is_empty());
        assert_eq!(state.windows().len(), 1);
        assert_eq!(state.windows()[0].logical_window_id.raw(), 99);
        assert_eq!(
            state.windows()[0].host_options,
            WindowHostOptions::standard_foreground()
        );
    }

    #[test]
    fn shell_runtime_pumps_host_stage_and_tracks_hosted_windows() {
        let request = WindowCreateRequest {
            logical_window_id: LogicalWindowId::new(99),
            title: "Cursor Gallery",
            present_policy: PresentPolicy::LowLatencyHwnd,
            host_options: WindowHostOptions {
                chrome_kind: WindowChromeKind::Tool,
                visibility: WindowVisibility::ShowNoActivate,
                activation: WindowActivationPolicy::NoActivate,
            },
        };
        let expected_host_options = request.host_options;
        let mut timeline = ConstructedTimeline::new();
        let mut runtime = ShellRuntime::new();
        let mut epoch = WritableArena::new("teamy_studio.cursor_gallery");
        epoch.push(PublishedEvent::new(
            &WINDOW_CREATE_REQUEST_EVENT_DEFINITION,
            request.clone(),
        ));
        timeline.ingest(CanonicalTimeKey::from_femtoseconds(10), epoch.seal());

        runtime.apply_published_event(&PublishedEvent::new(
            &WINDOW_CREATE_REQUEST_EVENT_DEFINITION,
            request,
        ));

        let emitted_epoch_count = runtime
            .run_host_stage(&mut timeline, CanonicalTimeKey::from_femtoseconds(11))
            .expect("simulated shell host stage should succeed");

        assert_eq!(emitted_epoch_count, 1);
        assert!(runtime.state().pending_requests().is_empty());
        assert_eq!(runtime.state().windows().len(), 1);
        assert_eq!(runtime.hosted_windows().len(), 1);
        assert_eq!(runtime.hosted_windows()[0].hosted_window_id.raw(), 1);
        assert_eq!(
            runtime.hosted_windows()[0].host_options,
            expected_host_options
        );
        assert_eq!(timeline.published_epochs().len(), 2);
        assert_eq!(
            timeline.published_epochs()[1].1.events()[0].definition().id,
            WINDOW_CREATED_EVENT_DEFINITION.id
        );
    }

    #[test]
    fn pristine_shell_runtime_can_enable_native_hosting() {
        let mut runtime = ShellRuntime::new();

        runtime
            .enable_native_hosting_for_pristine_runtime()
            .expect("pristine shell runtime should allow native-hosting upgrade");

        assert_eq!(runtime.host_mode(), ShellHostMode::Native);
    }

    #[test]
    fn shell_runtime_rejects_native_hosting_upgrade_after_observing_requests() {
        let request = WindowCreateRequest {
            logical_window_id: LogicalWindowId::new(99),
            title: "Cursor Gallery",
            present_policy: PresentPolicy::LowLatencyHwnd,
            host_options: WindowHostOptions::standard_foreground(),
        };
        let mut runtime = ShellRuntime::new();

        runtime.apply_published_event(&PublishedEvent::new(
            &WINDOW_CREATE_REQUEST_EVENT_DEFINITION,
            request,
        ));

        let error = runtime
            .enable_native_hosting_for_pristine_runtime()
            .expect_err(
                "shell runtime should reject native-hosting upgrade after observing requests",
            );

        assert_eq!(
            error.to_string(),
            "cannot enable native hosting after shell runtime has observed or materialized windows"
        );
        assert_eq!(runtime.host_mode(), ShellHostMode::Simulated);
    }
}
