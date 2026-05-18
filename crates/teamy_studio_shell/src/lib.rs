mod d3d12_resources;
mod d3d12_smoke;
mod d3d12_sprite_atlas;
mod d3d12_text_pipeline;
mod d3d12_text_renderer_host;
mod d3d12_text_renderer_proxy;
mod d3d12_text_renderer_resources;
mod d3d12_srv;
mod d3d12_upload;
mod scene_cache;
mod scene;
mod scene_packet;
mod scene_upload;
mod scene_upload_batch;
mod scene_vertices;
mod text_atlas;

use std::sync::OnceLock;

use eyre::{Result, WrapErr};
use linkme::distributed_slice;
use teamy_studio_event_core::{EventDefinition, EventDefinitionId, PublishedEvent, WritableArena};
use teamy_studio_registration_core::{EVENT_DEFINITION_REGISTRATIONS, EventDefinitionRegistration};
use teamy_studio_timeline_core::{CanonicalTimeKey, ConstructedTimeline, TriggerRuntime};
use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CS_DBLCLKS, CreateWindowExW, DefWindowProcW, DestroyWindow, GetSystemMetrics,
    IDC_ARROW, LoadCursorW, RegisterClassExW, SM_CXSCREEN, SM_CYSCREEN, SW_SHOW, SW_SHOWNA,
    ShowWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSEXW, WS_EX_APPWINDOW, WS_EX_NOACTIVATE,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP,
    WS_THICKFRAME, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

pub use d3d12_resources::{
    SceneUploadResources, create_scene_upload_buffers, create_scene_vertex_buffer,
    create_shader_param_buffer, curve_buffer_size_bytes, scene_vertex_buffer_size_bytes,
    transformed_glyph_inverse_buffer_size_bytes,
};
pub use d3d12_smoke::create_text_renderer_host_for_scene;
pub use d3d12_text_renderer_host::{TextRendererHost, create_text_renderer_device};
pub use d3d12_text_renderer_proxy::TextRendererThreadProxy;
pub use d3d12_text_renderer_resources::{
    TextRendererResources, create_text_renderer_resources,
};
pub use d3d12_srv::{TextShaderResourceSet, create_text_shader_resource_set};
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

static STANDARD_WINDOW_CLASS_REGISTERED: OnceLock<()> = OnceLock::new();
static TOOL_WINDOW_CLASS_REGISTERED: OnceLock<()> = OnceLock::new();
static DPI_AWARENESS_INITIALIZED: OnceLock<()> = OnceLock::new();

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
    // Safety: this host scaffold currently delegates all behavior to the default
    // window procedure until the shell crate owns a real message loop.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
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
    let _ = DPI_AWARENESS_INITIALIZED.get_or_init(|| {
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
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
    let native_host_plan = request.host_options.native_host_plan(request.present_policy);
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
};

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static WINDOW_CREATE_REQUEST_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &WINDOW_CREATE_REQUEST_EVENT_DEFINITION,
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
};

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static WINDOW_CREATED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &WINDOW_CREATED_EVENT_DEFINITION,
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
            native_host_plan: request.host_options.native_host_plan(request.present_policy),
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
            WindowHostOptions::standard_foreground().native_host_plan(PresentPolicy::LowLatencyHwnd),
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
        let detail_plan = WindowHostOptions::tool_no_activate()
            .native_host_plan(PresentPolicy::LowLatencyHwnd);

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
        let plan = WindowHostOptions::standard_foreground().native_host_plan(PresentPolicy::Composed);
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
        timeline.ingest(CanonicalTimeKey(10), epoch.seal());

        runtime.apply_published_event(&PublishedEvent::new(
            &WINDOW_CREATE_REQUEST_EVENT_DEFINITION,
            request,
        ));

        let emitted_epoch_count = runtime
            .run_host_stage(&mut timeline, CanonicalTimeKey(11))
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
}
