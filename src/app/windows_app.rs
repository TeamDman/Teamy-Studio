use std::cell::RefCell;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
#[cfg(feature = "tracy")]
use tracing::debug_span;
use tracing::trace;

use chrono::Utc;
use eyre::Context;
use facet::Facet;
use teamy_windows::clipboard::{read_clipboard, write_clipboard};
use teamy_windows::module::get_current_module;
use tracing::{debug, error, info, info_span, instrument};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CreateFontIndirectW, DeleteObject, EndPaint, GetDC,
    GetDeviceCaps, GetTextExtentPoint32W, HFONT, LOGFONTW, PAINTSTRUCT, ReleaseDC, SelectObject,
    VREFRESH,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_LBUTTON, VK_MENU};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetCursorPos,
    GetMessageW, GetSystemMetrics, GetWindowRect, HTCAPTION, HTCLIENT, IDC_ARROW, IDC_SIZEALL,
    KillTimer, LoadCursorW, MSG, MoveWindow, PostMessageW, PostQuitMessage, RegisterClassExW,
    SM_CXPADDEDBORDER, SM_CXSCREEN, SM_CXSIZEFRAME, SM_CYSCREEN, SM_CYSIZEFRAME, SW_SHOW,
    SYSTEM_METRICS_INDEX, SetCursor, SetTimer, ShowWindow, TranslateMessage, WM_CHAR, WM_CLOSE,
    WM_DESTROY, WM_ENTERSIZEMOVE, WM_ERASEBKGND, WM_EXITSIZEMOVE, WM_KEYDOWN, WM_KEYUP,
    WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCALCSIZE,
    WM_NCHITTEST, WM_NCLBUTTONDOWN, WM_PAINT, WM_RBUTTONUP, WM_SETCURSOR, WM_SETFOCUS, WM_SIZE,
    WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WNDCLASSEXW, WS_EX_APPWINDOW, WS_MAXIMIZEBOX,
    WS_MINIMIZEBOX, WS_POPUP, WS_THICKFRAME, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

use crate::paths::{AppHome, CacheHome};

use super::spatial::{
    ClientPoint, ClientRect, ScreenPoint, ScreenRect, ScreenToClientTransform, TerminalCellPoint,
    classify_resize_border_hit, drag_threshold_exceeded,
};
use super::windows_d3d12_renderer::{
    RenderFrameModel, RenderThreadProxy, RendererTerminalVisualState,
};
use super::windows_dialogs::{
    PasteConfirmationChoice, paste_confirmation_required, show_multiline_paste_confirmation_dialog,
};
use super::windows_terminal::{
    POLL_INTERVAL_MS, POLL_TIMER_ID, TERMINAL_WORKER_WAKE_MESSAGE, TerminalDisplayCursorStyle,
    TerminalDisplayScrollbar, TerminalDisplayState, TerminalLayout, TerminalPerformanceSnapshot,
    TerminalSelection, TerminalSelectionMode, TerminalSession, keyboard_mods,
};
use super::{TerminalThroughputBenchmarkMode, VtEngineChoice, WorkspaceWindowState};

unsafe extern "system" {
    fn SetCapture(hwnd: HWND) -> HWND;
    fn ReleaseCapture() -> i32;
}

const WINDOW_CLASS_NAME: PCWSTR = w!("TeamyStudioTerminalWindow");
const BENCHMARK_WINDOW_CLASS_NAME: PCWSTR = w!("TeamyStudioTerminalBenchmarkWindow");
const WINDOW_TITLE: &str = "Teamy Studio Terminal";
const TERMINAL_FONT_HEIGHT: i32 = -16;
const OUTPUT_FONT_HEIGHT: i32 = -16;
const FONT_FAMILY: &str = "CaskaydiaCove Nerd Font Mono";
const MIN_FONT_HEIGHT: i32 = -12;
const MAX_FONT_HEIGHT: i32 = -72;
const FONT_ZOOM_STEP: i32 = 2;
const INITIAL_WINDOW_WIDTH: i32 = 1040;
const INITIAL_WINDOW_HEIGHT: i32 = 680;
const DRAG_START_THRESHOLD_PX: i32 = 0;
const MIN_RESIZE_BORDER_THICKNESS: i32 = 1;
const MOUSE_WHEEL_DELTA: i16 = 120;
const TERMINAL_WHEEL_SCROLL_LINES: isize = 3;
const SELECTION_AUTO_SCROLL_MAX_LINES: isize = 12;
const FOCUSED_RENDER_TIMER_ID: usize = 2;
const TERMINAL_THROUGHPUT_BENCHMARK_START_MARKER: &str = "__TEAMY_TERMINAL_THROUGHPUT_START__";
const TERMINAL_THROUGHPUT_BENCHMARK_DONE_MARKER: &str = "__TEAMY_TERMINAL_THROUGHPUT_DONE__";
const TERMINAL_THROUGHPUT_BENCHMARK_MEASURE_PREFIX: &str =
    "__TEAMY_TERMINAL_THROUGHPUT_MEASURE_MS=";
const TERMINAL_THROUGHPUT_BENCHMARK_TIMEOUT: Duration = Duration::from_secs(60);
const TERMINAL_THROUGHPUT_BENCHMARK_POLL_INTERVAL: Duration = Duration::from_millis(1);
const TERMINAL_THROUGHPUT_RESULTS_DIR: &str = "self-test/terminal-throughput";

thread_local! {
    static APP_STATE: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

struct AppState {
    app_home: AppHome,
    hwnd: Option<WindowHandle>,
    workspace_window: Option<WorkspaceWindowState>,
    pending_window_drag: Option<PendingWindowDrag>,
    terminal_selection: Option<TerminalSelection>,
    pending_terminal_selection: Option<PendingTerminalSelection>,
    terminal_selection_drag_point: Option<ClientPoint>,
    terminal_scrollbar_hovered_part: Option<TerminalScrollbarPart>,
    terminal_scrollbar_drag: Option<TerminalScrollbarDrag>,
    in_move_size_loop: bool,
    window_focused: bool,
    terminal_layout: Option<TerminalLayout>,
    pending_terminal_resize: Option<TerminalLayout>,
    terminal_poll_pending: bool,
    focused_render_interval_ms: u32,
    terminal_font_height: i32,
    terminal_cell_width: i32,
    terminal_cell_height: i32,
    output_font_height: i32,
    output_cell_width: i32,
    output_cell_height: i32,
    terminal: TerminalSession,
    renderer: Option<RenderThreadProxy>,
}

// convention[impl convention.invariants.encode-in-types]
#[derive(Clone, Copy, Debug)]
struct WindowThread {
    _thread_affinity: PhantomData<*mut ()>,
}

impl WindowThread {
    fn current() -> Self {
        Self {
            _thread_affinity: PhantomData,
        }
    }

    fn assert_window_thread(self) {
        let _ = self._thread_affinity;
    }

    fn post_quit_message(self) {
        self.assert_window_thread();
        // Safety: this token is only created and used on the UI thread that owns the message queue.
        unsafe { PostQuitMessage(0) };
    }
}

#[derive(Clone, Copy, Debug)]
struct WindowHandle {
    hwnd: HWND,
    window_thread: WindowThread,
}

impl WindowHandle {
    fn new(window_thread: WindowThread, hwnd: HWND) -> Self {
        Self {
            hwnd,
            window_thread,
        }
    }

    fn raw(self) -> HWND {
        self.hwnd
    }

    fn show(self) {
        self.window_thread.assert_window_thread();
        // Safety: `self.hwnd` is a live top-level window owned by this process on `self.window_thread`.
        let _ = unsafe { ShowWindow(self.hwnd, SW_SHOW) };
    }

    fn destroy(self) {
        self.window_thread.assert_window_thread();
        // Safety: `self.hwnd` is a live top-level window owned by this process on `self.window_thread`.
        let _ = unsafe { DestroyWindow(self.hwnd) };
    }

    fn post_close(self) {
        self.window_thread.assert_window_thread();
        // Safety: posting WM_CLOSE to this live top-level window defers destruction until the message loop handles it.
        let _ = unsafe { PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
    }

    fn client_rect(self) -> eyre::Result<ClientRect> {
        self.window_thread.assert_window_thread();
        let mut rect = RECT::default();
        // Safety: `rect` is a valid out-pointer for GetClientRect and `self.hwnd` names the window being queried.
        if unsafe { GetClientRect(self.hwnd, &raw mut rect) }.is_err() {
            eyre::bail!("failed to query client rect")
        }
        Ok(ClientRect::from_win32_rect(rect))
    }

    fn window_rect(self) -> eyre::Result<ScreenRect> {
        self.window_thread.assert_window_thread();
        let mut rect = RECT::default();
        // Safety: `rect` is a valid out-pointer for GetWindowRect and `self.hwnd` names the window being queried.
        if unsafe { GetWindowRect(self.hwnd, &raw mut rect) }.is_err() {
            eyre::bail!("failed to query window rect")
        }
        Ok(ScreenRect::from_win32_rect(rect))
    }

    fn set_poll_timer(self) -> eyre::Result<()> {
        self.set_timer(POLL_TIMER_ID, POLL_INTERVAL_MS)
    }

    fn set_focused_render_timer(self, interval_ms: u32) -> eyre::Result<()> {
        self.set_timer(FOCUSED_RENDER_TIMER_ID, interval_ms)
    }

    fn clear_focused_render_timer(self) {
        self.window_thread.assert_window_thread();
        // Safety: removing a thread-owned timer from this live HWND is valid.
        let _ = unsafe { KillTimer(Some(self.hwnd), FOCUSED_RENDER_TIMER_ID) };
    }

    fn set_timer(self, timer_id: usize, interval_ms: u32) -> eyre::Result<()> {
        self.window_thread.assert_window_thread();
        // Safety: installing a thread-owned timer on a live HWND is valid.
        let timer = unsafe { SetTimer(Some(self.hwnd), timer_id, interval_ms, None) };
        if timer == 0 {
            eyre::bail!("failed to start window timer")
        }
        Ok(())
    }

    fn post_system_drag(self, wparam: WPARAM, lparam: LPARAM) {
        self.window_thread.assert_window_thread();
        // Safety: WM_NCLBUTTONDOWN with HTCAPTION delegates drag handling to the native move loop.
        let _ = unsafe { PostMessageW(Some(self.hwnd), WM_NCLBUTTONDOWN, wparam, lparam) };
    }

    fn post_quit_message(self) {
        self.window_thread.post_quit_message();
    }

    fn capture_mouse(self) {
        self.window_thread.assert_window_thread();
        // Safety: capturing mouse input for this live window during a pointer drag is valid.
        unsafe {
            let _ = SetCapture(self.hwnd);
        }
    }

    fn release_mouse_capture(self) {
        self.window_thread.assert_window_thread();
        // Safety: releasing mouse capture after pointer interaction completes is valid.
        unsafe {
            let _ = ReleaseCapture();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PendingWindowDrag {
    origin: ClientPoint,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PendingTerminalSelection {
    origin: ClientPoint,
    anchor: TerminalCellPoint,
    mode: TerminalSelectionMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingDragAction {
    NotHandled,
    Consumed,
    StartSystemDrag,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingTerminalSelectionAction {
    KeepPending,
    ClearPending,
    Update(TerminalSelection),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RightClickTerminalAction {
    CopySelection,
    Paste,
    ConfirmPaste,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminalScrollbarPart {
    Track,
    Thumb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TerminalScrollbarDrag {
    grab_offset_y: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct TerminalScrollbarVisualState {
    track_hovered: bool,
    thumb_hovered: bool,
    thumb_grabbed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TerminalScrollbarGeometry {
    thumb_rect: ClientRect,
    thumb_height: i32,
    travel: i32,
    max_offset: u64,
}

#[derive(Debug, PartialEq, Eq)]
enum RightClickTerminalPreparation {
    NotTerminal,
    CopySelection(String),
    QueryClipboard,
}

impl PendingDragAction {
    fn clears_pending_drag(self) -> bool {
        matches!(self, Self::NotHandled | Self::StartSystemDrag)
    }
}

struct FontHandle(HFONT);

impl Drop for FontHandle {
    fn drop(&mut self) {
        // Safety: this `HFONT` is owned by `FontHandle` and may be deleted during drop.
        let _ = unsafe { DeleteObject(self.0.into()) };
    }
}

fn def_window_proc(hwnd: WindowHandle, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Safety: forwarding an unhandled window message to DefWindowProcW is the required Win32 default behavior.
    unsafe { DefWindowProcW(hwnd.raw(), message, wparam, lparam) }
}

fn wparam_to_u32(wparam: WPARAM) -> eyre::Result<u32> {
    u32::try_from(wparam.0).wrap_err("WPARAM did not fit in u32")
}

fn low_word_u16(value: isize) -> u16 {
    u16::try_from(value & 0xFFFF).expect("masking to 16 bits must fit in u16")
}

fn high_word_i16(value: usize) -> i16 {
    let high_word =
        u16::try_from((value >> 16) & 0xFFFF).expect("masking to 16 bits must fit in u16");
    i16::from_le_bytes(high_word.to_le_bytes())
}
fn query_cursor_pos() -> eyre::Result<ScreenPoint> {
    let mut point = POINT::default();
    // Safety: `point` is a valid out-pointer for GetCursorPos.
    if unsafe { GetCursorPos(&raw mut point) }.is_err() {
        eyre::bail!("failed to query cursor position")
    }
    Ok(ScreenPoint::from_win32_point(point))
}

fn system_metric(metric: SYSTEM_METRICS_INDEX) -> i32 {
    // Safety: GetSystemMetrics is safe for any documented metric constant.
    unsafe { GetSystemMetrics(metric) }
}

fn control_key_is_down() -> bool {
    // Safety: querying key state for VK_CONTROL does not require additional invariants.
    let state = unsafe { GetKeyState(i32::from(VK_CONTROL.0)) };
    (state.cast_unsigned() & 0x8000) != 0
}

fn alt_key_is_down() -> bool {
    // Safety: querying key state for VK_MENU does not require additional invariants.
    let state = unsafe { GetKeyState(i32::from(VK_MENU.0)) };
    (state.cast_unsigned() & 0x8000) != 0
}

fn register_window_class(class: &WNDCLASSEXW) -> u16 {
    // Safety: `class` points to a fully initialized WNDCLASSEXW for registration.
    unsafe { RegisterClassExW(&raw const *class) }
}

fn load_cursor(cursor: PCWSTR) -> windows::Win32::UI::WindowsAndMessaging::HCURSOR {
    // Safety: loading a shared system cursor by identifier is valid.
    unsafe { LoadCursorW(None, cursor).unwrap_or_default() }
}

fn begin_paint(hwnd: WindowHandle, paint: &mut PAINTSTRUCT) -> windows::Win32::Graphics::Gdi::HDC {
    // Safety: `paint` is a valid out-pointer for BeginPaint.
    unsafe { BeginPaint(hwnd.raw(), &raw mut *paint) }
}

fn end_paint(hwnd: WindowHandle, paint: &PAINTSTRUCT) {
    // Safety: `paint` was initialized by BeginPaint for the same window.
    let _ = unsafe { EndPaint(hwnd.raw(), &raw const *paint) };
}

fn translate_message(message: &MSG) {
    // Safety: `message` was produced by GetMessageW/DispatchMessageW on this thread.
    let _ = unsafe { TranslateMessage(&raw const *message) };
}

fn dispatch_message(message: &MSG) {
    // Safety: `message` was produced by GetMessageW on this thread.
    unsafe { DispatchMessageW(&raw const *message) };
}

/// Launch the Teamy Studio terminal window and block until it closes.
/// behavior[impl window.startup.centered]
/// behavior[impl window.startup.size]
/// os[impl window.appearance.translucent]
///
/// # Errors
///
/// This function will return an error if the window class, font, terminal session, or message loop fails.
#[instrument(level = "info", skip_all, fields(has_working_dir = working_dir.is_some(), has_workspace_window = workspace_window.is_some()))]
pub fn run(
    app_home: &AppHome,
    working_dir: Option<&Path>,
    workspace_window: Option<WorkspaceWindowState>,
    vt_engine: VtEngineChoice,
) -> eyre::Result<()> {
    let window_thread = WindowThread::current();
    let terminal_font_height = TERMINAL_FONT_HEIGHT;
    let (terminal_cell_width, terminal_cell_height) = info_span!(
        "measure_terminal_cell_size",
        kind = "terminal",
        font_height = terminal_font_height,
    )
    .in_scope(|| measure_terminal_cell_size(terminal_font_height))?;
    let output_font_height = OUTPUT_FONT_HEIGHT;
    let (output_cell_width, output_cell_height) = info_span!(
        "measure_terminal_cell_size",
        kind = "output",
        font_height = output_font_height,
    )
    .in_scope(|| measure_terminal_cell_size(output_font_height))?;
    let focused_render_interval_ms = measure_focused_render_interval_ms();
    let terminal = info_span!("create_terminal_session")
        .in_scope(|| TerminalSession::new(app_home, working_dir, vt_engine))?;

    APP_STATE.with(|state| {
        *state.borrow_mut() = Some(AppState {
            app_home: app_home.clone(),
            hwnd: None,
            workspace_window,
            pending_window_drag: None,
            terminal_selection: None,
            pending_terminal_selection: None,
            terminal_selection_drag_point: None,
            terminal_scrollbar_hovered_part: None,
            terminal_scrollbar_drag: None,
            in_move_size_loop: false,
            window_focused: false,
            terminal_layout: None,
            pending_terminal_resize: None,
            terminal_poll_pending: false,
            focused_render_interval_ms,
            terminal_font_height,
            terminal_cell_width,
            terminal_cell_height,
            output_font_height,
            output_cell_width,
            output_cell_height,
            terminal,
            renderer: None,
        });
    });

    let hwnd = info_span!("create_terminal_window").in_scope(|| create_window(window_thread))?;
    let renderer = info_span!("create_d3d12_renderer_thread")
        .in_scope(|| RenderThreadProxy::new(hwnd.raw()))?;
    with_app_state(|state| {
        state.hwnd = Some(hwnd);
        state.terminal.set_wake_window(hwnd.raw());
        state.renderer = Some(renderer);
        Ok(())
    })?;
    info_span!("show_window_and_resize_terminal").in_scope(|| -> eyre::Result<()> {
        hwnd.show();

        with_app_state(|state| {
            let layout =
                client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?;
            apply_terminal_resize(state, layout)?;
            Ok(())
        })
    })?;

    with_app_state(|state| render_current_frame(state, hwnd, None))?;

    info!("Teamy Studio terminal window shown");
    message_loop()
}

#[derive(Clone, Copy, Debug)]
struct TerminalThroughputBenchmarkPlan {
    mode: TerminalThroughputBenchmarkMode,
    line_count: usize,
    resize_target_client_size: Option<(u32, u32)>,
}

#[derive(Clone, Debug)]
struct TerminalThroughputBenchmarkSampleResult {
    mode: TerminalThroughputBenchmarkMode,
    line_count: usize,
    measure_command_ms: f64,
    graphical_completion_ms: f64,
    frames_rendered: u64,
    terminal_closed: bool,
    last_screen: String,
    performance: TerminalPerformanceSnapshot,
}

impl TerminalThroughputBenchmarkSampleResult {
    fn delta_ms(&self) -> f64 {
        self.graphical_completion_ms - self.measure_command_ms
    }

    fn ratio(&self) -> f64 {
        if self.measure_command_ms > 0.0 {
            self.graphical_completion_ms / self.measure_command_ms
        } else {
            0.0
        }
    }
}

#[derive(Clone, Debug)]
struct TerminalThroughputBenchmarkScenarioResult {
    plan: TerminalThroughputBenchmarkPlan,
    sample_results: Vec<TerminalThroughputBenchmarkSampleResult>,
}

#[derive(Debug, Facet)]
struct TerminalThroughputBenchmarkResultsReport {
    generated_at_utc: String,
    app_home: String,
    scenario_count: usize,
    scenarios: Vec<TerminalThroughputBenchmarkScenarioReport>,
}

#[derive(Debug, Facet)]
struct TerminalThroughputBenchmarkScenarioReport {
    mode: String,
    line_count: usize,
    resize_target_client_size: Option<TerminalThroughputClientSizeReport>,
    summary: TerminalThroughputBenchmarkScenarioSummaryReport,
    samples: Vec<TerminalThroughputBenchmarkSampleReport>,
}

#[derive(Debug, Facet)]
struct TerminalThroughputClientSizeReport {
    width: u32,
    height: u32,
}

#[derive(Debug, Facet)]
struct TerminalThroughputBenchmarkScenarioSummaryReport {
    samples: usize,
    median_measure_command_ms: f64,
    median_graphical_completion_ms: f64,
    median_delta_ms: f64,
    median_ratio: f64,
    median_frames_rendered: u64,
    median_max_pending_output_bytes: u64,
    median_avg_pending_output_bytes: f64,
    median_max_queue_latency_ms: f64,
    median_vt_write_calls: u64,
    median_vt_write_bytes: u64,
    median_display_publications: u64,
    median_dirty_rows_published: u64,
    terminal_closed: bool,
}

#[derive(Debug, Facet)]
struct TerminalThroughputBenchmarkSampleReport {
    mode: String,
    line_count: usize,
    measure_command_ms: f64,
    graphical_completion_ms: f64,
    delta_ms: f64,
    ratio: f64,
    frames_rendered: u64,
    terminal_closed: bool,
    performance: TerminalPerformanceSnapshotReport,
    last_screen: String,
}

#[derive(Debug, Facet)]
struct TerminalPerformanceSnapshotReport {
    pending_output_bytes: usize,
    max_pending_output_bytes: usize,
    pending_output_observations: u64,
    total_pending_output_bytes: u64,
    average_pending_output_bytes: f64,
    vt_write_calls: u64,
    vt_write_bytes: u64,
    display_publications: u64,
    dirty_rows_published: u64,
    max_dirty_rows_published: usize,
    queue_latency_observations: u64,
    max_queue_latency_us: u64,
    total_queue_latency_us: u64,
    average_queue_latency_ms: f64,
    max_queue_latency_ms: f64,
    input_response_latency_observations: u64,
    max_input_response_latency_us: u64,
    total_input_response_latency_us: u64,
    average_input_response_latency_ms: f64,
    max_input_response_latency_ms: f64,
    input_present_latency_observations: u64,
    max_input_present_latency_us: u64,
    total_input_present_latency_us: u64,
    average_input_present_latency_ms: f64,
    max_input_present_latency_ms: f64,
}

impl TerminalThroughputBenchmarkScenarioResult {
    fn last_result(&self) -> eyre::Result<&TerminalThroughputBenchmarkSampleResult> {
        self.sample_results
            .last()
            .ok_or_else(|| eyre::eyre!("terminal throughput benchmark did not produce any samples"))
    }

    fn median_measure_command_ms(&self) -> f64 {
        median_sample_metric(&self.sample_results, |result| result.measure_command_ms)
    }

    fn median_graphical_completion_ms(&self) -> f64 {
        median_sample_metric(&self.sample_results, |result| {
            result.graphical_completion_ms
        })
    }

    fn median_delta_ms(&self) -> f64 {
        median_sample_metric(
            &self.sample_results,
            TerminalThroughputBenchmarkSampleResult::delta_ms,
        )
    }

    fn median_ratio(&self) -> f64 {
        median_sample_metric(
            &self.sample_results,
            TerminalThroughputBenchmarkSampleResult::ratio,
        )
    }

    fn median_frames_rendered(&self) -> u64 {
        median_sample_u64_metric(&self.sample_results, |result| result.frames_rendered)
    }

    fn median_max_pending_output_bytes(&self) -> u64 {
        median_sample_u64_metric(&self.sample_results, |result| {
            u64::try_from(result.performance.max_pending_output_bytes).unwrap_or(u64::MAX)
        })
    }

    fn median_avg_pending_output_bytes(&self) -> f64 {
        median_sample_metric(&self.sample_results, |result| {
            result.performance.average_pending_output_bytes()
        })
    }

    fn median_max_queue_latency_ms(&self) -> f64 {
        median_sample_metric(&self.sample_results, |result| {
            result.performance.max_queue_latency_ms()
        })
    }

    fn median_vt_write_calls(&self) -> u64 {
        median_sample_u64_metric(&self.sample_results, |result| {
            result.performance.vt_write_calls
        })
    }

    fn median_vt_write_bytes(&self) -> u64 {
        median_sample_u64_metric(&self.sample_results, |result| {
            result.performance.vt_write_bytes
        })
    }

    fn median_display_publications(&self) -> u64 {
        median_sample_u64_metric(&self.sample_results, |result| {
            result.performance.display_publications
        })
    }

    fn median_dirty_rows_published(&self) -> u64 {
        median_sample_u64_metric(&self.sample_results, |result| {
            result.performance.dirty_rows_published
        })
    }
}

#[instrument(level = "info", skip_all, fields(?mode, line_count, samples))]
pub fn run_terminal_throughput_self_test(
    app_home: &AppHome,
    cache_home: &CacheHome,
    mode: Option<TerminalThroughputBenchmarkMode>,
    line_count: usize,
    samples: usize,
) -> eyre::Result<()> {
    let benchmark_plans = terminal_throughput_benchmark_plans(mode, line_count);
    let mut scenario_results = Vec::with_capacity(benchmark_plans.len());

    for plan in benchmark_plans {
        let mut sample_results = Vec::with_capacity(samples);
        for sample_index in 0..samples {
            let result = run_terminal_throughput_self_test_sample(plan)?;
            if samples > 1 {
                println!(
                    "scenario: {}\nsample: {}\nmeasure_command_ms: {:.3}\ngraphical_completion_ms: {:.3}\ndelta_ms: {:.3}\nratio: {:.3}\nframes_rendered: {}\nmax_pending_output_bytes: {}\navg_pending_output_bytes: {:.3}\nmax_queue_latency_ms: {:.3}\nmax_input_response_latency_ms: {:.3}\nmax_input_present_latency_ms: {:.3}\nvt_write_calls: {}\nvt_write_bytes: {}\ndisplay_publications: {}\ndirty_rows_published: {}\nterminal_closed: {}\n",
                    terminal_throughput_mode_name(plan.mode),
                    sample_index + 1,
                    result.measure_command_ms,
                    result.graphical_completion_ms,
                    result.delta_ms(),
                    result.ratio(),
                    result.frames_rendered,
                    result.performance.max_pending_output_bytes,
                    result.performance.average_pending_output_bytes(),
                    result.performance.max_queue_latency_ms(),
                    result.performance.max_input_response_latency_ms(),
                    result.performance.max_input_present_latency_ms(),
                    result.performance.vt_write_calls,
                    result.performance.vt_write_bytes,
                    result.performance.display_publications,
                    result.performance.dirty_rows_published,
                    result.terminal_closed,
                );
            }
            sample_results.push(result);
        }

        let scenario_result = TerminalThroughputBenchmarkScenarioResult {
            plan,
            sample_results,
        };
        print_terminal_throughput_scenario_summary(&scenario_result)?;
        scenario_results.push(scenario_result);
    }

    let results_path = write_terminal_throughput_results(app_home, cache_home, &scenario_results)?;
    println!("results_path: {}", results_path.display());
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "the benchmark loop keeps terminal, window, and renderer state transitions in one place"
)]
fn run_terminal_throughput_self_test_sample(
    plan: TerminalThroughputBenchmarkPlan,
) -> eyre::Result<TerminalThroughputBenchmarkSampleResult> {
    let window_thread = WindowThread::current();
    let terminal_font_height = TERMINAL_FONT_HEIGHT;
    let (terminal_cell_width, terminal_cell_height) = info_span!(
        "measure_terminal_cell_size",
        kind = "terminal-benchmark",
        font_height = terminal_font_height,
    )
    .in_scope(|| measure_terminal_cell_size(terminal_font_height))?;
    let output_font_height = OUTPUT_FONT_HEIGHT;
    let (output_cell_width, output_cell_height) = info_span!(
        "measure_terminal_cell_size",
        kind = "output-benchmark",
        font_height = output_font_height,
    )
    .in_scope(|| measure_terminal_cell_size(output_font_height))?;
    let command = terminal_throughput_benchmark_command(plan.mode, plan.line_count)?;
    let mut terminal = info_span!("create_terminal_benchmark_session")
        .in_scope(|| TerminalSession::new_with_command(command, VtEngineChoice::Ghostty))?;
    let hwnd = info_span!("create_terminal_benchmark_window")
        .in_scope(|| create_benchmark_window(window_thread))?;
    let renderer = info_span!("create_terminal_benchmark_renderer")
        .in_scope(|| RenderThreadProxy::new(hwnd.raw()))?;

    let benchmark_result = (|| -> eyre::Result<TerminalThroughputBenchmarkSampleResult> {
        terminal.set_wake_window(hwnd.raw());
        let layout = client_layout(hwnd, terminal_cell_width, terminal_cell_height)?;
        terminal.resize(layout)?;

        let benchmark_started_at = Instant::now();
        let mut visual_started_at = None;
        let mut frames_rendered = 0_u64;
        let mut last_screen = String::new();
        let mut resize_performed = false;

        loop {
            let poll_result = terminal.poll_pty_output()?;
            let pump_result = terminal.pump_pending_output()?;
            let repaint_requested = terminal.take_repaint_requested();
            let pending_output = terminal.has_pending_output();

            let should_render =
                frames_rendered == 0 || poll_result.queued_output || repaint_requested;
            if should_render {
                if visual_started_at.is_none() && (poll_result.queued_output || pending_output) {
                    visual_started_at = Some(Instant::now());
                }
                render_terminal_throughput_benchmark_frame(
                    hwnd,
                    &renderer,
                    &mut terminal,
                    terminal_cell_width,
                    terminal_cell_height,
                    output_cell_width,
                    output_cell_height,
                    plan.mode,
                    plan.line_count,
                )?;
                frames_rendered += 1;
            }

            if !resize_performed
                && let Some((client_width, client_height)) = plan.resize_target_client_size
                && visual_started_at.is_some()
                && frames_rendered >= 3
            {
                resize_benchmark_window_client(
                    hwnd,
                    i32::try_from(client_width).unwrap_or(i32::MAX),
                    i32::try_from(client_height).unwrap_or(i32::MAX),
                )?;
                let layout = client_layout(hwnd, terminal_cell_width, terminal_cell_height)?;
                renderer.resize(client_width, client_height)?;
                terminal.resize(layout)?;
                resize_performed = true;
            }

            let terminal_closed = pump_result.should_close || poll_result.should_close;
            let pending_output_after_render = terminal.has_pending_output();

            if let Some(visual_started_at) = visual_started_at
                && terminal_closed
                && !pending_output_after_render
            {
                last_screen = terminal.visible_text()?;
                let measure_command_ms =
                    parse_terminal_throughput_measure_command_ms(&last_screen)?;
                let graphical_completion_ms = visual_started_at.elapsed().as_secs_f64() * 1000.0;
                let performance = terminal.performance_snapshot()?;
                return Ok(TerminalThroughputBenchmarkSampleResult {
                    mode: plan.mode,
                    line_count: plan.line_count,
                    measure_command_ms,
                    graphical_completion_ms,
                    frames_rendered,
                    terminal_closed,
                    last_screen,
                    performance,
                });
            }

            if benchmark_started_at.elapsed() >= TERMINAL_THROUGHPUT_BENCHMARK_TIMEOUT {
                if last_screen.is_empty() {
                    last_screen = terminal.visible_text().unwrap_or_default();
                }
                eyre::bail!(
                    "timed out waiting for terminal throughput benchmark completion\n\n=== final_screen ===\n{last_screen}"
                );
            }

            thread::sleep(TERMINAL_THROUGHPUT_BENCHMARK_POLL_INTERVAL);
        }
    })();

    drop(renderer);
    hwnd.destroy();
    benchmark_result
}

fn median_sample_metric<T>(samples: &[TerminalThroughputBenchmarkSampleResult], selector: T) -> f64
where
    T: Fn(&TerminalThroughputBenchmarkSampleResult) -> f64,
{
    let mut values = samples.iter().map(selector).collect::<Vec<_>>();
    values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        f64::midpoint(values[mid - 1], values[mid])
    } else {
        values[mid]
    }
}

fn median_sample_u64_metric<T>(
    samples: &[TerminalThroughputBenchmarkSampleResult],
    selector: T,
) -> u64
where
    T: Fn(&TerminalThroughputBenchmarkSampleResult) -> u64,
{
    let mut values = samples.iter().map(selector).collect::<Vec<_>>();
    values.sort_unstable();
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        u64::midpoint(values[mid - 1], values[mid])
    } else {
        values[mid]
    }
}

/// os[impl window.appearance.os-chrome-none]
#[instrument(level = "info", skip_all)]
fn create_window(window_thread: WindowThread) -> eyre::Result<WindowHandle> {
    let instance = get_current_module().wrap_err("failed to get module handle")?;

    let class = WNDCLASSEXW {
        cbSize: u32::try_from(std::mem::size_of::<WNDCLASSEXW>())
            .expect("WNDCLASSEXW size must fit in u32"),
        hInstance: instance.into(),
        lpszClassName: WINDOW_CLASS_NAME,
        lpfnWndProc: Some(window_proc),
        hCursor: load_cursor(IDC_ARROW),
        ..Default::default()
    };
    let atom = register_window_class(&class);
    if atom == 0 {
        debug!(
            "terminal window class already registered or registration deferred to create-window path"
        );
    }

    let screen_width = system_metric(SM_CXSCREEN);
    let screen_height = system_metric(SM_CYSCREEN);
    let x = (screen_width - INITIAL_WINDOW_WIDTH) / 2;
    let y = (screen_height - INITIAL_WINDOW_HEIGHT) / 2;
    let title = wide_null_terminated(WINDOW_TITLE);

    // Safety: all pointers and handles passed to CreateWindowExW are valid for the duration of the call.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW,
            WINDOW_CLASS_NAME,
            PCWSTR(title.as_ptr()),
            WS_POPUP | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_VISIBLE,
            x,
            y,
            INITIAL_WINDOW_WIDTH,
            INITIAL_WINDOW_HEIGHT,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
    .wrap_err("failed to create terminal window")?;

    let window = WindowHandle::new(window_thread, hwnd);
    window.set_poll_timer()?;
    Ok(window)
}

#[instrument(level = "info", skip_all)]
fn create_benchmark_window(window_thread: WindowThread) -> eyre::Result<WindowHandle> {
    let instance = get_current_module().wrap_err("failed to get module handle")?;

    let class = WNDCLASSEXW {
        cbSize: u32::try_from(std::mem::size_of::<WNDCLASSEXW>())
            .expect("WNDCLASSEXW size must fit in u32"),
        hInstance: instance.into(),
        lpszClassName: BENCHMARK_WINDOW_CLASS_NAME,
        lpfnWndProc: Some(benchmark_window_proc),
        hCursor: load_cursor(IDC_ARROW),
        ..Default::default()
    };
    let atom = register_window_class(&class);
    if atom == 0 {
        debug!("benchmark window class already registered or registration deferred");
    }

    let title = wide_null_terminated(WINDOW_TITLE);
    // Safety: all pointers and handles passed to CreateWindowExW are valid for the duration of the call.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW,
            BENCHMARK_WINDOW_CLASS_NAME,
            PCWSTR(title.as_ptr()),
            WS_POPUP | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX,
            0,
            0,
            INITIAL_WINDOW_WIDTH,
            INITIAL_WINDOW_HEIGHT,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
    .wrap_err("failed to create terminal benchmark window")?;

    Ok(WindowHandle::new(window_thread, hwnd))
}

fn message_loop() -> eyre::Result<()> {
    loop {
        let mut message = MSG::default();
        let status = {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("wait_for_window_message").entered();
            // Safety: `message` is a valid out-pointer for GetMessageW on this UI thread.
            unsafe { GetMessageW(&raw mut message, None, 0, 0) }
        };
        if status.0 == -1 {
            eyre::bail!("failed to get next window message")
        }
        if status.0 == 0 {
            return Ok(());
        }

        translate_message(&message);
        dispatch_message(&message);
    }
}

extern "system" fn benchmark_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let hwnd = WindowHandle::new(WindowThread::current(), hwnd);
    match message {
        WM_NCCALCSIZE => LRESULT(0),
        WM_ERASEBKGND => LRESULT(1),
        _ => def_window_proc(hwnd, message, wparam, lparam),
    }
}

extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let hwnd = WindowHandle::new(WindowThread::current(), hwnd);
    match message {
        WM_NCCALCSIZE => LRESULT(0),
        WM_SETFOCUS => handle_focus_changed(hwnd, true),
        WM_KILLFOCUS => handle_focus_changed(hwnd, false),
        WM_ENTERSIZEMOVE => handle_enter_size_move(hwnd),
        WM_EXITSIZEMOVE => handle_exit_size_move(hwnd),
        WM_SIZE => handle_size(hwnd),
        TERMINAL_WORKER_WAKE_MESSAGE => handle_terminal_worker_wake(hwnd),
        WM_TIMER if wparam.0 == POLL_TIMER_ID => handle_timer(hwnd),
        WM_TIMER if wparam.0 == FOCUSED_RENDER_TIMER_ID => handle_focused_render_timer(hwnd),
        WM_CHAR => handle_char_message(hwnd, message, wparam, lparam),
        WM_KEYDOWN | WM_SYSKEYDOWN => handle_key_down_message(hwnd, message, wparam, lparam),
        WM_KEYUP | WM_SYSKEYUP => handle_key_up_message(hwnd, message, wparam, lparam),
        WM_LBUTTONDOWN => handle_bool_message(hwnd, message, wparam, lparam, |hwnd| {
            handle_left_button_down(hwnd, lparam)
        }),
        WM_MOUSEMOVE => handle_bool_message(hwnd, message, wparam, lparam, |hwnd| {
            handle_mouse_move(hwnd, wparam, lparam)
        }),
        WM_PAINT => match acknowledge_paint(hwnd) {
            Ok(()) => LRESULT(0),
            Err(error) => fail_and_close(hwnd, &error),
        },
        WM_LBUTTONUP => handle_bool_message(hwnd, message, wparam, lparam, |hwnd| {
            handle_left_button_up(hwnd, lparam)
        }),
        WM_RBUTTONUP => handle_bool_message(hwnd, message, wparam, lparam, |hwnd| {
            handle_right_button_up(hwnd, lparam)
        }),
        WM_MOUSEWHEEL => handle_bool_message(hwnd, message, wparam, lparam, |hwnd| {
            handle_mouse_wheel(hwnd, wparam, lparam)
        }),
        WM_SETCURSOR => match handle_set_cursor(hwnd, lparam) {
            Ok(true) => LRESULT(1),
            Ok(false) => def_window_proc(hwnd, message, wparam, lparam),
            Err(error) => fail_and_close(hwnd, &error),
        },
        WM_NCHITTEST => handle_non_client_hit_test(hwnd, lparam),
        WM_ERASEBKGND => LRESULT(1),
        WM_DESTROY => handle_destroy_message(hwnd),
        _ => def_window_proc(hwnd, message, wparam, lparam),
    }
}

fn handle_enter_size_move(hwnd: WindowHandle) -> LRESULT {
    match with_app_state(|state| {
        state.in_move_size_loop = true;
        render_current_frame(state, hwnd, None)?;
        Ok(())
    }) {
        Ok(()) => LRESULT(0),
        Err(error) => fail_and_close(hwnd, &error),
    }
}

fn handle_exit_size_move(hwnd: WindowHandle) -> LRESULT {
    match with_app_state(|state| {
        state.in_move_size_loop = false;
        apply_pending_terminal_resize(state)?;
        render_current_frame(state, hwnd, None)?;
        Ok(())
    }) {
        Ok(()) => LRESULT(0),
        Err(error) => fail_and_close(hwnd, &error),
    }
}

fn handle_size(hwnd: WindowHandle) -> LRESULT {
    match with_app_state(|state| {
        let layout = client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?;
        if state.in_move_size_loop {
            state.pending_terminal_resize = Some(layout);
        } else {
            state.pending_terminal_resize = None;
            apply_terminal_resize(state, layout)?;
        }
        render_current_frame(
            state,
            hwnd,
            Some((
                layout.client_width.cast_unsigned(),
                layout.client_height.cast_unsigned(),
            )),
        )?;
        Ok(())
    }) {
        Ok(()) => LRESULT(0),
        Err(error) => fail_and_close(hwnd, &error),
    }
}

fn handle_timer(hwnd: WindowHandle) -> LRESULT {
    match handle_poll_timer(hwnd) {
        Ok(should_close) => {
            if should_close {
                hwnd.destroy();
            }
            LRESULT(0)
        }
        Err(error) => fail_and_close(hwnd, &error),
    }
}

fn handle_focused_render_timer(hwnd: WindowHandle) -> LRESULT {
    match with_app_state(|state| {
        if !state.window_focused {
            return Ok(());
        }

        let () = {
            #[cfg(feature = "tracy")]
            let _span = debug_span!("render_focused_animation_frame").entered();
            render_current_frame_with_options(state, hwnd, None, true)?;
        };
        Ok(())
    }) {
        Ok(()) => LRESULT(0),
        Err(error) => fail_and_close(hwnd, &error),
    }
}

fn handle_focus_changed(hwnd: WindowHandle, focused: bool) -> LRESULT {
    match with_app_state(|state| {
        state.window_focused = focused;
        if focused {
            hwnd.set_focused_render_timer(state.focused_render_interval_ms)?;
            render_current_frame_with_options(state, hwnd, None, true)?;
        } else {
            hwnd.clear_focused_render_timer();
        }
        Ok(())
    }) {
        Ok(()) => LRESULT(0),
        Err(error) => fail_and_close(hwnd, &error),
    }
}

fn handle_terminal_worker_wake(hwnd: WindowHandle) -> LRESULT {
    match with_app_state(|state| {
        let repaint_requested = state.terminal.take_repaint_requested();
        if state.terminal.pump()?.should_close {
            hwnd.post_close();
            return Ok(());
        }

        if repaint_requested {
            render_current_frame(state, hwnd, None)?;
        }

        Ok(())
    }) {
        Ok(()) => LRESULT(0),
        Err(error) => fail_and_close(hwnd, &error),
    }
}

/// behavior[impl window.interaction.input]
fn handle_char_message(
    hwnd: WindowHandle,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let code_unit = match wparam_to_u32(wparam) {
        Ok(code_unit) => code_unit,
        Err(error) => return fail_and_close(hwnd, &error),
    };

    match with_app_state(|state| state.terminal.handle_char(code_unit, lparam.0)) {
        Ok(consumed) => {
            trace!(
                message = "WM_CHAR",
                code_unit,
                lparam = lparam.0,
                consumed,
                "processed keyboard char message"
            );
            if consumed {
                LRESULT(0)
            } else {
                def_window_proc(hwnd, message, wparam, lparam)
            }
        }
        Err(error) => fail_and_close(hwnd, &error),
    }
}

/// behavior[impl window.interaction.input]
fn handle_key_down_message(
    hwnd: WindowHandle,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let virtual_key = match wparam_to_u32(wparam) {
        Ok(virtual_key) => virtual_key,
        Err(error) => return fail_and_close(hwnd, &error),
    };
    let was_down = ((lparam.0 >> 30) & 1) != 0;

    match with_app_state(|state| {
        state.terminal.handle_key_event(
            virtual_key,
            lparam.0,
            was_down,
            false,
            keyboard_mods(virtual_key, lparam.0, false),
        )
    }) {
        Ok(consumed) => {
            trace!(
                message = if message == WM_SYSKEYDOWN {
                    "WM_SYSKEYDOWN"
                } else {
                    "WM_KEYDOWN"
                },
                vkey = virtual_key,
                lparam = lparam.0,
                was_down,
                consumed,
                "processed keyboard down message"
            );
            if consumed {
                LRESULT(0)
            } else {
                def_window_proc(hwnd, message, wparam, lparam)
            }
        }
        Err(error) => fail_and_close(hwnd, &error),
    }
}

fn handle_key_up_message(
    hwnd: WindowHandle,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let virtual_key = match wparam_to_u32(wparam) {
        Ok(virtual_key) => virtual_key,
        Err(error) => return fail_and_close(hwnd, &error),
    };

    match with_app_state(|state| {
        state.terminal.handle_key_event(
            virtual_key,
            lparam.0,
            false,
            true,
            keyboard_mods(virtual_key, lparam.0, true),
        )
    }) {
        Ok(consumed) => {
            trace!(
                message = if message == WM_SYSKEYUP {
                    "WM_SYSKEYUP"
                } else {
                    "WM_KEYUP"
                },
                vkey = virtual_key,
                lparam = lparam.0,
                consumed,
                "processed keyboard up message"
            );
            if consumed {
                LRESULT(0)
            } else {
                def_window_proc(hwnd, message, wparam, lparam)
            }
        }
        Err(error) => fail_and_close(hwnd, &error),
    }
}

fn handle_bool_message(
    hwnd: WindowHandle,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    handler: impl FnOnce(WindowHandle) -> eyre::Result<bool>,
) -> LRESULT {
    match handler(hwnd) {
        Ok(true) => LRESULT(0),
        Ok(false) => def_window_proc(hwnd, message, wparam, lparam),
        Err(error) => fail_and_close(hwnd, &error),
    }
}

/// os[impl window.interaction.resize.native-edges]
fn handle_non_client_hit_test(hwnd: WindowHandle, lparam: LPARAM) -> LRESULT {
    let point = match screen_to_client_point(hwnd, lparam) {
        Ok(point) => point,
        Err(error) => return fail_and_close(hwnd, &error),
    };
    match hit_test_resize_border(hwnd, point) {
        Ok(Some(hit)) => hit,
        Ok(None) => LRESULT(isize::try_from(HTCLIENT).expect("HTCLIENT fits in isize")),
        Err(error) => fail_and_close(hwnd, &error),
    }
}

fn handle_destroy_message(hwnd: WindowHandle) -> LRESULT {
    APP_STATE.with(|state| {
        let _ = state.borrow_mut().take();
    });
    hwnd.post_quit_message();
    LRESULT(0)
}

fn acknowledge_paint(hwnd: WindowHandle) -> eyre::Result<()> {
    let mut paint = PAINTSTRUCT::default();
    let hdc = begin_paint(hwnd, &mut paint);
    if hdc.0.is_null() {
        eyre::bail!("failed to begin painting")
    }

    end_paint(hwnd, &paint);
    Ok(())
}

/// behavior[impl window.interaction.drag.live]
/// behavior[impl window.interaction.resize.live]
/// behavior[impl window.interaction.resize.terminal-live-output]
/// behavior[impl window.interaction.resize.low-latency]
#[cfg_attr(feature = "tracy", instrument(level = "debug", skip_all))]
fn handle_poll_timer(hwnd: WindowHandle) -> eyre::Result<bool> {
    with_app_state(|state| {
        let poll_result = state.terminal.poll_pty_output()?;
        state.terminal_poll_pending |=
            poll_result.queued_output || state.terminal.has_pending_output();

        if poll_result.should_close {
            hwnd.post_close();
            return Ok(false);
        }

        let selection_scrolled = auto_scroll_pending_terminal_selection(state, hwnd)?;

        if should_render_from_poll_timer(state.in_move_size_loop) || selection_scrolled {
            render_current_frame(state, hwnd, None)?;
        }

        Ok(false)
    })
}

#[cfg_attr(feature = "tracy", instrument(level = "debug", skip_all))]
fn render_current_frame(
    state: &mut AppState,
    hwnd: WindowHandle,
    resize: Option<(u32, u32)>,
) -> eyre::Result<()> {
    render_current_frame_with_options(state, hwnd, resize, false)
}

fn render_current_frame_with_options(
    state: &mut AppState,
    hwnd: WindowHandle,
    resize: Option<(u32, u32)>,
    force_redraw: bool,
) -> eyre::Result<()> {
    if let Some((width, height)) = resize
        && let Some(renderer) = state.renderer.as_mut()
    {
        renderer.resize(width, height)?;
    }

    let layout = {
        #[cfg(feature = "tracy")]
        let _span = debug_span!("compute_client_layout").entered();
        client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?
    };
    let output_text = {
        #[cfg(feature = "tracy")]
        let _span = debug_span!("build_output_panel_text").entered();
        build_output_panel_text(state, layout)
    };
    let terminal_display = {
        #[cfg(feature = "tracy")]
        let _span = debug_span!("build_terminal_display_state").entered();
        let display = if let Some(selection) = state.terminal_selection {
            Arc::new(
                state
                    .terminal
                    .visible_display_state_with_selection(Some(selection))?,
            )
        } else {
            state.terminal.cached_display_state()
        };
        clip_terminal_display_to_layout(
            display,
            layout,
            state.terminal_cell_width,
            state.terminal_cell_height,
        )
    };
    let terminal_visual_state = terminal_scrollbar_visual_state(state);

    let Some(renderer) = state.renderer.as_mut() else {
        return Ok(());
    };
    let () = {
        #[cfg(feature = "tracy")]
        let _span = debug_span!("submit_render_frame_model").entered();
        let frame = RenderFrameModel {
            layout,
            cell_number: state
                .workspace_window
                .as_ref()
                .map_or(1, |workspace_window| workspace_window.cell_number),
            output_text,
            output_cell_width: state.output_cell_width,
            output_cell_height: state.output_cell_height,
            terminal_cell_width: state.terminal_cell_width,
            terminal_cell_height: state.terminal_cell_height,
            terminal_display,
            terminal_visual_state: RendererTerminalVisualState {
                track_hovered: terminal_visual_state.track_hovered,
                thumb_hovered: terminal_visual_state.thumb_hovered,
                thumb_grabbed: terminal_visual_state.thumb_grabbed,
            },
        };
        if force_redraw {
            renderer.render_frame_model_force_redraw(frame)?;
        } else {
            renderer.render_frame_model(frame)?;
        }
    };
    state.terminal.note_frame_presented();
    Ok(())
}

fn measure_focused_render_interval_ms() -> u32 {
    // Safety: querying the screen DC with a null HWND is valid.
    let hdc = unsafe { GetDC(None) };
    if hdc.0.is_null() {
        return 16;
    }

    // Safety: querying a device capability from a live screen DC is valid.
    let refresh_hz = unsafe { GetDeviceCaps(Some(hdc), VREFRESH) };
    // Safety: releasing the screen DC after GetDC(None) is required.
    unsafe { ReleaseDC(None, hdc) };

    let refresh_hz = u32::try_from(refresh_hz).unwrap_or(60);
    if refresh_hz <= 1 {
        return 16;
    }

    (1_000 / refresh_hz).max(1)
}

fn terminal_scrollbar_visual_state(state: &AppState) -> TerminalScrollbarVisualState {
    let thumb_grabbed = state.terminal_scrollbar_drag.is_some();
    let hovered_part = if thumb_grabbed {
        Some(TerminalScrollbarPart::Thumb)
    } else {
        state.terminal_scrollbar_hovered_part
    };

    TerminalScrollbarVisualState {
        track_hovered: hovered_part.is_some(),
        thumb_hovered: matches!(hovered_part, Some(TerminalScrollbarPart::Thumb)),
        thumb_grabbed,
    }
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

fn terminal_scrollbar_hit_test(
    scrollbar_rect: ClientRect,
    scrollbar: TerminalDisplayScrollbar,
    point: ClientPoint,
) -> Option<TerminalScrollbarPart> {
    if !scrollbar_rect.contains(point) {
        return None;
    }

    let geometry = terminal_scrollbar_geometry(scrollbar_rect, scrollbar)?;
    Some(if geometry.thumb_rect.contains(point) {
        TerminalScrollbarPart::Thumb
    } else {
        TerminalScrollbarPart::Track
    })
}

fn terminal_scrollbar_offset_for_pointer(
    scrollbar_rect: ClientRect,
    geometry: TerminalScrollbarGeometry,
    point: ClientPoint,
    grab_offset_y: i32,
) -> eyre::Result<u64> {
    if geometry.travel <= 0 || geometry.max_offset == 0 {
        return Ok(0);
    }

    let y = point.to_win32_point()?.y;
    let thumb_top = (y - scrollbar_rect.top() - grab_offset_y).clamp(0, geometry.travel);
    let thumb_top = u64::try_from(thumb_top).unwrap_or_default();
    let travel = u64::try_from(geometry.travel).unwrap_or(1);
    Ok((thumb_top.saturating_mul(geometry.max_offset) + (travel / 2)) / travel)
}

fn current_terminal_scrollbar(state: &AppState) -> eyre::Result<Option<TerminalDisplayScrollbar>> {
    let viewport = state.terminal.viewport_metrics()?;
    if viewport.total == 0 || viewport.visible == 0 {
        return Ok(None);
    }

    Ok(Some(TerminalDisplayScrollbar {
        total: viewport.total,
        offset: viewport.offset,
        visible: viewport.visible,
    }))
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "cursor overlay helper now lives on the render thread and this copy remains for focused tests"
    )
)]
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

fn terminal_render_rect(layout: TerminalLayout) -> ClientRect {
    layout.terminal_content_rect()
}

fn terminal_cell_from_client_point(
    layout: TerminalLayout,
    point: ClientPoint,
    clamp_to_viewport: bool,
) -> Option<TerminalCellPoint> {
    let terminal_rect = terminal_render_rect(layout);
    if terminal_rect.width() <= 0 || terminal_rect.height() <= 0 {
        return None;
    }

    if !terminal_rect.contains(point) && !clamp_to_viewport {
        return None;
    }

    let point = point.to_win32_point().ok()?;
    let x = point.x;
    let y = point.y;
    let clamped_x = x.clamp(terminal_rect.left(), terminal_rect.right() - 1);
    let clamped_y = y.clamp(terminal_rect.top(), terminal_rect.bottom() - 1);
    let relative_x = clamped_x - terminal_rect.left();
    let relative_y = clamped_y - terminal_rect.top();

    let (visible_cols, visible_rows) = layout.visible_grid_size();
    let grid_cols = visible_cols.max(1);
    let grid_rows = visible_rows.max(1);
    let column = (relative_x / layout.cell_width.max(1)).clamp(0, grid_cols - 1);
    let row = (relative_y / layout.cell_height.max(1)).clamp(0, grid_rows - 1);
    Some(TerminalCellPoint::new(column, row))
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "cursor overlay helper now lives on the render thread and this copy remains for focused tests"
    )
)]
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

fn build_output_panel_text(state: &AppState, layout: TerminalLayout) -> String {
    let (cols, rows) = effective_terminal_grid_size(layout);
    if let Some(workspace_window) = &state.workspace_window {
        format!(
            "workspace {}\ncell {} of {}\n{} cols x {} rows",
            workspace_window.workspace.name,
            workspace_window.cell_number,
            workspace_window.workspace.cell_count,
            cols,
            rows
        )
    } else {
        format!("standalone shell\n{cols} cols x {rows} rows")
    }
}

fn build_terminal_throughput_benchmark_output_panel_text(
    terminal: &TerminalSession,
    mode: TerminalThroughputBenchmarkMode,
    line_count: usize,
) -> String {
    format!(
        "self-test {}\n{} lines\n{} cols x {} rows",
        terminal_throughput_mode_name(mode),
        line_count,
        terminal.cols(),
        terminal.rows()
    )
}

fn terminal_throughput_benchmark_plans(
    mode: Option<TerminalThroughputBenchmarkMode>,
    line_count: usize,
) -> Vec<TerminalThroughputBenchmarkPlan> {
    let build_plan = |mode| TerminalThroughputBenchmarkPlan {
        mode,
        line_count,
        resize_target_client_size: match mode {
            TerminalThroughputBenchmarkMode::ResizeDuringOutput => Some((820, 520)),
            _ => None,
        },
    };

    if let Some(mode) = mode {
        vec![build_plan(mode)]
    } else {
        vec![
            build_plan(TerminalThroughputBenchmarkMode::MeasureCommandOutHost),
            build_plan(TerminalThroughputBenchmarkMode::StreamSmallBatches),
            build_plan(TerminalThroughputBenchmarkMode::WideLines),
            build_plan(TerminalThroughputBenchmarkMode::ScrollFlood),
            build_plan(TerminalThroughputBenchmarkMode::PromptBursts),
            build_plan(TerminalThroughputBenchmarkMode::ResizeDuringOutput),
        ]
    }
}

fn print_terminal_throughput_scenario_summary(
    scenario_result: &TerminalThroughputBenchmarkScenarioResult,
) -> eyre::Result<()> {
    let last_result = scenario_result.last_result()?;
    println!(
        "scenario: {}\nline_count: {}\nsamples: {}\nmeasure_command_ms: {:.3}\ngraphical_completion_ms: {:.3}\ndelta_ms: {:.3}\nratio: {:.3}\nframes_rendered: {}\nmax_pending_output_bytes: {}\navg_pending_output_bytes: {:.3}\nmax_queue_latency_ms: {:.3}\nmax_input_response_latency_ms: {:.3}\nmax_input_present_latency_ms: {:.3}\nvt_write_calls: {}\nvt_write_bytes: {}\ndisplay_publications: {}\ndirty_rows_published: {}\nterminal_closed: {}\n\n=== final_screen ===\n{}",
        terminal_throughput_mode_name(scenario_result.plan.mode),
        scenario_result.plan.line_count,
        scenario_result.sample_results.len(),
        scenario_result.median_measure_command_ms(),
        scenario_result.median_graphical_completion_ms(),
        scenario_result.median_delta_ms(),
        scenario_result.median_ratio(),
        scenario_result.median_frames_rendered(),
        scenario_result.median_max_pending_output_bytes(),
        scenario_result.median_avg_pending_output_bytes(),
        scenario_result.median_max_queue_latency_ms(),
        median_sample_metric(&scenario_result.sample_results, |result| {
            result.performance.max_input_response_latency_ms()
        }),
        median_sample_metric(&scenario_result.sample_results, |result| {
            result.performance.max_input_present_latency_ms()
        }),
        scenario_result.median_vt_write_calls(),
        scenario_result.median_vt_write_bytes(),
        scenario_result.median_display_publications(),
        scenario_result.median_dirty_rows_published(),
        last_result.terminal_closed,
        last_result.last_screen,
    );
    Ok(())
}

fn write_terminal_throughput_results(
    app_home: &AppHome,
    cache_home: &CacheHome,
    scenario_results: &[TerminalThroughputBenchmarkScenarioResult],
) -> eyre::Result<std::path::PathBuf> {
    let results_dir = cache_home.join(TERMINAL_THROUGHPUT_RESULTS_DIR);
    std::fs::create_dir_all(&results_dir).wrap_err_with(|| {
        format!(
            "failed to create terminal throughput results directory {}",
            results_dir.display()
        )
    })?;

    let timestamp = Utc::now();
    let results_path = results_dir.join(format!(
        "terminal-throughput-{}.json",
        timestamp.format("%Y%m%dT%H%M%SZ")
    ));
    let report = build_terminal_throughput_results_report(app_home, timestamp, scenario_results);
    let json = facet_json::to_string_pretty(&report)
        .wrap_err("failed to serialize terminal throughput benchmark results")?;
    std::fs::write(&results_path, json)
        .wrap_err_with(|| format!("failed to write {}", results_path.display()))?;
    Ok(results_path)
}

fn build_terminal_throughput_results_report(
    app_home: &AppHome,
    generated_at: chrono::DateTime<Utc>,
    scenario_results: &[TerminalThroughputBenchmarkScenarioResult],
) -> TerminalThroughputBenchmarkResultsReport {
    TerminalThroughputBenchmarkResultsReport {
        generated_at_utc: generated_at.to_rfc3339(),
        app_home: app_home.display().to_string(),
        scenario_count: scenario_results.len(),
        scenarios: scenario_results
            .iter()
            .map(terminal_throughput_scenario_report)
            .collect(),
    }
}

fn terminal_throughput_scenario_report(
    scenario_result: &TerminalThroughputBenchmarkScenarioResult,
) -> TerminalThroughputBenchmarkScenarioReport {
    let last_result = scenario_result
        .last_result()
        .expect("scenario results should contain at least one sample");
    TerminalThroughputBenchmarkScenarioReport {
        mode: terminal_throughput_mode_name(scenario_result.plan.mode).to_owned(),
        line_count: scenario_result.plan.line_count,
        resize_target_client_size: scenario_result
            .plan
            .resize_target_client_size
            .map(|(width, height)| TerminalThroughputClientSizeReport { width, height }),
        summary: TerminalThroughputBenchmarkScenarioSummaryReport {
            samples: scenario_result.sample_results.len(),
            median_measure_command_ms: scenario_result.median_measure_command_ms(),
            median_graphical_completion_ms: scenario_result.median_graphical_completion_ms(),
            median_delta_ms: scenario_result.median_delta_ms(),
            median_ratio: scenario_result.median_ratio(),
            median_frames_rendered: scenario_result.median_frames_rendered(),
            median_max_pending_output_bytes: scenario_result.median_max_pending_output_bytes(),
            median_avg_pending_output_bytes: scenario_result.median_avg_pending_output_bytes(),
            median_max_queue_latency_ms: scenario_result.median_max_queue_latency_ms(),
            median_vt_write_calls: scenario_result.median_vt_write_calls(),
            median_vt_write_bytes: scenario_result.median_vt_write_bytes(),
            median_display_publications: scenario_result.median_display_publications(),
            median_dirty_rows_published: scenario_result.median_dirty_rows_published(),
            terminal_closed: last_result.terminal_closed,
        },
        samples: scenario_result
            .sample_results
            .iter()
            .map(terminal_throughput_sample_report)
            .collect(),
    }
}

fn terminal_throughput_sample_report(
    sample_result: &TerminalThroughputBenchmarkSampleResult,
) -> TerminalThroughputBenchmarkSampleReport {
    TerminalThroughputBenchmarkSampleReport {
        mode: terminal_throughput_mode_name(sample_result.mode).to_owned(),
        line_count: sample_result.line_count,
        measure_command_ms: sample_result.measure_command_ms,
        graphical_completion_ms: sample_result.graphical_completion_ms,
        delta_ms: sample_result.delta_ms(),
        ratio: sample_result.ratio(),
        frames_rendered: sample_result.frames_rendered,
        terminal_closed: sample_result.terminal_closed,
        performance: TerminalPerformanceSnapshotReport {
            pending_output_bytes: sample_result.performance.pending_output_bytes,
            max_pending_output_bytes: sample_result.performance.max_pending_output_bytes,
            pending_output_observations: sample_result.performance.pending_output_observations,
            total_pending_output_bytes: sample_result.performance.total_pending_output_bytes,
            average_pending_output_bytes: sample_result.performance.average_pending_output_bytes(),
            vt_write_calls: sample_result.performance.vt_write_calls,
            vt_write_bytes: sample_result.performance.vt_write_bytes,
            display_publications: sample_result.performance.display_publications,
            dirty_rows_published: sample_result.performance.dirty_rows_published,
            max_dirty_rows_published: sample_result.performance.max_dirty_rows_published,
            queue_latency_observations: sample_result.performance.queue_latency_observations,
            max_queue_latency_us: sample_result.performance.max_queue_latency_us,
            total_queue_latency_us: sample_result.performance.total_queue_latency_us,
            average_queue_latency_ms: sample_result.performance.average_queue_latency_ms(),
            max_queue_latency_ms: sample_result.performance.max_queue_latency_ms(),
            input_response_latency_observations: sample_result
                .performance
                .input_response_latency_observations,
            max_input_response_latency_us: sample_result.performance.max_input_response_latency_us,
            total_input_response_latency_us: sample_result
                .performance
                .total_input_response_latency_us,
            average_input_response_latency_ms: sample_result
                .performance
                .average_input_response_latency_ms(),
            max_input_response_latency_ms: sample_result
                .performance
                .max_input_response_latency_ms(),
            input_present_latency_observations: sample_result
                .performance
                .input_present_latency_observations,
            max_input_present_latency_us: sample_result.performance.max_input_present_latency_us,
            total_input_present_latency_us: sample_result
                .performance
                .total_input_present_latency_us,
            average_input_present_latency_ms: sample_result
                .performance
                .average_input_present_latency_ms(),
            max_input_present_latency_ms: sample_result.performance.max_input_present_latency_ms(),
        },
        last_screen: sample_result.last_screen.clone(),
    }
}

fn resize_benchmark_window_client(
    hwnd: WindowHandle,
    client_width: i32,
    client_height: i32,
) -> eyre::Result<()> {
    let window_rect = hwnd.window_rect()?;
    let client_rect = hwnd.client_rect()?;
    let frame_width = window_rect.width() - client_rect.width();
    let frame_height = window_rect.height() - client_rect.height();
    let outer_width = client_width + frame_width;
    let outer_height = client_height + frame_height;

    // Safety: the benchmark window is live on the current thread and the computed outer bounds preserve its position.
    if unsafe {
        MoveWindow(
            hwnd.raw(),
            window_rect.left(),
            window_rect.top(),
            outer_width,
            outer_height,
            true,
        )
    }
    .is_err()
    {
        eyre::bail!("failed to resize benchmark window")
    }

    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "keeps the benchmark frame builder explicit while mirroring the render path inputs"
)]
fn render_terminal_throughput_benchmark_frame(
    hwnd: WindowHandle,
    renderer: &RenderThreadProxy,
    terminal: &mut TerminalSession,
    terminal_cell_width: i32,
    terminal_cell_height: i32,
    output_cell_width: i32,
    output_cell_height: i32,
    mode: TerminalThroughputBenchmarkMode,
    line_count: usize,
) -> eyre::Result<()> {
    let layout = client_layout(hwnd, terminal_cell_width, terminal_cell_height)?;
    let output_text =
        build_terminal_throughput_benchmark_output_panel_text(terminal, mode, line_count);
    let terminal_display = terminal.cached_display_state();

    renderer.render_frame_model_blocking(RenderFrameModel {
        layout,
        cell_number: 1,
        output_text,
        output_cell_width,
        output_cell_height,
        terminal_cell_width,
        terminal_cell_height,
        terminal_display,
        terminal_visual_state: RendererTerminalVisualState::default(),
    })
}

#[expect(
    clippy::too_many_lines,
    reason = "the benchmark script table stays easier to compare when each scenario script is inline"
)]
fn terminal_throughput_benchmark_command(
    mode: TerminalThroughputBenchmarkMode,
    line_count: usize,
) -> eyre::Result<portable_pty::CommandBuilder> {
    let script = match mode {
        TerminalThroughputBenchmarkMode::MeasureCommandOutHost => format!(
            concat!(
                "$ErrorActionPreference = 'Stop'\n",
                "Write-Host '{start_marker}'\n",
                "Start-Sleep -Milliseconds 100\n",
                "$duration = Measure-Command {{ 1..{line_count} | Out-Host }}\n",
                "$measureMs = [string]::Format([System.Globalization.CultureInfo]::InvariantCulture, '{{0:F3}}', $duration.TotalMilliseconds)\n",
                "Write-Host ('{measure_prefix}' + $measureMs)\n",
                "Write-Host '{done_marker}'\n"
            ),
            start_marker = TERMINAL_THROUGHPUT_BENCHMARK_START_MARKER,
            line_count = line_count,
            measure_prefix = TERMINAL_THROUGHPUT_BENCHMARK_MEASURE_PREFIX,
            done_marker = TERMINAL_THROUGHPUT_BENCHMARK_DONE_MARKER,
        ),
        TerminalThroughputBenchmarkMode::StreamSmallBatches => format!(
            concat!(
                "$ErrorActionPreference = 'Stop'\n",
                "Write-Host '{start_marker}'\n",
                "Start-Sleep -Milliseconds 100\n",
                "$duration = Measure-Command {{\n",
                "  for ($i = 1; $i -le {line_count}; $i++) {{\n",
                "    [Console]::Out.Write(('chunk-' + $i.ToString([System.Globalization.CultureInfo]::InvariantCulture) + '`r`n'))\n",
                "    if (($i % 32) -eq 0) {{ Start-Sleep -Milliseconds 1 }}\n",
                "  }}\n",
                "}}\n",
                "$measureMs = [string]::Format([System.Globalization.CultureInfo]::InvariantCulture, '{{0:F3}}', $duration.TotalMilliseconds)\n",
                "Write-Host ('{measure_prefix}' + $measureMs)\n",
                "Write-Host '{done_marker}'\n"
            ),
            start_marker = TERMINAL_THROUGHPUT_BENCHMARK_START_MARKER,
            line_count = line_count,
            measure_prefix = TERMINAL_THROUGHPUT_BENCHMARK_MEASURE_PREFIX,
            done_marker = TERMINAL_THROUGHPUT_BENCHMARK_DONE_MARKER,
        ),
        TerminalThroughputBenchmarkMode::WideLines => format!(
            concat!(
                "$ErrorActionPreference = 'Stop'\n",
                "$wide = ('W' * 320)\n",
                "Write-Host '{start_marker}'\n",
                "Start-Sleep -Milliseconds 100\n",
                "$duration = Measure-Command {{\n",
                "  for ($i = 1; $i -le {line_count}; $i++) {{\n",
                "    [Console]::Out.Write(($wide + '|' + $i.ToString([System.Globalization.CultureInfo]::InvariantCulture) + '`r`n'))\n",
                "  }}\n",
                "}}\n",
                "$measureMs = [string]::Format([System.Globalization.CultureInfo]::InvariantCulture, '{{0:F3}}', $duration.TotalMilliseconds)\n",
                "Write-Host ('{measure_prefix}' + $measureMs)\n",
                "Write-Host '{done_marker}'\n"
            ),
            start_marker = TERMINAL_THROUGHPUT_BENCHMARK_START_MARKER,
            line_count = line_count,
            measure_prefix = TERMINAL_THROUGHPUT_BENCHMARK_MEASURE_PREFIX,
            done_marker = TERMINAL_THROUGHPUT_BENCHMARK_DONE_MARKER,
        ),
        TerminalThroughputBenchmarkMode::ScrollFlood => format!(
            concat!(
                "$ErrorActionPreference = 'Stop'\n",
                "Write-Host '{start_marker}'\n",
                "Start-Sleep -Milliseconds 100\n",
                "$duration = Measure-Command {{\n",
                "  for ($i = 1; $i -le {line_count}; $i++) {{\n",
                "    [Console]::Out.Write(('scroll-' + $i.ToString([System.Globalization.CultureInfo]::InvariantCulture).PadLeft(6, '0') + ' ' + ('#' * 120) + '`r`n'))\n",
                "    if (($i % 128) -eq 0) {{ [Console]::Out.Flush() }}\n",
                "  }}\n",
                "}}\n",
                "$measureMs = [string]::Format([System.Globalization.CultureInfo]::InvariantCulture, '{{0:F3}}', $duration.TotalMilliseconds)\n",
                "Write-Host ('{measure_prefix}' + $measureMs)\n",
                "Write-Host '{done_marker}'\n"
            ),
            start_marker = TERMINAL_THROUGHPUT_BENCHMARK_START_MARKER,
            line_count = line_count.saturating_mul(4),
            measure_prefix = TERMINAL_THROUGHPUT_BENCHMARK_MEASURE_PREFIX,
            done_marker = TERMINAL_THROUGHPUT_BENCHMARK_DONE_MARKER,
        ),
        TerminalThroughputBenchmarkMode::PromptBursts => format!(
            concat!(
                "$ErrorActionPreference = 'Stop'\n",
                "Write-Host '{start_marker}'\n",
                "Start-Sleep -Milliseconds 100\n",
                "$duration = Measure-Command {{\n",
                "  for ($i = 1; $i -le {line_count}; $i++) {{\n",
                "    [Console]::Out.Write(('PS benchmark> command-' + $i.ToString([System.Globalization.CultureInfo]::InvariantCulture) + '`r`n'))\n",
                "    [Console]::Out.Write(('result-' + $i.ToString([System.Globalization.CultureInfo]::InvariantCulture) + ': ' + ('*' * 48) + '`r`n'))\n",
                "    if (($i % 24) -eq 0) {{ Start-Sleep -Milliseconds 2 }}\n",
                "  }}\n",
                "}}\n",
                "$measureMs = [string]::Format([System.Globalization.CultureInfo]::InvariantCulture, '{{0:F3}}', $duration.TotalMilliseconds)\n",
                "Write-Host ('{measure_prefix}' + $measureMs)\n",
                "Write-Host '{done_marker}'\n"
            ),
            start_marker = TERMINAL_THROUGHPUT_BENCHMARK_START_MARKER,
            line_count = line_count.max(1),
            measure_prefix = TERMINAL_THROUGHPUT_BENCHMARK_MEASURE_PREFIX,
            done_marker = TERMINAL_THROUGHPUT_BENCHMARK_DONE_MARKER,
        ),
        TerminalThroughputBenchmarkMode::ResizeDuringOutput => format!(
            concat!(
                "$ErrorActionPreference = 'Stop'\n",
                "Write-Host '{start_marker}'\n",
                "Start-Sleep -Milliseconds 100\n",
                "$duration = Measure-Command {{\n",
                "  for ($i = 1; $i -le {line_count}; $i++) {{\n",
                "    [Console]::Out.Write(('resize-' + $i.ToString([System.Globalization.CultureInfo]::InvariantCulture) + ' ' + ('=' * 160) + '`r`n'))\n",
                "    if (($i % 16) -eq 0) {{ Start-Sleep -Milliseconds 2 }}\n",
                "  }}\n",
                "}}\n",
                "$measureMs = [string]::Format([System.Globalization.CultureInfo]::InvariantCulture, '{{0:F3}}', $duration.TotalMilliseconds)\n",
                "Write-Host ('{measure_prefix}' + $measureMs)\n",
                "Write-Host '{done_marker}'\n"
            ),
            start_marker = TERMINAL_THROUGHPUT_BENCHMARK_START_MARKER,
            line_count = line_count.max(1),
            measure_prefix = TERMINAL_THROUGHPUT_BENCHMARK_MEASURE_PREFIX,
            done_marker = TERMINAL_THROUGHPUT_BENCHMARK_DONE_MARKER,
        ),
    };

    crate::shell_default::command_builder_from_argv(&[
        "pwsh.exe".to_owned(),
        "-NoLogo".to_owned(),
        "-NoProfile".to_owned(),
        "-Command".to_owned(),
        script,
    ])
}

fn terminal_throughput_mode_name(mode: TerminalThroughputBenchmarkMode) -> &'static str {
    match mode {
        TerminalThroughputBenchmarkMode::MeasureCommandOutHost => "measure-command-out-host",
        TerminalThroughputBenchmarkMode::StreamSmallBatches => "stream-small-batches",
        TerminalThroughputBenchmarkMode::WideLines => "wide-lines",
        TerminalThroughputBenchmarkMode::ScrollFlood => "scroll-flood",
        TerminalThroughputBenchmarkMode::PromptBursts => "prompt-bursts",
        TerminalThroughputBenchmarkMode::ResizeDuringOutput => "resize-during-output",
    }
}

fn parse_terminal_throughput_measure_command_ms(screen: &str) -> eyre::Result<f64> {
    for line in screen.lines() {
        if let Some(value) = line.strip_prefix(TERMINAL_THROUGHPUT_BENCHMARK_MEASURE_PREFIX) {
            return value.trim().parse::<f64>().wrap_err_with(|| {
                format!("failed to parse benchmark measure-command output line `{line}`")
            });
        }
    }

    eyre::bail!(
        "terminal throughput benchmark output did not include `{TERMINAL_THROUGHPUT_BENCHMARK_MEASURE_PREFIX}`\n\n=== screen ===\n{screen}"
    )
}

fn measure_terminal_cell_size(font_height: i32) -> eyre::Result<(i32, i32)> {
    let font_definition = terminal_font_definition(font_height);
    // Safety: `font_definition` is fully initialized and valid for CreateFontIndirectW.
    let font = unsafe { CreateFontIndirectW(&raw const font_definition) };
    if font.0.is_null() {
        eyre::bail!("failed to create terminal font")
    }
    let font = FontHandle(font);

    // Safety: querying the screen DC with a null HWND is valid.
    let hdc = unsafe { GetDC(None) };
    if hdc.0.is_null() {
        eyre::bail!("failed to acquire screen DC for font metrics")
    }

    // Safety: selecting the created font into the device context is valid.
    let previous_font = unsafe { SelectObject(hdc, font.0.into()) };
    let glyph = ['W' as u16];
    let mut size = SIZE::default();
    // Safety: `glyph` and `size` remain valid for the duration of the measurement call.
    let measured = unsafe { GetTextExtentPoint32W(hdc, &glyph, &raw mut size) }.as_bool();
    // Safety: restoring the previous selected object is valid for this device context.
    let _ = unsafe { SelectObject(hdc, previous_font) };
    // Safety: releasing the screen DC after GetDC(None) is required.
    unsafe { ReleaseDC(None, hdc) };

    if !measured {
        eyre::bail!("failed to measure terminal font")
    }

    Ok((size.cx.max(8), size.cy.max(16)))
}

fn terminal_font_definition(font_height: i32) -> LOGFONTW {
    let mut font = LOGFONTW {
        lfHeight: font_height,
        lfQuality: CLEARTYPE_QUALITY,
        ..Default::default()
    };
    for (slot, value) in font.lfFaceName.iter_mut().zip(FONT_FAMILY.encode_utf16()) {
        *slot = value;
    }
    font
}

fn handle_mouse_wheel(hwnd: WindowHandle, wparam: WPARAM, lparam: LPARAM) -> eyre::Result<bool> {
    // behavior[impl window.interaction.zoom.terminal]
    // behavior[impl window.interaction.zoom.output]
    let ctrl_down = control_key_is_down();
    if !ctrl_down {
        return with_app_state(|state| {
            let layout =
                client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?;
            let point = screen_to_client_point(hwnd, lparam)?;
            if !layout.code_panel_rect().contains(point) {
                return Ok(false);
            }

            let wheel_delta = high_word_i16(wparam.0);
            if wheel_delta == 0 {
                return Ok(true);
            }

            let steps = if wheel_delta.abs() < MOUSE_WHEEL_DELTA {
                isize::from(wheel_delta.signum())
            } else {
                isize::from(wheel_delta / MOUSE_WHEEL_DELTA)
            };
            let line_delta = -steps * TERMINAL_WHEEL_SCROLL_LINES;
            state.terminal.scroll_viewport_by(line_delta);
            render_current_frame(state, hwnd, None)?;
            Ok(true)
        });
    }

    with_app_state(|state| {
        let layout = client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?;
        let point = screen_to_client_point(hwnd, lparam)?;
        let in_terminal = layout.terminal_rect().contains(point);
        let in_output = layout.result_panel_rect().contains(point);
        if !in_terminal && !in_output {
            return Ok(false);
        }

        let wheel_delta = high_word_i16(wparam.0);
        if wheel_delta == 0 {
            return Ok(true);
        }

        let zoom_direction = if wheel_delta > 0 { -1 } else { 1 };
        if in_terminal {
            let next_font_height = (state.terminal_font_height + (zoom_direction * FONT_ZOOM_STEP))
                .clamp(MAX_FONT_HEIGHT, MIN_FONT_HEIGHT);
            if next_font_height == state.terminal_font_height {
                return Ok(true);
            }

            let (cell_width, cell_height) = measure_terminal_cell_size(next_font_height)?;
            debug!(
                font_height = next_font_height,
                const_name = "TERMINAL_FONT_HEIGHT",
                "terminal zoom changed; use this font height for the default constant"
            );
            state.terminal_font_height = next_font_height;
            state.terminal_cell_width = cell_width;
            state.terminal_cell_height = cell_height;

            let layout =
                client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?;
            state.pending_terminal_resize = None;
            apply_terminal_resize(state, layout)?;
            render_current_frame(state, hwnd, None)?;
            return Ok(true);
        }

        let next_font_height = (state.output_font_height + (zoom_direction * FONT_ZOOM_STEP))
            .clamp(MAX_FONT_HEIGHT, MIN_FONT_HEIGHT);
        if next_font_height == state.output_font_height {
            return Ok(true);
        }

        let (cell_width, cell_height) = measure_terminal_cell_size(next_font_height)?;
        debug!(
            font_height = next_font_height,
            const_name = "OUTPUT_FONT_HEIGHT",
            "output zoom changed; use this font height for the default constant"
        );
        state.output_font_height = next_font_height;
        state.output_cell_width = cell_width;
        state.output_cell_height = cell_height;
        render_current_frame(state, hwnd, None)?;
        Ok(true)
    })
}

fn client_layout(
    hwnd: WindowHandle,
    cell_width: i32,
    cell_height: i32,
) -> eyre::Result<TerminalLayout> {
    let rect = hwnd.client_rect()?;
    Ok(TerminalLayout {
        client_width: rect.width(),
        client_height: rect.height(),
        cell_width,
        cell_height,
    })
}

fn effective_terminal_grid_size(layout: TerminalLayout) -> (u16, u16) {
    layout.grid_size()
}

fn visible_terminal_display_capacity(
    layout: TerminalLayout,
    terminal_cell_width: i32,
    terminal_cell_height: i32,
) -> (i32, i32) {
    let _ = (terminal_cell_width, terminal_cell_height);
    layout.visible_grid_size()
}

fn clip_terminal_display_to_layout(
    display: Arc<TerminalDisplayState>,
    layout: TerminalLayout,
    terminal_cell_width: i32,
    terminal_cell_height: i32,
) -> Arc<TerminalDisplayState> {
    let (visible_cols, visible_rows) =
        visible_terminal_display_capacity(layout, terminal_cell_width, terminal_cell_height);
    let visible_row_count = usize::try_from(visible_rows).unwrap_or_default();

    let needs_clip = display.rows.len() > visible_row_count
        || display.cursor.is_some_and(|cursor| {
            cursor.cell.column() >= visible_cols || cursor.cell.row() >= visible_rows
        })
        || display.scrollbar.is_some_and(|scrollbar| {
            scrollbar.visible
                != scrollbar
                    .visible
                    .min(u64::try_from(visible_row_count).unwrap_or(u64::MAX))
        })
        || display.rows.iter().take(visible_row_count).any(|row| {
            row.backgrounds.iter().any(|background| {
                background.cell.column() >= visible_cols || background.cell.row() >= visible_rows
            }) || row.glyphs.iter().any(|glyph| {
                glyph.cell.column() >= visible_cols || glyph.cell.row() >= visible_rows
            })
        });

    if !needs_clip {
        return display;
    }

    let mut rows = Vec::with_capacity(display.rows.len().min(visible_row_count));
    for row in display.rows.iter().take(visible_row_count) {
        let mut row = row.clone();
        row.backgrounds.retain(|background| {
            background.cell.column() >= 0
                && background.cell.column() < visible_cols
                && background.cell.row() >= 0
                && background.cell.row() < visible_rows
        });
        row.glyphs.retain(|glyph| {
            glyph.cell.column() >= 0
                && glyph.cell.column() < visible_cols
                && glyph.cell.row() >= 0
                && glyph.cell.row() < visible_rows
        });
        rows.push(row);
    }

    Arc::new(TerminalDisplayState {
        rows,
        dirty_rows: (0..visible_row_count.min(display.rows.len())).collect(),
        cursor: display.cursor.filter(|cursor| {
            cursor.cell.column() >= 0
                && cursor.cell.column() < visible_cols
                && cursor.cell.row() >= 0
                && cursor.cell.row() < visible_rows
        }),
        scrollbar: display.scrollbar.map(|mut scrollbar| {
            scrollbar.visible = scrollbar
                .visible
                .min(u64::try_from(visible_row_count).unwrap_or(u64::MAX));
            scrollbar
        }),
    })
}

fn apply_terminal_resize(state: &mut AppState, layout: TerminalLayout) -> eyre::Result<bool> {
    if state.terminal_layout == Some(layout) {
        return Ok(false);
    }

    state.terminal.resize(layout)?;
    state.terminal_layout = Some(layout);
    Ok(true)
}

fn apply_pending_terminal_resize(state: &mut AppState) -> eyre::Result<bool> {
    let Some(layout) = state.pending_terminal_resize.take() else {
        return Ok(false);
    };

    apply_terminal_resize(state, layout)
}

fn with_app_state<T>(f: impl FnOnce(&mut AppState) -> eyre::Result<T>) -> eyre::Result<T> {
    APP_STATE.with(|state| {
        let mut borrowed = state.borrow_mut();
        let app_state = borrowed
            .as_mut()
            .ok_or_else(|| eyre::eyre!("application state was not initialized"))?;
        f(app_state)
    })
}

fn handle_left_button_up(hwnd: WindowHandle, lparam: LPARAM) -> eyre::Result<bool> {
    with_app_state(|state| {
        if state.terminal_scrollbar_drag.take().is_some() {
            let point = ClientPoint::from_lparam(lparam);
            let layout =
                client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?;
            state.terminal_scrollbar_hovered_part =
                current_terminal_scrollbar(state)?.and_then(|scrollbar| {
                    terminal_scrollbar_hit_test(
                        layout.terminal_scrollbar_rect().inset(4),
                        scrollbar,
                        point,
                    )
                });
            hwnd.release_mouse_capture();
            render_current_frame(state, hwnd, None)?;
            return Ok(true);
        }

        if state.pending_window_drag.take().is_some() {
            return Ok(true);
        }

        if let Some(pending_selection) = state.pending_terminal_selection.take() {
            state.terminal_selection_drag_point = None;
            let layout =
                client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?;
            let point = ClientPoint::from_lparam(lparam);
            let cell = terminal_cell_from_client_point(layout, point, true)
                .map(|cell| state.terminal.viewport_to_screen_cell(cell))
                .transpose()?;
            if let Some(selection) =
                complete_pending_terminal_selection(pending_selection, point, cell)
            {
                state.terminal_selection = Some(selection);
            }
            render_current_frame(state, hwnd, None)?;
            return Ok(true);
        }

        let Some(workspace_window) = state.workspace_window.clone() else {
            return Ok(false);
        };

        let layout = client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?;
        let point = ClientPoint::from_lparam(lparam);
        if !layout.plus_button_rect().contains(point) {
            return Ok(false);
        }

        let app_home = state.app_home.clone();
        let cache_home = workspace_window.cache_home.clone();
        let workspace_id = workspace_window.workspace.id.clone();

        thread::Builder::new()
            .name(format!(
                "teamy-studio-cell-{}",
                workspace_window.cell_number + 1
            ))
            .spawn(move || {
                let launch_result =
                    crate::workspace::append_workspace_cell(&cache_home, &workspace_id).and_then(
                        |launch| super::run_workspace_launch(&app_home, &cache_home, launch),
                    );
                if let Err(error) = launch_result {
                    error!(?error, "failed to open additional Teamy Studio cell window");
                }
            })
            .wrap_err("failed to spawn Teamy Studio cell window thread")?;

        Ok(true)
    })
}

/// behavior[impl window.interaction.drag]
/// behavior[impl window.interaction.selection.linear]
/// behavior[impl window.interaction.selection.block-alt-drag]
/// behavior[impl window.interaction.selection.click-dismiss]
fn handle_left_button_down(hwnd: WindowHandle, lparam: LPARAM) -> eyre::Result<bool> {
    let point = ClientPoint::from_lparam(lparam);
    let in_drag_handle = hit_test_drag_handle_point(hwnd, point)?;
    let selection_mode = if alt_key_is_down() {
        TerminalSelectionMode::Block
    } else {
        TerminalSelectionMode::Linear
    };

    with_app_state(|state| {
        state.pending_window_drag = None;
        state.terminal_selection_drag_point = None;
        if !in_drag_handle {
            let layout =
                client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?;
            state.pending_terminal_selection = None;

            if let Some(scrollbar) = current_terminal_scrollbar(state)? {
                let scrollbar_rect = layout.terminal_scrollbar_rect().inset(4);
                if let Some(part) = terminal_scrollbar_hit_test(scrollbar_rect, scrollbar, point) {
                    let Some(geometry) = terminal_scrollbar_geometry(scrollbar_rect, scrollbar)
                    else {
                        return Ok(false);
                    };
                    let point_y = point.to_win32_point()?.y;
                    let grab_offset_y = match part {
                        TerminalScrollbarPart::Thumb => point_y - geometry.thumb_rect.top(),
                        TerminalScrollbarPart::Track => geometry.thumb_height / 2,
                    };
                    let target_offset = match part {
                        TerminalScrollbarPart::Thumb => scrollbar.offset.min(geometry.max_offset),
                        TerminalScrollbarPart::Track => terminal_scrollbar_offset_for_pointer(
                            scrollbar_rect,
                            geometry,
                            point,
                            grab_offset_y,
                        )?,
                    };

                    state.terminal_scrollbar_hovered_part = Some(part);
                    state.terminal_scrollbar_drag = Some(TerminalScrollbarDrag { grab_offset_y });
                    state.terminal.scroll_viewport_to_offset(target_offset)?;
                    hwnd.capture_mouse();
                    render_current_frame(state, hwnd, None)?;
                    return Ok(true);
                }
            }

            state.terminal_selection = None;
            state.terminal_scrollbar_hovered_part = None;
            if let Some(cell) = terminal_cell_from_client_point(layout, point, false) {
                let anchor = state.terminal.viewport_to_screen_cell(cell)?;
                state.pending_terminal_selection = Some(PendingTerminalSelection {
                    origin: point,
                    anchor,
                    mode: selection_mode,
                });
                state.terminal_selection_drag_point = Some(point);
                render_current_frame(state, hwnd, None)?;
                return Ok(true);
            }

            return Ok(false);
        }

        state.terminal_selection = None;
        state.pending_terminal_selection = None;
        state.terminal_selection_drag_point = None;
        state.terminal_scrollbar_hovered_part = None;
        state.terminal_scrollbar_drag = None;
        begin_system_window_drag(hwnd, point)?;
        Ok(true)
    })
}

fn handle_mouse_move(hwnd: WindowHandle, wparam: WPARAM, lparam: LPARAM) -> eyre::Result<bool> {
    let point = ClientPoint::from_lparam(lparam);

    let selection_result = with_app_state(|state| {
        let Some(pending_selection) = state.pending_terminal_selection else {
            return Ok(None);
        };

        state.terminal_selection_drag_point = Some(point);

        let action = if (wparam.0 & 0x0001) == 0 {
            update_pending_terminal_selection_action(pending_selection, point, false, None)
        } else if point == pending_selection.origin {
            update_pending_terminal_selection_action(pending_selection, point, true, None)
        } else {
            let layout =
                client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?;
            let cell = terminal_cell_from_client_point(layout, point, true)
                .map(|cell| state.terminal.viewport_to_screen_cell(cell))
                .transpose()?;
            update_pending_terminal_selection_action(pending_selection, point, true, cell)
        };

        match action {
            PendingTerminalSelectionAction::KeepPending => Ok(Some(true)),
            PendingTerminalSelectionAction::ClearPending => {
                state.pending_terminal_selection = None;
                state.terminal_selection_drag_point = None;
                Ok(Some(state.terminal_selection.is_some()))
            }
            PendingTerminalSelectionAction::Update(selection) => {
                state.terminal_selection = Some(selection);
                Ok(Some(true))
            }
        }
    })?;

    if let Some(consumed) = selection_result {
        if consumed {
            with_app_state(|state| render_current_frame(state, hwnd, None))?;
        }
        return Ok(consumed);
    }

    let scrollbar_result =
        with_app_state(|state| handle_terminal_scrollbar_mouse_move(state, hwnd, point))?;

    if let Some(consumed) = scrollbar_result {
        if consumed {
            with_app_state(|state| render_current_frame(state, hwnd, None))?;
        }
        return Ok(consumed);
    }

    let action = with_app_state(|state| {
        let Some(pending_drag) = state.pending_window_drag else {
            return Ok(PendingDragAction::NotHandled);
        };

        let action = update_pending_drag_action(
            pending_drag,
            point,
            (wparam.0 & 0x0001) != 0,
            DRAG_START_THRESHOLD_PX,
            DRAG_START_THRESHOLD_PX,
        );
        if action.clears_pending_drag() {
            state.pending_window_drag = None;
        }
        Ok(action)
    })?;

    match action {
        PendingDragAction::NotHandled => Ok(false),
        PendingDragAction::Consumed => Ok(true),
        PendingDragAction::StartSystemDrag => {
            begin_system_window_drag(hwnd, point)
                .wrap_err("failed to hand deferred drag strip motion to the native move loop")?;
            Ok(true)
        }
    }
}

fn handle_terminal_scrollbar_mouse_move(
    state: &mut AppState,
    hwnd: WindowHandle,
    point: ClientPoint,
) -> eyre::Result<Option<bool>> {
    let layout = client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?;
    let scrollbar_rect = layout.terminal_scrollbar_rect().inset(4);
    let scrollbar = current_terminal_scrollbar(state)?;

    if let Some(drag) = state.terminal_scrollbar_drag {
        if !left_mouse_button_is_down() {
            state.terminal_scrollbar_drag = None;
            state.terminal_scrollbar_hovered_part = None;
            hwnd.release_mouse_capture();
            return Ok(Some(true));
        }

        let Some(scrollbar) = scrollbar else {
            state.terminal_scrollbar_drag = None;
            state.terminal_scrollbar_hovered_part = None;
            hwnd.release_mouse_capture();
            return Ok(Some(true));
        };
        let Some(geometry) = terminal_scrollbar_geometry(scrollbar_rect, scrollbar) else {
            state.terminal_scrollbar_drag = None;
            state.terminal_scrollbar_hovered_part = None;
            hwnd.release_mouse_capture();
            return Ok(Some(true));
        };

        state.terminal_scrollbar_hovered_part =
            terminal_scrollbar_hit_test(scrollbar_rect, scrollbar, point);
        let target_offset = terminal_scrollbar_offset_for_pointer(
            scrollbar_rect,
            geometry,
            point,
            drag.grab_offset_y,
        )?;
        state.terminal.scroll_viewport_to_offset(target_offset)?;
        return Ok(Some(true));
    }

    if state.pending_window_drag.is_some() {
        return Ok(None);
    }

    let hovered_part = scrollbar
        .and_then(|scrollbar| terminal_scrollbar_hit_test(scrollbar_rect, scrollbar, point));
    state.terminal_scrollbar_hovered_part = hovered_part;
    Ok(hovered_part.map(|_| true))
}

/// behavior[impl window.interaction.clipboard.right-click-copy-selection]
/// behavior[impl window.interaction.clipboard.right-click-paste]
/// behavior[impl window.interaction.clipboard.right-click-paste.confirm-multiline]
fn handle_right_button_up(hwnd: WindowHandle, lparam: LPARAM) -> eyre::Result<bool> {
    let point = ClientPoint::from_lparam(lparam);

    let preparation = with_app_state(|state| {
        let layout = client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?;
        if !terminal_render_rect(layout).contains(point) {
            return Ok(RightClickTerminalPreparation::NotTerminal);
        }

        state.pending_terminal_selection = None;
        state.terminal_selection_drag_point = None;
        if state.terminal_scrollbar_drag.take().is_some() {
            hwnd.release_mouse_capture();
        }

        if let Some(selection) = state.terminal_selection.take() {
            return Ok(RightClickTerminalPreparation::CopySelection(
                state.terminal.selected_text(selection)?,
            ));
        }

        Ok(RightClickTerminalPreparation::QueryClipboard)
    })?;

    match preparation {
        RightClickTerminalPreparation::NotTerminal => Ok(false),
        RightClickTerminalPreparation::CopySelection(selected_text) => {
            if !selected_text.is_empty()
                && let Err(error) = write_clipboard(&selected_text)
            {
                error!(?error, "failed to copy terminal selection to the clipboard");
            }
            Ok(true)
        }
        RightClickTerminalPreparation::QueryClipboard => {
            let clipboard_text = match read_clipboard() {
                Ok(clipboard_text) => clipboard_text,
                Err(error) => {
                    error!(?error, "failed to read clipboard text for terminal paste");
                    return Ok(true);
                }
            };

            match right_click_terminal_action(false, &clipboard_text) {
                Some(RightClickTerminalAction::Paste) => {
                    with_app_state(|state| {
                        state.terminal.handle_paste(&clipboard_text)?;
                        Ok(())
                    })?;
                    Ok(true)
                }
                Some(RightClickTerminalAction::ConfirmPaste) => {
                    // Native modal dialogs pump the message loop, so they must not run while
                    // the mutable APP_STATE RefCell borrow is held.
                    let choice = show_multiline_paste_confirmation_dialog(
                        Some(hwnd.raw()),
                        &clipboard_text,
                    )?;
                    if choice == PasteConfirmationChoice::Paste {
                        with_app_state(|state| {
                            state.terminal.handle_paste(&clipboard_text)?;
                            Ok(())
                        })?;
                    }
                    Ok(true)
                }
                Some(RightClickTerminalAction::CopySelection) => {
                    unreachable!("selection copies are prepared before clipboard lookup")
                }
                None => Ok(true),
            }
        }
    }
}

fn right_click_terminal_action(
    has_selection: bool,
    clipboard_text: &str,
) -> Option<RightClickTerminalAction> {
    if has_selection {
        Some(RightClickTerminalAction::CopySelection)
    } else if clipboard_text.is_empty() {
        None
    } else if paste_confirmation_required(clipboard_text) {
        Some(RightClickTerminalAction::ConfirmPaste)
    } else {
        Some(RightClickTerminalAction::Paste)
    }
}

/// os[impl window.interaction.drag.threshold]
fn update_pending_drag_action(
    pending_drag: PendingWindowDrag,
    point: ClientPoint,
    left_button_down: bool,
    threshold_x: i32,
    threshold_y: i32,
) -> PendingDragAction {
    if !left_button_down {
        return PendingDragAction::NotHandled;
    }

    if !drag_threshold_exceeded(pending_drag.origin, point, threshold_x, threshold_y) {
        return PendingDragAction::Consumed;
    }

    PendingDragAction::StartSystemDrag
}

fn update_pending_terminal_selection_action(
    pending_selection: PendingTerminalSelection,
    point: ClientPoint,
    left_button_down: bool,
    cell: Option<TerminalCellPoint>,
) -> PendingTerminalSelectionAction {
    if !left_button_down {
        return PendingTerminalSelectionAction::ClearPending;
    }

    if point == pending_selection.origin {
        return PendingTerminalSelectionAction::KeepPending;
    }

    let Some(cell) = cell else {
        return PendingTerminalSelectionAction::KeepPending;
    };

    PendingTerminalSelectionAction::Update(TerminalSelection::new(
        pending_selection.anchor,
        cell,
        pending_selection.mode,
    ))
}

fn complete_pending_terminal_selection(
    pending_selection: PendingTerminalSelection,
    point: ClientPoint,
    cell: Option<TerminalCellPoint>,
) -> Option<TerminalSelection> {
    if point == pending_selection.origin {
        return None;
    }

    cell.map(|cell| TerminalSelection::new(pending_selection.anchor, cell, pending_selection.mode))
}

fn hit_test_drag_handle_point(hwnd: WindowHandle, point: ClientPoint) -> eyre::Result<bool> {
    with_app_state(|state| {
        let layout = client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?;
        Ok(layout.drag_handle_rect().contains(point))
    })
}

fn begin_system_window_drag(hwnd: WindowHandle, client_point: ClientPoint) -> eyre::Result<()> {
    let screen_point = client_to_screen_point(hwnd, client_point)?;
    let (wparam, lparam) = system_drag_message(screen_point)?;
    hwnd.post_system_drag(wparam, lparam);
    Ok(())
}

fn system_drag_message(screen_point: ScreenPoint) -> eyre::Result<(WPARAM, LPARAM)> {
    Ok((
        WPARAM(usize::try_from(HTCAPTION).expect("HTCAPTION fits in usize")),
        LPARAM(screen_point.pack_lparam()?),
    ))
}

fn screen_to_client_point(hwnd: WindowHandle, lparam: LPARAM) -> eyre::Result<ClientPoint> {
    let screen_point = ScreenPoint::from_lparam(lparam);
    screen_to_client_point_from_screen(hwnd, screen_point)
}

fn cursor_client_point(hwnd: WindowHandle) -> eyre::Result<ClientPoint> {
    let screen_point = query_cursor_pos()?;
    screen_to_client_point_from_screen(hwnd, screen_point)
}

fn screen_to_client_point_from_screen(
    hwnd: WindowHandle,
    screen_point: ScreenPoint,
) -> eyre::Result<ClientPoint> {
    let transform = ScreenToClientTransform::for_window(hwnd.window_rect()?);
    Ok(transform.screen_to_client(screen_point))
}

fn client_to_screen_point(
    hwnd: WindowHandle,
    client_point: ClientPoint,
) -> eyre::Result<ScreenPoint> {
    let transform = ScreenToClientTransform::for_window(hwnd.window_rect()?);
    Ok(transform.client_to_screen(client_point))
}

/// behavior[impl window.appearance.drag-cursor]
fn handle_set_cursor(hwnd: WindowHandle, lparam: LPARAM) -> eyre::Result<bool> {
    if !should_override_drag_cursor(with_app_state(|state| Ok(state.in_move_size_loop))?) {
        return Ok(false);
    }

    let hit_test_code = u32::from(low_word_u16(lparam.0));
    if hit_test_code != HTCAPTION && hit_test_code != HTCLIENT {
        return Ok(false);
    }

    let point = cursor_client_point(hwnd)?;
    if !hit_test_drag_handle_point(hwnd, point)? {
        return Ok(false);
    }

    let move_cursor = load_cursor(IDC_SIZEALL);
    // Safety: setting the cursor for the current WM_SETCURSOR handling path is valid.
    unsafe { SetCursor(Some(move_cursor)) };
    Ok(true)
}

fn hit_test_resize_border(hwnd: WindowHandle, point: ClientPoint) -> eyre::Result<Option<LRESULT>> {
    let client_rect = hwnd
        .client_rect()
        .wrap_err("failed to query client rect for hit testing")?;

    let resize_border_x = resize_border_thickness(SM_CXSIZEFRAME);
    let resize_border_y = resize_border_thickness(SM_CYSIZEFRAME);
    let hit = classify_resize_border_hit(client_rect, point, resize_border_x, resize_border_y);

    Ok(hit.map(|code| LRESULT(isize::try_from(code).expect("hit-test code fits in isize"))))
}

fn resize_border_thickness(size_frame_metric: SYSTEM_METRICS_INDEX) -> i32 {
    let padded_border = system_metric(SM_CXPADDEDBORDER);
    let size_frame = system_metric(size_frame_metric);
    (size_frame + padded_border).max(MIN_RESIZE_BORDER_THICKNESS)
}

fn fail_and_close(hwnd: WindowHandle, error: &eyre::Error) -> LRESULT {
    tracing::error!(?error, "terminal window failed");
    hwnd.destroy();
    LRESULT(0)
}

fn auto_scroll_pending_terminal_selection(
    state: &mut AppState,
    hwnd: WindowHandle,
) -> eyre::Result<bool> {
    let Some(pending_selection) = state.pending_terminal_selection else {
        state.terminal_selection_drag_point = None;
        return Ok(false);
    };
    if !left_mouse_button_is_down() {
        state.pending_terminal_selection = None;
        state.terminal_selection_drag_point = None;
        return Ok(false);
    }

    let Some(point) = state.terminal_selection_drag_point else {
        return Ok(false);
    };

    let layout = client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?;
    let scroll_delta =
        terminal_selection_autoscroll_delta(layout, point, state.terminal_cell_height)?;
    if scroll_delta == 0 {
        return Ok(false);
    }

    state.terminal.scroll_viewport_by(scroll_delta);
    let cell = terminal_cell_from_client_point(layout, point, true)
        .map(|cell| state.terminal.viewport_to_screen_cell(cell))
        .transpose()?;
    if let PendingTerminalSelectionAction::Update(selection) =
        update_pending_terminal_selection_action(pending_selection, point, true, cell)
    {
        state.terminal_selection = Some(selection);
    }

    Ok(true)
}

/// behavior[impl window.interaction.selection.drag-auto-scroll]
fn terminal_selection_autoscroll_delta(
    layout: TerminalLayout,
    point: ClientPoint,
    cell_height: i32,
) -> eyre::Result<isize> {
    let point = point.to_win32_point()?;
    let rect = terminal_render_rect(layout);
    if point.y < rect.top() {
        let overshoot = rect.top() - point.y;
        return Ok(-scroll_lines_for_overshoot(overshoot, cell_height));
    }
    if point.y >= rect.bottom() {
        let overshoot = point.y - rect.bottom() + 1;
        return Ok(scroll_lines_for_overshoot(overshoot, cell_height));
    }

    Ok(0)
}

fn scroll_lines_for_overshoot(overshoot: i32, cell_height: i32) -> isize {
    let base = overshoot.max(1);
    let lines = 1 + (base / cell_height.max(1));
    isize::try_from(lines)
        .unwrap_or(SELECTION_AUTO_SCROLL_MAX_LINES)
        .clamp(1, SELECTION_AUTO_SCROLL_MAX_LINES)
}

fn left_mouse_button_is_down() -> bool {
    // Safety: querying the async key state for the left mouse button does not require extra invariants.
    let state = unsafe { GetKeyState(i32::from(VK_LBUTTON.0)) };
    (state.cast_unsigned() & 0x8000) != 0
}

fn wide_null_terminated(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hollow_cursor_builds_four_border_rects() {
        let rects = terminal_cursor_overlay_rects(
            ClientRect::new(10, 20, 18, 36),
            TerminalDisplayCursorStyle::BlockHollow,
        );

        assert_eq!(rects.len(), 4);
        assert_eq!(rects[0].top(), 20);
        assert_eq!(rects[1].bottom(), 36);
        assert_eq!(rects[2].left(), 10);
        assert_eq!(rects[3].right(), 18);
    }

    // behavior[verify window.appearance.drag-cursor]
    #[test]
    fn drag_cursor_override_is_disabled_during_native_move_size() {
        assert!(should_override_drag_cursor(false));
        assert!(!should_override_drag_cursor(true));
    }

    #[test]
    fn pending_drag_is_consumed_before_threshold_is_crossed() {
        let action = update_pending_drag_action(
            PendingWindowDrag {
                origin: ClientPoint::new(10, 20),
            },
            ClientPoint::new(10, 20),
            true,
            1,
            1,
        );

        assert_eq!(action, PendingDragAction::Consumed);
        assert!(!action.clears_pending_drag());
    }

    // os[verify window.interaction.drag.threshold]
    #[test]
    fn pending_drag_starts_immediately_when_threshold_is_zero() {
        let action = update_pending_drag_action(
            PendingWindowDrag {
                origin: ClientPoint::new(10, 20),
            },
            ClientPoint::new(10, 20),
            true,
            0,
            0,
        );

        assert_eq!(action, PendingDragAction::StartSystemDrag);
        assert!(action.clears_pending_drag());
    }

    // behavior[verify window.interaction.drag]
    // os[verify window.interaction.drag.threshold]
    #[test]
    fn pending_drag_requests_native_drag_after_threshold_is_crossed() {
        let action = update_pending_drag_action(
            PendingWindowDrag {
                origin: ClientPoint::new(10, 20),
            },
            ClientPoint::new(11, 20),
            true,
            DRAG_START_THRESHOLD_PX,
            DRAG_START_THRESHOLD_PX,
        );

        assert_eq!(action, PendingDragAction::StartSystemDrag);
        assert!(action.clears_pending_drag());
    }

    #[test]
    fn pending_drag_clears_when_button_is_released() {
        let action = update_pending_drag_action(
            PendingWindowDrag {
                origin: ClientPoint::new(10, 20),
            },
            ClientPoint::new(10, 20),
            false,
            DRAG_START_THRESHOLD_PX,
            DRAG_START_THRESHOLD_PX,
        );

        assert_eq!(action, PendingDragAction::NotHandled);
        assert!(action.clears_pending_drag());
    }

    #[test]
    fn clips_terminal_display_when_layout_cannot_show_any_full_rows() {
        let layout = TerminalLayout {
            client_width: 320,
            client_height: 40,
            cell_width: 8,
            cell_height: 16,
        };
        let display = Arc::new(TerminalDisplayState {
            rows: vec![crate::app::windows_terminal::TerminalDisplayRow::default()],
            dirty_rows: vec![0],
            cursor: None,
            scrollbar: Some(TerminalDisplayScrollbar {
                total: 100,
                offset: 0,
                visible: 1,
            }),
        });

        let clipped = clip_terminal_display_to_layout(display, layout, 8, 16);

        assert!(clipped.rows.is_empty());
        assert_eq!(clipped.scrollbar.expect("scrollbar preserved").visible, 0,);
    }

    #[test]
    fn parses_terminal_throughput_measure_command_marker() {
        let screen = concat!(
            "header\n",
            "__TEAMY_TERMINAL_THROUGHPUT_MEASURE_MS=123.456\n",
            "footer\n"
        );

        let measure_ms = parse_terminal_throughput_measure_command_ms(screen)
            .expect("benchmark marker should parse");

        assert_eq!(measure_ms, 123.456);
    }

    // behavior[verify window.interaction.selection.click-dismiss]
    #[test]
    fn pending_terminal_selection_does_not_materialize_without_pointer_movement() {
        let action = update_pending_terminal_selection_action(
            PendingTerminalSelection {
                origin: ClientPoint::new(10, 20),
                anchor: TerminalCellPoint::new(2, 3),
                mode: TerminalSelectionMode::Linear,
            },
            ClientPoint::new(10, 20),
            true,
            Some(TerminalCellPoint::new(2, 3)),
        );

        assert_eq!(action, PendingTerminalSelectionAction::KeepPending);
    }

    // behavior[verify window.interaction.selection.linear]
    #[test]
    fn pending_terminal_selection_materializes_after_pointer_movement() {
        let action = update_pending_terminal_selection_action(
            PendingTerminalSelection {
                origin: ClientPoint::new(10, 20),
                anchor: TerminalCellPoint::new(2, 3),
                mode: TerminalSelectionMode::Linear,
            },
            ClientPoint::new(11, 20),
            true,
            Some(TerminalCellPoint::new(4, 5)),
        );

        assert_eq!(
            action,
            PendingTerminalSelectionAction::Update(TerminalSelection::new(
                TerminalCellPoint::new(2, 3),
                TerminalCellPoint::new(4, 5),
                TerminalSelectionMode::Linear,
            ))
        );
    }

    // behavior[verify window.interaction.selection.click-dismiss]
    #[test]
    fn pending_terminal_selection_completion_keeps_clicks_from_creating_single_cell_selection() {
        let selection = complete_pending_terminal_selection(
            PendingTerminalSelection {
                origin: ClientPoint::new(10, 20),
                anchor: TerminalCellPoint::new(2, 3),
                mode: TerminalSelectionMode::Linear,
            },
            ClientPoint::new(10, 20),
            Some(TerminalCellPoint::new(2, 3)),
        );

        assert_eq!(selection, None);
    }

    // behavior[verify window.interaction.selection.click-dismiss]
    #[test]
    fn pending_terminal_selection_completion_uses_release_point_after_drag() {
        let selection = complete_pending_terminal_selection(
            PendingTerminalSelection {
                origin: ClientPoint::new(10, 20),
                anchor: TerminalCellPoint::new(2, 3),
                mode: TerminalSelectionMode::Block,
            },
            ClientPoint::new(18, 26),
            Some(TerminalCellPoint::new(6, 7)),
        );

        assert_eq!(
            selection,
            Some(TerminalSelection::new(
                TerminalCellPoint::new(2, 3),
                TerminalCellPoint::new(6, 7),
                TerminalSelectionMode::Block,
            ))
        );
    }

    // behavior[verify window.interaction.selection.drag-auto-scroll]
    #[test]
    fn selection_autoscroll_velocity_scales_with_overshoot() {
        assert_eq!(scroll_lines_for_overshoot(1, 16), 1);
        assert_eq!(scroll_lines_for_overshoot(16, 16), 2);
        assert!(scroll_lines_for_overshoot(160, 16) > scroll_lines_for_overshoot(16, 16));
    }

    #[test]
    fn terminal_scrollbar_thumb_reaches_track_end_at_max_offset() {
        let rect = ClientRect::new(0, 0, 16, 100);
        let geometry = terminal_scrollbar_geometry(
            rect,
            TerminalDisplayScrollbar {
                total: 200,
                offset: 150,
                visible: 50,
            },
        )
        .expect("scrollbar geometry should exist");

        assert_eq!(geometry.thumb_rect.bottom(), rect.bottom());
    }

    #[test]
    fn terminal_scrollbar_pointer_mapping_clamps_to_scrollable_range() {
        let rect = ClientRect::new(0, 0, 16, 100);
        let geometry = terminal_scrollbar_geometry(
            rect,
            TerminalDisplayScrollbar {
                total: 200,
                offset: 60,
                visible: 50,
            },
        )
        .expect("scrollbar geometry should exist");

        let top_offset = terminal_scrollbar_offset_for_pointer(
            rect,
            geometry,
            ClientPoint::new(8, -20),
            geometry.thumb_height / 2,
        )
        .expect("top pointer should map");
        let bottom_offset = terminal_scrollbar_offset_for_pointer(
            rect,
            geometry,
            ClientPoint::new(8, 140),
            geometry.thumb_height / 2,
        )
        .expect("bottom pointer should map");

        assert_eq!(top_offset, 0);
        assert_eq!(bottom_offset, geometry.max_offset);
    }

    // behavior[verify window.interaction.drag]
    #[test]
    fn system_drag_message_targets_caption_with_screen_coordinates() {
        let (wparam, lparam) = system_drag_message(ScreenPoint::new(300, 400)).unwrap();

        assert_eq!(wparam.0, usize::try_from(HTCAPTION).unwrap());
        assert_eq!(lparam.0, ScreenPoint::new(300, 400).pack_lparam().unwrap());
    }

    // behavior[verify window.interaction.drag.live]
    // behavior[verify window.interaction.resize.live]
    // behavior[verify window.interaction.resize.terminal-live-output]
    // behavior[verify window.interaction.resize.low-latency]
    #[test]
    fn timer_render_path_stays_active_only_during_move_size() {
        assert!(!should_render_from_poll_timer(false));
        assert!(should_render_from_poll_timer(true));
    }

    // behavior[verify window.appearance.terminal.cursor.legible-block]
    #[test]
    fn block_cursor_overlay_is_translucent() {
        let color =
            terminal_cursor_overlay_color([0.8, 0.9, 1.0, 1.0], TerminalDisplayCursorStyle::Block);

        assert_eq!(color, [0.8, 0.9, 1.0, 0.42]);
    }

    // behavior[verify window.interaction.clipboard.right-click-copy-selection]
    // behavior[verify window.interaction.clipboard.right-click-paste]
    // behavior[verify window.interaction.clipboard.right-click-paste.confirm-multiline]
    #[test]
    fn right_click_terminal_action_prefers_copy_then_paste_then_confirm() {
        assert_eq!(
            right_click_terminal_action(true, "ignored"),
            Some(RightClickTerminalAction::CopySelection)
        );
        assert_eq!(
            right_click_terminal_action(false, "single line"),
            Some(RightClickTerminalAction::Paste)
        );
        assert_eq!(
            right_click_terminal_action(false, "first\nsecond"),
            Some(RightClickTerminalAction::ConfirmPaste)
        );
        assert_eq!(right_click_terminal_action(false, ""), None);
    }
}

fn should_override_drag_cursor(in_move_size_loop: bool) -> bool {
    !in_move_size_loop
}

fn should_render_from_poll_timer(in_move_size_loop: bool) -> bool {
    in_move_size_loop
}
