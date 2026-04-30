use std::cell::RefCell;
use std::cmp::Ordering;
use std::ffi::c_void;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};
#[cfg(feature = "tracy")]
use tracing::debug_span;
use tracing::trace;

use chrono::Utc;
use eyre::Context;
use facet::Facet;
use facet_pretty::FacetPretty;
use rfd::{FileDialog, MessageButtons, MessageDialog, MessageLevel};
use tracing::{debug, error, info, info_span, instrument};
use uom::si::f64::Time;
use uom::si::time::{nanosecond, second};
use widestring::U16CString;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CreateFontIndirectW, DeleteObject, EndPaint, GetDC,
    GetDeviceCaps, GetMonitorInfoW, GetTextExtentPoint32W, HFONT, LOGFONTW, MONITOR_FROM_FLAGS,
    MONITORINFO, MonitorFromWindow, PAINTSTRUCT, ReleaseDC, SelectObject, VREFRESH,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::UI::Controls::{
    TOOLTIPS_CLASSW, TTF_ABSOLUTE, TTF_TRACK, TTM_ADDTOOLW, TTM_SETMAXTIPWIDTH, TTM_TRACKACTIVATE,
    TTM_TRACKPOSITION, TTM_UPDATETIPTEXTW, TTS_ALWAYSTIP, TTS_NOPREFIX, TTTOOLINFOW,
};
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_ADD, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_LBUTTON, VK_LEFT, VK_MENU,
    VK_OEM_MINUS, VK_OEM_PLUS, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_SUBTRACT, VK_TAB, VK_UP,
};
use windows::Win32::UI::Shell::{
    ITaskbarList3, TBPF_ERROR, TBPF_INDETERMINATE, TBPF_NOPROGRESS, TBPF_NORMAL, TBPF_PAUSED,
    TBPFLAG, TaskbarList,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows, GetClassNameW,
    GetClientRect, GetCursorPos, GetMessageW, GetSystemMetrics, GetWindowRect,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, HTCAPTION, HTCLIENT,
    HTTRANSPARENT, HWND_NOTOPMOST, HWND_TOPMOST, IDC_ARROW, IDC_CROSS, IDC_HAND, IDC_HELP,
    IDC_IBEAM, IDC_SIZEALL, IDC_SIZEWE, IDC_WAIT, IsWindowVisible, IsZoomed, KillTimer,
    LoadCursorW, MSG, MoveWindow, PostMessageW, PostQuitMessage, RegisterClassExW,
    SM_CXPADDEDBORDER, SM_CXSCREEN, SM_CXSIZEFRAME, SM_CXVIRTUALSCREEN, SM_CYSCREEN,
    SM_CYSIZEFRAME, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_HIDE, SW_MAXIMIZE,
    SW_MINIMIZE, SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SYSTEM_METRICS_INDEX, SendMessageW, SetCursor, SetCursorPos, SetTimer, SetWindowPos,
    SetWindowTextW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CHAR,
    WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_ENTERSIZEMOVE, WM_ERASEBKGND, WM_EXITSIZEMOVE,
    WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_NCCALCSIZE, WM_NCHITTEST, WM_NCLBUTTONDOWN, WM_PAINT, WM_RBUTTONDOWN, WM_RBUTTONUP,
    WM_SETCURSOR, WM_SETFOCUS, WM_SIZE, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WNDCLASSEXW,
    WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_THICKFRAME, WS_VISIBLE,
};
use windows::core::{BOOL, PCWSTR, w};

use crate::logs::{self, LogRecordSnapshot, ThreadBuilderSpanExt};
use crate::model::KNOWN_WHISPER_MODELS;
use crate::paths::{AppHome, CacheHome};
use crate::timeline::{
    TimelineDataset, TimelineDocument, TimelineGroupingMode, TimelineInstantNs, TimelineItemKind,
    TimelinePlaygroundDetail, TimelineRenderPlan, TimelineRenderRowKey, TimelineSyntheticConfig,
    TimelineTimeNs, TimelineTimeRangeNs, TimelineTrackId, TimelineViewport, TimelineViewportQuery,
    generate_synthetic_timeline_dataset, timeline_playground_detail_for_render_item,
};
use crate::win32_support::clipboard::{read_clipboard, write_clipboard};
use crate::win32_support::module::get_current_module;
use crate::win32_support::string::{EasyPCWSTR, PWSTRBuffer};

use super::cell_grid;
use super::spatial::{
    ClientPoint, ClientRect, ScreenPoint, ScreenRect, ScreenToClientTransform, TerminalCellPoint,
    classify_resize_border_hit, drag_threshold_exceeded,
};
use super::vt_types::key;
use super::windows_audio::{
    BellSource, current_bell_source, current_bell_source_label, initialize_bell_source,
    ring_terminal_bell, set_bell_source,
};
use super::windows_audio_input::{
    AudioInputDeviceSummary, AudioInputDeviceWindowState, AudioInputPickerKey,
    AudioInputPickerKeyResult, AudioInputPickerState, AudioInputTimelineHeadKind,
    list_active_audio_input_devices, open_legacy_recording_devices_dialog,
};
use super::windows_cursor_info::{CursorInfoConfig, CursorInfoVirtualSession};
use super::windows_d3d12_renderer::{
    ButtonVisualState, RenderFrameModel, RenderScene, RenderThreadProxy,
    RendererTerminalVisualState, WindowChromeButtonsState,
};
use super::windows_demo_mode::{
    current_demo_mode_state, initialize_demo_mode_state, set_scramble_input_device_identifiers,
};
use super::windows_dialogs::{
    PasteConfirmationChoice, paste_confirmation_required, show_multiline_paste_confirmation_dialog,
};
use super::windows_scene::{self, ClickState, SceneAction, SceneWindowKind};
use super::windows_terminal::{
    POLL_INTERVAL_MS, POLL_TIMER_ID, TERMINAL_WORKER_WAKE_MESSAGE, TerminalChromeState,
    TerminalDisplayCursorStyle, TerminalDisplayScrollbar, TerminalDisplayState, TerminalLayout,
    TerminalPerformanceSnapshot, TerminalProgressState, TerminalSelection, TerminalSelectionMode,
    TerminalSession, keyboard_mods,
};
use super::{TerminalThroughputBenchmarkMode, TerminalWindowSummary, VtEngineChoice};

unsafe extern "system" {
    fn SetCapture(hwnd: HWND) -> HWND;
    fn ReleaseCapture() -> i32;
}

const TERMINAL_WINDOW_CLASS_NAME: &str = "TeamyStudioTerminalWindow";
const WINDOW_CLASS_NAME: PCWSTR = w!("TeamyStudioTerminalWindow");
const SCENE_WINDOW_CLASS_NAME: PCWSTR = w!("TeamyStudioSceneWindow");
const BENCHMARK_WINDOW_CLASS_NAME: PCWSTR = w!("TeamyStudioTerminalBenchmarkWindow");
const TOAST_WINDOW_CLASS_NAME: PCWSTR = w!("TeamyStudioToastWindow");
const WINDOW_TITLE: &str = "Teamy Studio Terminal";
const TERMINAL_FONT_HEIGHT: i32 = -16;
const DIAGNOSTIC_FONT_HEIGHT: i32 = -16;
const FONT_FAMILY: &str = "CaskaydiaCove Nerd Font Mono";
const MIN_FONT_HEIGHT: i32 = -12;
const MAX_FONT_HEIGHT: i32 = -72;
const FONT_ZOOM_STEP: i32 = 2;
const WINDOW_RESIZE_STEP_COLS: i32 = 4;
const WINDOW_RESIZE_STEP_ROWS: i32 = 2;
const MIN_WINDOW_CLIENT_WIDTH: i32 = 320;
const MIN_WINDOW_CLIENT_HEIGHT: i32 = 240;
const INITIAL_WINDOW_WIDTH: i32 = 1300;
const INITIAL_WINDOW_HEIGHT: i32 = 900;
const DRAG_START_THRESHOLD_PX: i32 = 0;
const MIN_RESIZE_BORDER_THICKNESS: i32 = 1;
const MOUSE_WHEEL_DELTA: i16 = 120;
const TERMINAL_WHEEL_SCROLL_LINES: isize = 3;
const SELECTION_AUTO_SCROLL_MAX_LINES: isize = 12;
const FOCUSED_RENDER_TIMER_ID: usize = 2;
const TOAST_RENDER_TIMER_ID: usize = 3;
const TOAST_RENDER_INTERVAL_MS: u32 = 16;
const USER_DEFAULT_SCREEN_DPI: u32 = 96;
const TERMINAL_THROUGHPUT_BENCHMARK_START_MARKER: &str = "__TEAMY_TERMINAL_THROUGHPUT_START__";
const TERMINAL_THROUGHPUT_BENCHMARK_DONE_MARKER: &str = "__TEAMY_TERMINAL_THROUGHPUT_DONE__";
const TERMINAL_THROUGHPUT_BENCHMARK_MEASURE_PREFIX: &str =
    "__TEAMY_TERMINAL_THROUGHPUT_MEASURE_MS=";
const TERMINAL_THROUGHPUT_BENCHMARK_TIMEOUT: Duration = Duration::from_mins(1);
const TERMINAL_THROUGHPUT_BENCHMARK_POLL_INTERVAL: Duration = Duration::from_millis(1);
const TIMELINE_ZOOM_ANIMATION_DURATION: Duration = Duration::from_millis(220);
const TIMELINE_PLAYGROUND_ROW_ANIMATION_DURATION: Duration = Duration::from_millis(180);
const TIMELINE_PLAYGROUND_LIVE_INITIAL_DURATION_NS: i64 = 30_000_000_000;
const MODEL_WARNING_PREPARE_HOLD_DURATION: Duration = Duration::from_millis(1400);
const LOG_TOAST_DURATION: Duration = Duration::from_secs(5);
const LOG_TOAST_FADE_DURATION: Duration = Duration::from_millis(260);
const LOG_TOAST_WIDTH: i32 = 420;
const LOG_TOAST_HEIGHT: i32 = 60;
const LOG_TOAST_GAP: i32 = 10;
const LOG_TOAST_HOST_PADDING: i32 = 10;
const TERMINAL_THROUGHPUT_RESULTS_DIR: &str = "self-test/terminal-throughput";
const DEMO_MODE_STATE_CHANGED_MESSAGE: u32 = WM_APP + 0x402;
const TIMELINE_DOCUMENT_CHANGED_MESSAGE: u32 = WM_APP + 0x403;
const TIMELINE_DOCUMENT_COMMAND_MESSAGE: u32 = WM_APP + 0x404;
const TIMELINE_TRANSCRIPTION_WORKER_COMPLETED_MESSAGE: u32 = WM_APP + 0x405;
const TIMELINE_PLAYGROUND_DETAIL_CHANGED_MESSAGE: u32 = WM_APP + 0x406;

#[derive(Clone, Debug)]
enum TimelineDocumentCommand {
    ApplyTranscriptionSettings(windows_scene::TimelineTranscriptionSettingsViewState),
    AppendTextTrackForTranscriptionSettings {
        track_id: TimelineTrackId,
    },
    AppendMicrophoneTrackForTranscriptionSettings {
        track_id: TimelineTrackId,
        device_name: String,
        device_id: String,
    },
    SetTranscriptionAutomation {
        track_id: TimelineTrackId,
        advance_boundaries: bool,
        submit_chunks: bool,
    },
}

thread_local! {
    static APP_STATE: RefCell<Option<AppState>> = const { RefCell::new(None) };
    static SCENE_APP_STATE: RefCell<Option<SceneAppState>> = const { RefCell::new(None) };
}

fn scene_window_registry() -> &'static Mutex<Vec<isize>> {
    static SCENE_WINDOW_REGISTRY: OnceLock<Mutex<Vec<isize>>> = OnceLock::new();

    SCENE_WINDOW_REGISTRY.get_or_init(|| Mutex::new(Vec::new()))
}

fn register_scene_window(hwnd: WindowHandle) {
    if let Ok(mut windows) = scene_window_registry().lock() {
        let raw = hwnd.raw().0 as isize;
        if !windows.contains(&raw) {
            windows.push(raw);
        }
    }
}

fn unregister_scene_window(hwnd: WindowHandle) {
    if let Ok(mut windows) = scene_window_registry().lock() {
        let raw = hwnd.raw().0 as isize;
        windows.retain(|window| *window != raw);
    }
}

fn broadcast_demo_mode_state_changed() {
    // windowing[impl demo-mode.live-audio-device-scramble]
    let Ok(windows) = scene_window_registry().lock() else {
        return;
    };

    for raw in windows.iter().copied() {
        let hwnd = HWND(raw as *mut c_void);
        // Safety: the registry stores HWND values for live scene windows and stale entries are tolerated by PostMessageW.
        let _ = unsafe {
            PostMessageW(
                Some(hwnd),
                DEMO_MODE_STATE_CHANGED_MESSAGE,
                WPARAM(0),
                LPARAM(0),
            )
        };
    }
}

fn broadcast_timeline_document_changed() {
    let Ok(windows) = scene_window_registry().lock() else {
        return;
    };

    for raw in windows.iter().copied() {
        let hwnd = HWND(raw as *mut c_void);
        // Safety: the registry stores HWND values for live scene windows and stale entries are tolerated by PostMessageW.
        let _ = unsafe {
            PostMessageW(
                Some(hwnd),
                TIMELINE_DOCUMENT_CHANGED_MESSAGE,
                WPARAM(0),
                LPARAM(0),
            )
        };
    }
}

#[derive(Default)]
struct SceneWindowInitialization {
    audio_input_device_window: Option<AudioInputDeviceWindowState>,
    timeline_document: Option<TimelineDocument>,
    timeline_transcription_settings: Option<windows_scene::TimelineTranscriptionSettingsViewState>,
    model_warning: Option<windows_scene::ModelWarningViewState>,
    timeline_playground_detail: Option<TimelinePlaygroundDetailWindowHandle>,
    initial_position: Option<ScreenRect>,
    timeline_document_command_sender: Option<mpsc::Sender<TimelineDocumentCommand>>,
    timeline_document_command_target: Option<isize>,
}

#[derive(Clone, Debug)]
struct TimelinePlaygroundDetailWindowHandle {
    shared: Arc<Mutex<TimelinePlaygroundDetailWindowState>>,
    hwnd: Arc<Mutex<Option<isize>>>,
}

impl TimelinePlaygroundDetailWindowHandle {
    fn new(detail: TimelinePlaygroundDetail, pinned: bool) -> Self {
        Self {
            shared: Arc::new(Mutex::new(TimelinePlaygroundDetailWindowState {
                detail: Some(detail),
                pinned,
            })),
            hwnd: Arc::new(Mutex::new(None)),
        }
    }

    fn update(&self, detail: TimelinePlaygroundDetail) {
        if let Ok(mut state) = self.shared.lock() {
            state.detail = Some(detail);
        }
        if let Ok(hwnd) = self.hwnd.lock()
            && let Some(raw) = *hwnd
        {
            let hwnd = HWND(raw as *mut c_void);
            // Safety: the stored HWND belongs to a Teamy scene window; stale handles are tolerated by PostMessageW.
            let _ = unsafe {
                PostMessageW(
                    Some(hwnd),
                    TIMELINE_PLAYGROUND_DETAIL_CHANGED_MESSAGE,
                    WPARAM(0),
                    LPARAM(0),
                )
            };
        }
    }
}

#[derive(Clone, Debug)]
struct TimelinePlaygroundDetailWindowState {
    detail: Option<TimelinePlaygroundDetail>,
    pinned: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TimelinePlaygroundZoomAnimation {
    start_visible_start_ns: i64,
    start_visible_end_ns: i64,
    target_visible_start_ns: i64,
    target_visible_end_ns: i64,
    started_at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TimelinePlaygroundSourceMode {
    Synthetic,
    LiveTracingEvents,
}

#[derive(Debug)]
struct TimelinePlaygroundState {
    seed: u64,
    dataset: TimelineDataset,
    grouping_mode: TimelineGroupingMode,
    minimum_visible_pixels: u32,
    visible_start_ns: i64,
    visible_end_ns: i64,
    vertical_scroll_offset: i32,
    source_mode: TimelinePlaygroundSourceMode,
    live_tracing_follow_tail: bool,
    zoom_animation: Option<TimelinePlaygroundZoomAnimation>,
    row_position_animation: Option<TimelinePlaygroundRowPositionAnimation>,
    last_row_positions: Vec<(TimelineRenderRowKey, i32)>,
    hovered_item: Option<windows_scene::TimelinePlaygroundHitTarget>,
    hover_detail_window: Option<TimelinePlaygroundDetailWindowHandle>,
}

#[derive(Clone, Debug)]
struct TimelinePlaygroundRowPositionAnimation {
    start_positions: Vec<(TimelineRenderRowKey, i32)>,
    target_positions: Vec<(TimelineRenderRowKey, i32)>,
    started_at: Instant,
}

impl TimelinePlaygroundState {
    fn new() -> eyre::Result<Self> {
        let seed = TimelineSyntheticConfig::default().seed();
        let dataset = generate_synthetic_timeline_dataset(
            &TimelineSyntheticConfig::default().with_seed(seed),
        )?;
        Ok(Self {
            seed,
            dataset,
            grouping_mode: TimelineGroupingMode::GroupKey,
            minimum_visible_pixels: 4,
            visible_start_ns: 0,
            visible_end_ns: 24_000_000,
            vertical_scroll_offset: 0,
            source_mode: TimelinePlaygroundSourceMode::Synthetic,
            live_tracing_follow_tail: false,
            zoom_animation: None,
            row_position_animation: None,
            last_row_positions: Vec::new(),
            hovered_item: None,
            hover_detail_window: None,
        })
    }

    fn regenerate(&mut self) -> eyre::Result<()> {
        self.source_mode = TimelinePlaygroundSourceMode::Synthetic;
        self.live_tracing_follow_tail = false;
        self.seed = self.seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
        self.dataset = generate_synthetic_timeline_dataset(
            &TimelineSyntheticConfig::default().with_seed(self.seed),
        )?;
        self.zoom_animation = None;
        self.row_position_animation = None;
        self.last_row_positions.clear();
        self.hovered_item = None;
        self.vertical_scroll_offset = 0;
        Ok(())
    }

    fn view_state(
        &self,
        cursor_position: Option<ClientPoint>,
    ) -> windows_scene::TimelinePlaygroundViewState {
        windows_scene::TimelinePlaygroundViewState {
            source_label: match self.source_mode {
                TimelinePlaygroundSourceMode::Synthetic => "synthetic",
                TimelinePlaygroundSourceMode::LiveTracingEvents => "live tracing events",
            },
            seed: self.seed,
            grouping_mode: self.grouping_mode,
            minimum_visible_pixels: self.minimum_visible_pixels,
            visible_start_ns: self.visible_start_ns,
            visible_end_ns: self.visible_end_ns,
            vertical_scroll_offset: self.vertical_scroll_offset,
            hovered_item: self.hovered_item,
            cursor_position,
        }
    }

    fn toggle_live_tracing_events(&mut self) {
        match self.source_mode {
            TimelinePlaygroundSourceMode::Synthetic => {
                self.source_mode = TimelinePlaygroundSourceMode::LiveTracingEvents;
                self.live_tracing_follow_tail = true;
                self.visible_start_ns = 0;
                self.visible_end_ns = TIMELINE_PLAYGROUND_LIVE_INITIAL_DURATION_NS;
                self.sync_live_tracing_events();
            }
            TimelinePlaygroundSourceMode::LiveTracingEvents => {
                self.source_mode = TimelinePlaygroundSourceMode::Synthetic;
                self.live_tracing_follow_tail = false;
            }
        }
        self.hovered_item = None;
        self.vertical_scroll_offset = 0;
        self.zoom_animation = None;
        self.row_position_animation = None;
        self.last_row_positions.clear();
    }

    fn sync_live_tracing_events(&mut self) {
        if self.source_mode != TimelinePlaygroundSourceMode::LiveTracingEvents {
            return;
        }
        let (dataset, latest_at_ns) = logs::tracing_event_timeline_dataset();
        self.dataset = dataset;
        if self.live_tracing_follow_tail {
            let duration = (self.visible_end_ns - self.visible_start_ns).max(1);
            self.visible_end_ns = latest_at_ns.max(1);
            self.visible_start_ns = self.visible_end_ns.saturating_sub(duration);
        }
    }

    // timeline[impl playground.vertical-pan-clamp]
    fn clamp_vertical_scroll_offset(
        &mut self,
        layout: windows_scene::TimelinePlaygroundLayout,
        row_count: usize,
    ) {
        self.vertical_scroll_offset =
            windows_scene::timeline_playground_clamp_vertical_scroll_offset(
                layout,
                row_count,
                self.vertical_scroll_offset,
            );
    }

    fn update_row_position_animation(&mut self, render_plan: &TimelineRenderPlan) {
        // timeline[impl playground.row-transition-animation]
        let target_positions = timeline_playground_row_positions(render_plan);
        if self.last_row_positions.is_empty() {
            self.last_row_positions = target_positions;
            return;
        }
        if self.last_row_positions == target_positions {
            return;
        }
        let start_positions = self.row_position_animation.as_ref().map_or_else(
            || self.last_row_positions.clone(),
            Self::row_visual_positions_from_animation,
        );
        self.row_position_animation = Some(TimelinePlaygroundRowPositionAnimation {
            start_positions,
            target_positions: target_positions.clone(),
            started_at: Instant::now(),
        });
        self.last_row_positions = target_positions;
    }

    fn row_visual_positions(
        &self,
        render_plan: &TimelineRenderPlan,
    ) -> Vec<(TimelineRenderRowKey, i32)> {
        let Some(animation) = &self.row_position_animation else {
            return timeline_playground_row_positions(render_plan);
        };
        Self::row_visual_positions_from_animation(animation)
    }

    fn row_visual_positions_from_animation(
        animation: &TimelinePlaygroundRowPositionAnimation,
    ) -> Vec<(TimelineRenderRowKey, i32)> {
        let progress = (animation.started_at.elapsed().as_secs_f64()
            / TIMELINE_PLAYGROUND_ROW_ANIMATION_DURATION.as_secs_f64())
        .clamp(0.0, 1.0);
        if progress >= 1.0 {
            return animation.target_positions.clone();
        }
        let progress = ease_in_out(progress);
        animation
            .target_positions
            .iter()
            .map(|(key, target_top)| {
                let start_top = animation
                    .start_positions
                    .iter()
                    .find_map(|(start_key, top)| (*start_key == *key).then_some(*top))
                    .unwrap_or(*target_top);
                (*key, interpolate_i32(start_top, *target_top, progress))
            })
            .collect()
    }

    fn apply_row_position_animation(&mut self) {
        let Some(animation) = &self.row_position_animation else {
            return;
        };
        let progress = animation.started_at.elapsed().as_secs_f64()
            / TIMELINE_PLAYGROUND_ROW_ANIMATION_DURATION.as_secs_f64();
        if progress >= 1.0 {
            self.row_position_animation = None;
        }
    }

    fn query(&self, viewport_width_pixels: u32) -> eyre::Result<TimelineViewportQuery> {
        TimelineViewportQuery::try_new(
            TimelineInstantNs::new(self.visible_start_ns),
            TimelineInstantNs::new(self.visible_end_ns),
            TimelineInstantNs::new(self.visible_end_ns),
            viewport_width_pixels,
        )
        .map(|query| {
            query
                .with_grouping_mode(self.grouping_mode)
                .with_minimum_visible_pixels(self.minimum_visible_pixels)
        })
    }

    fn pan(&mut self, direction: i64) {
        // timeline[impl playground.pan-negative-time]
        // timeline[impl playground.pan-button-snap-item]
        self.live_tracing_follow_tail = false;
        self.cancel_zoom_animation_at_current_range();
        let duration = visible_duration_ns_i64(self.visible_start_ns, self.visible_end_ns);
        if let Some(target_time_ns) = self.pan_snap_target(direction) {
            self.center_on_time(target_time_ns, duration);
            return;
        }
        let delta = duration / 4 * direction;
        self.visible_start_ns = self.visible_start_ns.saturating_add(delta);
        self.visible_end_ns = self
            .visible_end_ns
            .saturating_add(delta)
            .max(self.visible_start_ns.saturating_add(1));
    }

    fn fit_to_content(&mut self) {
        // timeline[impl playground.fit-content]
        self.live_tracing_follow_tail = false;
        self.zoom_animation = None;
        self.hovered_item = None;
        let Some((start, end)) = timeline_playground_dataset_time_bounds(&self.dataset) else {
            return;
        };
        let content_duration = visible_duration_ns_i64(start, end);
        let padding = (content_duration / 10).max(1);
        self.visible_start_ns = start.saturating_sub(padding);
        self.visible_end_ns = end
            .saturating_add(padding)
            .max(self.visible_start_ns.saturating_add(1));
    }

    fn pan_snap_target(&self, direction: i64) -> Option<i64> {
        if timeline_playground_dataset_intersects_range(
            &self.dataset,
            self.visible_start_ns,
            self.visible_end_ns,
        ) {
            return None;
        }
        match direction.cmp(&0) {
            Ordering::Less => {
                timeline_playground_nearest_item_at_or_before(&self.dataset, self.visible_start_ns)
            }
            Ordering::Greater => {
                timeline_playground_nearest_item_at_or_after(&self.dataset, self.visible_end_ns)
            }
            Ordering::Equal => None,
        }
    }

    fn center_on_time(&mut self, time_ns: i64, duration: i64) {
        let half_duration = duration / 2;
        self.visible_start_ns = time_ns.saturating_sub(half_duration);
        self.visible_end_ns = self
            .visible_start_ns
            .saturating_add(duration)
            .max(self.visible_start_ns.saturating_add(1));
    }

    fn zoom(&mut self, numerator: i64, denominator: i64) {
        self.zoom_about_ratio(1, 2, numerator, denominator);
    }

    fn zoom_about_pixels(
        &mut self,
        anchor_x_pixels: i32,
        viewport_width_pixels: i32,
        numerator: i64,
        denominator: i64,
    ) {
        self.zoom_about_ratio(
            i128::from(anchor_x_pixels.clamp(0, viewport_width_pixels.max(1))),
            i128::from(viewport_width_pixels.max(1)),
            numerator,
            denominator,
        );
    }

    fn zoom_about_ratio(
        &mut self,
        anchor_numerator: i128,
        anchor_denominator: i128,
        numerator: i64,
        denominator: i64,
    ) {
        // timeline[impl playground.zoom-compounds]
        self.live_tracing_follow_tail = false;
        let (animation_start_ns, animation_end_ns) = self.current_visible_range();
        let (base_start_ns, base_end_ns) =
            self.zoom_animation
                .map_or((self.visible_start_ns, self.visible_end_ns), |animation| {
                    (
                        animation.target_visible_start_ns,
                        animation.target_visible_end_ns,
                    )
                });
        self.visible_start_ns = animation_start_ns;
        self.visible_end_ns = animation_end_ns;
        let current_duration = visible_duration_ns_i64(base_start_ns, base_end_ns);
        let duration = i64::try_from(
            (i128::from(current_duration) * i128::from(numerator.max(1))
                / i128::from(denominator.max(1)))
            .max(1),
        )
        .unwrap_or(i64::MAX);
        let anchor_denominator = anchor_denominator.max(1);
        let anchor_offset =
            i64::try_from(i128::from(current_duration) * anchor_numerator / anchor_denominator)
                .unwrap_or(i64::MAX);
        let target_anchor_offset =
            i64::try_from(i128::from(duration) * anchor_numerator / anchor_denominator)
                .unwrap_or(i64::MAX);
        let anchor_time = base_start_ns.saturating_add(anchor_offset);
        let target_start = anchor_time.saturating_sub(target_anchor_offset);
        let target_end = target_start
            .saturating_add(duration)
            .max(target_start.saturating_add(1));
        self.zoom_animation = Some(TimelinePlaygroundZoomAnimation {
            start_visible_start_ns: self.visible_start_ns,
            start_visible_end_ns: self.visible_end_ns,
            target_visible_start_ns: target_start,
            target_visible_end_ns: target_end,
            started_at: Instant::now(),
        });
        self.hovered_item = None;
    }

    fn current_visible_range(&self) -> (i64, i64) {
        self.zoom_animation
            .map_or((self.visible_start_ns, self.visible_end_ns), |animation| {
                let progress = (animation.started_at.elapsed().as_secs_f64()
                    / TIMELINE_ZOOM_ANIMATION_DURATION.as_secs_f64())
                .clamp(0.0, 1.0);
                if progress >= 1.0 {
                    return (
                        animation.target_visible_start_ns,
                        animation.target_visible_end_ns,
                    );
                }
                interpolate_timeline_playground_range(animation, ease_in_out(progress))
            })
    }

    fn apply_zoom_animation(&mut self) {
        let Some(animation) = self.zoom_animation else {
            return;
        };
        let progress = (animation.started_at.elapsed().as_secs_f64()
            / TIMELINE_ZOOM_ANIMATION_DURATION.as_secs_f64())
        .clamp(0.0, 1.0);
        if progress >= 1.0 {
            self.visible_start_ns = animation.target_visible_start_ns;
            self.visible_end_ns = animation.target_visible_end_ns;
            self.zoom_animation = None;
            return;
        }
        let (visible_start_ns, visible_end_ns) =
            interpolate_timeline_playground_range(animation, ease_in_out(progress));
        self.visible_start_ns = visible_start_ns;
        self.visible_end_ns = visible_end_ns;
    }

    fn cancel_zoom_animation_at_current_range(&mut self) {
        let (visible_start_ns, visible_end_ns) = self.current_visible_range();
        self.visible_start_ns = visible_start_ns;
        self.visible_end_ns = visible_end_ns;
        self.zoom_animation = None;
    }
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "window interaction state is tracked independently for input routing"
)]
struct AppState {
    hwnd: Option<WindowHandle>,
    dpi: u32,
    launch_title: Option<String>,
    terminal_chrome: TerminalChromeState,
    last_applied_window_title: String,
    taskbar_progress: TaskbarProgressController,
    pointer_position: Option<ClientPoint>,
    pending_window_drag: Option<PendingWindowDrag>,
    diagnostic_panel_visible: bool,
    diagnostic_selection: Option<TerminalSelection>,
    pending_diagnostic_selection: Option<PendingTerminalSelection>,
    diagnostic_selection_drag_point: Option<ClientPoint>,
    pressed_chrome_button: Option<WindowChromeButton>,
    pin_button_last_clicked_at: Option<Instant>,
    pinned_topmost: bool,
    diagnostics_button_last_clicked_at: Option<Instant>,
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
    diagnostic_font_height: i32,
    diagnostic_cell_width: i32,
    diagnostic_cell_height: i32,
    chrome_tooltip: ChromeTooltipController,
    terminal: HostedTerminalSession,
    renderer: Option<RenderThreadProxy>,
}

enum HostedTerminalSession {
    Pty(TerminalSession),
    CursorInfoVirtual(Box<CursorInfoVirtualSession>),
}

impl HostedTerminalSession {
    fn new_cursor_info_virtual(config: CursorInfoConfig) -> eyre::Result<Self> {
        Ok(Self::CursorInfoVirtual(Box::new(
            CursorInfoVirtualSession::new(config)?,
        )))
    }

    fn set_wake_window(&mut self, hwnd: HWND) {
        if let Self::Pty(terminal) = self {
            terminal.set_wake_window(hwnd);
        }
    }

    fn has_pending_output(&self) -> bool {
        match self {
            Self::Pty(terminal) => terminal.has_pending_output(),
            Self::CursorInfoVirtual(_) => false,
        }
    }

    fn chrome_state(&mut self) -> TerminalChromeState {
        match self {
            Self::Pty(terminal) => terminal.chrome_state(),
            Self::CursorInfoVirtual(_) => TerminalChromeState::default(),
        }
    }

    fn cached_display_state(&mut self) -> Arc<TerminalDisplayState> {
        match self {
            Self::Pty(terminal) => terminal.cached_display_state(),
            Self::CursorInfoVirtual(terminal) => terminal.cached_display_state(),
        }
    }

    fn take_repaint_requested(&mut self) -> bool {
        match self {
            Self::Pty(terminal) => terminal.take_repaint_requested(),
            Self::CursorInfoVirtual(terminal) => terminal.take_repaint_requested(),
        }
    }

    fn resize(&mut self, layout: TerminalLayout) -> eyre::Result<()> {
        match self {
            Self::Pty(terminal) => terminal.resize(layout),
            Self::CursorInfoVirtual(terminal) => {
                terminal.resize(layout);
                Ok(())
            }
        }
    }

    fn pump(&mut self) -> eyre::Result<super::windows_terminal::PumpResult> {
        match self {
            Self::Pty(terminal) => terminal.pump(),
            Self::CursorInfoVirtual(terminal) => Ok(terminal.pump()),
        }
    }

    fn poll_pty_output(&mut self) -> eyre::Result<super::windows_terminal::PollPtyOutputResult> {
        match self {
            Self::Pty(terminal) => terminal.poll_pty_output(),
            Self::CursorInfoVirtual(terminal) => terminal.poll_output(),
        }
    }

    fn handle_char(&mut self, code_unit: u32, lparam: isize) -> eyre::Result<bool> {
        let _ = lparam;
        Ok(match self {
            Self::Pty(terminal) => terminal.handle_char(code_unit, lparam)?,
            Self::CursorInfoVirtual(terminal) => terminal.handle_char(code_unit),
        })
    }

    fn handle_key_event(
        &mut self,
        vkey: u32,
        lparam: isize,
        was_down: bool,
        is_release: bool,
        mods: key::Mods,
    ) -> eyre::Result<bool> {
        let _ = (lparam, was_down, mods);
        Ok(match self {
            Self::Pty(terminal) => {
                terminal.handle_key_event(vkey, lparam, was_down, is_release, mods)?
            }
            Self::CursorInfoVirtual(terminal) => terminal.handle_key_event(vkey, is_release),
        })
    }

    fn handle_paste(&mut self, text: &str) -> eyre::Result<()> {
        match self {
            Self::Pty(terminal) => terminal.handle_paste(text),
            Self::CursorInfoVirtual(_) => Ok(()),
        }
    }

    fn note_frame_presented(&mut self) {
        if let Self::Pty(terminal) = self {
            terminal.note_frame_presented();
        }
    }

    fn selected_text(&mut self, selection: TerminalSelection) -> eyre::Result<String> {
        match self {
            Self::Pty(terminal) => terminal.selected_text(selection),
            Self::CursorInfoVirtual(terminal) => Ok(terminal.visible_text()),
        }
    }

    fn viewport_metrics(&self) -> eyre::Result<super::windows_terminal::TerminalViewportMetrics> {
        match self {
            Self::Pty(terminal) => terminal.viewport_metrics(),
            Self::CursorInfoVirtual(terminal) => Ok(terminal.viewport_metrics()),
        }
    }

    fn viewport_to_screen_cell(&self, cell: TerminalCellPoint) -> eyre::Result<TerminalCellPoint> {
        match self {
            Self::Pty(terminal) => terminal.viewport_to_screen_cell(cell),
            Self::CursorInfoVirtual(_) => Ok(cell),
        }
    }

    fn scroll_viewport_by(&mut self, delta: isize) {
        if let Self::Pty(terminal) = self {
            terminal.scroll_viewport_by(delta);
        }
    }

    fn scroll_viewport_to_offset(&mut self, offset: u64) -> eyre::Result<()> {
        match self {
            Self::Pty(terminal) => terminal.scroll_viewport_to_offset(offset),
            Self::CursorInfoVirtual(_) => Ok(()),
        }
    }

    fn mouse_reporting_enabled(&self) -> bool {
        match self {
            Self::Pty(terminal) => terminal.mouse_reporting_enabled(),
            Self::CursorInfoVirtual(_) => true,
        }
    }

    fn send_mouse_wheel(&mut self, cell: TerminalCellPoint, scroll_up: bool) -> eyre::Result<bool> {
        let _ = cell;
        Ok(match self {
            Self::Pty(terminal) => terminal.send_mouse_wheel(cell, scroll_up)?,
            Self::CursorInfoVirtual(terminal) => terminal.handle_mouse_wheel(scroll_up),
        })
    }

    fn visible_display_state_with_selection(
        &mut self,
        selection: Option<TerminalSelection>,
    ) -> eyre::Result<TerminalDisplayState> {
        let _ = selection;
        match self {
            Self::Pty(terminal) => terminal.visible_display_state_with_selection(selection),
            Self::CursorInfoVirtual(terminal) => {
                Ok(terminal.cached_display_state().as_ref().clone())
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowChromeButton {
    Pin,
    Diagnostics,
    Minimize,
    MaximizeRestore,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScenePressedTarget {
    ChromeButton(WindowChromeButton),
    Action(SceneAction),
    TimelineTransportPlayPause,
    TimelineTranscriptionSettingsTarget(windows_scene::TimelineTranscriptionSettingsTarget),
    ModelWarningPrepare,
    TimelineTrackReorderHandle(usize),
    TimelineTrackRecord(usize),
    TimelineTrackPlayback(usize),
    TimelineTrackLoopback(usize),
    TimelineTrackTranscriptionToggle(usize),
    TimelineTrackTranscriptionSettings(usize),
    TimelineAudioHead(AudioInputTimelineHeadKind),
    AudioInputDevice(usize),
    AudioInputDeviceArm,
    AudioInputTranscription,
    AudioInputTranscriptionFlush,
    AudioDaemonCudaCheck,
    AudioDaemonModel(usize),
    AudioInputPlayback,
    AudioInputDeviceLoopback,
    AudioInputTimelineHead(AudioInputTimelineHeadKind),
    LegacyRecordingDevices,
    AudioInputTimeline,
    DemoModeButton,
    DemoModeScrambleToggle,
    LogsControl(windows_scene::LogsWindowControl),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum AudioInputPickerCompletion {
    #[default]
    OpenDeviceWindow,
    TimelineMicrophoneTrack,
    TimelineTranscriptionInputTrack,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum TimelineInteractionTool {
    #[default]
    Select,
    Brush,
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "window interaction state is tracked independently for input routing"
)]
struct SceneAppState {
    app_home: AppHome,
    hwnd: Option<WindowHandle>,
    dpi: u32,
    scene_kind: SceneWindowKind,
    scene_opened_at: Instant,
    vt_engine: VtEngineChoice,
    audio_input_picker: AudioInputPickerState,
    audio_input_picker_completion: AudioInputPickerCompletion,
    audio_input_device_window: Option<AudioInputDeviceWindowState>,
    timeline_document: Option<TimelineDocument>,
    timeline_transcription_settings: Option<windows_scene::TimelineTranscriptionSettingsViewState>,
    timeline_document_command_sender: Option<mpsc::Sender<TimelineDocumentCommand>>,
    timeline_document_command_receiver: Option<mpsc::Receiver<TimelineDocumentCommand>>,
    timeline_document_command_target: Option<isize>,
    model_warning: Option<windows_scene::ModelWarningViewState>,
    model_warning_prepare_started_at: Option<Instant>,
    timeline_playground: Option<TimelinePlaygroundState>,
    timeline_playground_detail: Option<TimelinePlaygroundDetailWindowHandle>,
    timeline_tool: TimelineInteractionTool,
    timeline_selection: Option<windows_scene::TimelineRectSelection>,
    pending_timeline_selection: Option<PendingTimelineSelection>,
    pending_timeline_text_block: Option<PendingTimelineTextBlock>,
    pending_timeline_track_reorder: Option<PendingTimelineTrackReorder>,
    timeline_pan_drag: Option<TimelinePanDrag>,
    timeline_playground_pan_drag: Option<TimelinePlaygroundPanDrag>,
    timeline_zoom_animation: Option<TimelineZoomAnimation>,
    timeline_vertical_scroll_offset: i32,
    demo_mode_scramble_input_device_identifiers: DemoModeInputDeviceIdentifierScramble,
    demo_mode_scramble_toggle_last_changed_at: Option<Instant>,
    scene_action_selected_index: usize,
    scene_virtual_cursor: Option<ClientPoint>,
    pointer_position: Option<ClientPoint>,
    pressed_target: Option<ScenePressedTarget>,
    pin_button_last_clicked_at: Option<Instant>,
    pinned_topmost: bool,
    last_clicked_action: Option<ClickState<SceneAction>>,
    diagnostics_button_last_clicked_at: Option<Instant>,
    diagnostics_visible: bool,
    diagnostic_selection: Option<TerminalSelection>,
    pending_diagnostic_selection: Option<PendingTerminalSelection>,
    diagnostic_selection_drag_point: Option<ClientPoint>,
    in_move_size_loop: bool,
    window_focused: bool,
    focused_render_interval_ms: u32,
    terminal_cell_width: i32,
    terminal_cell_height: i32,
    diagnostic_cell_width: i32,
    diagnostic_cell_height: i32,
    logs_scroll_offset: usize,
    logs_follow_tail: bool,
    last_applied_scene_window_title: String,
    chrome_tooltip: ChromeTooltipController,
    renderer: Option<RenderThreadProxy>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum DemoModeInputDeviceIdentifierScramble {
    #[default]
    Off,
    On,
}

impl DemoModeInputDeviceIdentifierScramble {
    const fn from_enabled(enabled: bool) -> Self {
        if enabled { Self::On } else { Self::Off }
    }

    const fn is_enabled(self) -> bool {
        matches!(self, Self::On)
    }
}

fn sync_demo_mode_state(state: &mut SceneAppState) {
    let enabled = current_demo_mode_state().scramble_input_device_identifiers;
    let next = DemoModeInputDeviceIdentifierScramble::from_enabled(enabled);
    if state.demo_mode_scramble_input_device_identifiers != next {
        state.demo_mode_scramble_input_device_identifiers = next;
        state.demo_mode_scramble_toggle_last_changed_at = Some(Instant::now());
    }
}

fn toggle_demo_mode_scramble_input_device_identifiers(
    state: &mut SceneAppState,
) -> eyre::Result<()> {
    let enabled = !state
        .demo_mode_scramble_input_device_identifiers
        .is_enabled();
    set_scramble_input_device_identifiers(&state.app_home, enabled)?;
    state.demo_mode_scramble_input_device_identifiers =
        DemoModeInputDeviceIdentifierScramble::from_enabled(enabled);
    state.demo_mode_scramble_toggle_last_changed_at = Some(Instant::now());
    broadcast_demo_mode_state_changed();
    Ok(())
}

#[derive(Debug, Default)]
struct ChromeTooltipController {
    hwnd: Option<HWND>,
    text: PWSTRBuffer,
    active_text: String,
    active_position: Option<ScreenPoint>,
    visible: bool,
}

impl ChromeTooltipController {
    fn create(owner: WindowHandle) -> eyre::Result<Self> {
        owner.window_thread.assert_window_thread();
        let instance = get_current_module().wrap_err("failed to get module handle for tooltip")?;
        // Safety: this creates a topmost tooltip control owned by the current UI-thread window.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST,
                TOOLTIPS_CLASSW,
                PCWSTR::null(),
                WINDOW_STYLE(WS_POPUP.0 | TTS_ALWAYSTIP | TTS_NOPREFIX),
                0,
                0,
                0,
                0,
                Some(owner.raw()),
                None,
                Some(instance.into()),
                None,
            )
        }
        .wrap_err("failed to create native chrome tooltip window")?;

        let mut controller = Self {
            hwnd: Some(hwnd),
            text: PWSTRBuffer::default(),
            active_text: String::new(),
            active_position: None,
            visible: false,
        };
        let tool = controller.tool_info(owner.raw());
        let tool_ptr: *const TTTOOLINFOW = &raw const tool;
        // Safety: the tooltip control reads the provided tool descriptor for this message call only.
        unsafe {
            let _ = SendMessageW(
                hwnd,
                TTM_ADDTOOLW,
                Some(WPARAM(0)),
                Some(LPARAM(tool_ptr as isize)),
            );
        }
        // Safety: sending this configuration message to a live tooltip control is valid.
        unsafe {
            let _ = SendMessageW(hwnd, TTM_SETMAXTIPWIDTH, Some(WPARAM(0)), Some(LPARAM(320)));
        }
        Ok(controller)
    }

    fn show_at(
        &mut self,
        owner: WindowHandle,
        text: &str,
        position: ScreenPoint,
    ) -> eyre::Result<()> {
        let Some(hwnd) = self.hwnd else {
            return Ok(());
        };

        if self.is_show_request_current(text, position) {
            return Ok(());
        }

        self.text.set(text)?;
        let tool = self.tool_info(owner.raw());
        let tool_ptr: *const TTTOOLINFOW = &raw const tool;
        // Safety: the tooltip control reads the provided tool descriptor for this message call only.
        unsafe {
            let _ = SendMessageW(
                hwnd,
                TTM_UPDATETIPTEXTW,
                Some(WPARAM(0)),
                Some(LPARAM(tool_ptr as isize)),
            );
        }
        // Safety: sending a screen-space track position to a live tooltip control is valid.
        unsafe {
            let _ = SendMessageW(
                hwnd,
                TTM_TRACKPOSITION,
                Some(WPARAM(0)),
                Some(LPARAM(position.pack_lparam()?)),
            );
        }
        // Safety: activating tracking mode on a live tooltip control with a valid tool descriptor is valid.
        unsafe {
            let _ = SendMessageW(
                hwnd,
                TTM_TRACKACTIVATE,
                Some(WPARAM(1)),
                Some(LPARAM(tool_ptr as isize)),
            );
        }
        self.visible = true;
        text.clone_into(&mut self.active_text);
        self.active_position = Some(position);
        Ok(())
    }

    fn hide(&mut self, owner: WindowHandle) {
        if !self.visible {
            return;
        }
        let Some(hwnd) = self.hwnd else {
            return;
        };

        let tool = self.tool_info(owner.raw());
        let tool_ptr: *const TTTOOLINFOW = &raw const tool;
        // Safety: deactivating tracking mode on a live tooltip control with a valid tool descriptor is valid.
        unsafe {
            let _ = SendMessageW(
                hwnd,
                TTM_TRACKACTIVATE,
                Some(WPARAM(0)),
                Some(LPARAM(tool_ptr as isize)),
            );
        }
        self.visible = false;
        self.active_text.clear();
        self.active_position = None;
    }

    fn destroy(&mut self) {
        if let Some(hwnd) = self.hwnd.take() {
            // Safety: this destroys the tooltip control owned by the current window before teardown.
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
        }
        self.visible = false;
        self.active_text.clear();
        self.active_position = None;
    }

    // timeline[impl playground.hover-title-tooltip-stable]
    fn is_show_request_current(&self, text: &str, position: ScreenPoint) -> bool {
        self.visible && self.active_text == text && self.active_position == Some(position)
    }

    fn tool_info(&mut self, owner: HWND) -> TTTOOLINFOW {
        TTTOOLINFOW {
            cbSize: u32::try_from(std::mem::size_of::<TTTOOLINFOW>())
                .expect("TTTOOLINFOW size must fit in u32"),
            uFlags: TTF_TRACK | TTF_ABSOLUTE,
            hwnd: owner,
            uId: 1,
            lpszText: self.text.as_pwstr(),
            ..Default::default()
        }
    }
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

    fn show_no_activate(self) {
        self.window_thread.assert_window_thread();
        // Safety: `self.hwnd` is a live top-level window owned by this process on `self.window_thread`.
        let _ = unsafe { ShowWindow(self.hwnd, SW_SHOWNOACTIVATE) };
    }

    fn hide(self) {
        self.window_thread.assert_window_thread();
        // Safety: `self.hwnd` is a live top-level window owned by this process on `self.window_thread`.
        let _ = unsafe { ShowWindow(self.hwnd, SW_HIDE) };
    }

    fn minimize(self) {
        self.window_thread.assert_window_thread();
        // Safety: `self.hwnd` is a live top-level window owned by this process on `self.window_thread`.
        let _ = unsafe { ShowWindow(self.hwnd, SW_MINIMIZE) };
    }

    fn maximize(self) {
        self.window_thread.assert_window_thread();
        // Safety: `self.hwnd` is a live top-level window owned by this process on `self.window_thread`.
        let _ = unsafe { ShowWindow(self.hwnd, SW_MAXIMIZE) };
    }

    fn restore(self) {
        self.window_thread.assert_window_thread();
        // Safety: `self.hwnd` is a live top-level window owned by this process on `self.window_thread`.
        let _ = unsafe { ShowWindow(self.hwnd, SW_RESTORE) };
    }

    fn is_zoomed(self) -> bool {
        self.window_thread.assert_window_thread();
        // Safety: querying the zoomed state of a live top-level window is valid.
        unsafe { IsZoomed(self.hwnd).as_bool() }
    }

    fn toggle_maximize_restore(self) {
        if self.is_zoomed() {
            self.restore();
        } else {
            self.maximize();
        }
    }

    fn set_topmost(self, enabled: bool) -> eyre::Result<()> {
        self.window_thread.assert_window_thread();
        let insert_after = if enabled {
            HWND_TOPMOST
        } else {
            HWND_NOTOPMOST
        };
        // Safety: SetWindowPos is applied to this live top-level window without moving or resizing it.
        unsafe {
            SetWindowPos(
                self.hwnd,
                Some(insert_after),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        }
        .wrap_err("failed to update window pin state")
    }

    fn set_position_no_activate(self, rect: ScreenRect) -> eyre::Result<()> {
        self.window_thread.assert_window_thread();
        // Safety: SetWindowPos moves and sizes this live top-level window without activating it.
        unsafe {
            SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                rect.left(),
                rect.top(),
                rect.width(),
                rect.height(),
                SWP_NOACTIVATE,
            )
        }
        .wrap_err("failed to position toast window")
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

    fn set_title(self, title: &str) -> eyre::Result<()> {
        self.window_thread.assert_window_thread();
        let title = title.easy_pcwstr()?;
        // Safety: `title` is a valid null-terminated UTF-16 buffer for the duration of the call.
        unsafe { SetWindowTextW(self.hwnd, title.as_ref()) }
            .wrap_err("failed to update window title")
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

#[derive(Default)]
struct TaskbarProgressController {
    taskbar: Option<ITaskbarList3>,
    initialization_attempted: bool,
    last_applied_progress: Option<TerminalProgressState>,
}

impl TaskbarProgressController {
    fn apply(&mut self, hwnd: WindowHandle, progress: TerminalProgressState) -> eyre::Result<()> {
        if self.last_applied_progress == Some(progress) {
            return Ok(());
        }

        self.ensure_initialized();
        let Some(taskbar) = self.taskbar.as_ref() else {
            self.last_applied_progress = Some(progress);
            return Ok(());
        };

        // Safety: the taskbar COM object is initialized on the UI thread and the HWND belongs to this live top-level window.
        unsafe { taskbar.SetProgressState(hwnd.raw(), taskbar_progress_flag(progress)) }
            .wrap_err("failed to update taskbar progress state")?;
        if let Some(value) = taskbar_progress_value(progress) {
            // Safety: the taskbar COM object is initialized on the UI thread and the HWND belongs to this live top-level window.
            unsafe { taskbar.SetProgressValue(hwnd.raw(), value, 100) }
                .wrap_err("failed to update taskbar progress value")?;
        }
        self.last_applied_progress = Some(progress);
        Ok(())
    }

    fn clear(&mut self, hwnd: WindowHandle) -> eyre::Result<()> {
        self.apply(hwnd, TerminalProgressState::Hidden)
    }

    fn ensure_initialized(&mut self) {
        if self.initialization_attempted {
            return;
        }

        self.initialization_attempted = true;
        // Safety: initializing COM for the UI thread before creating the taskbar COM object is required by the Win32 API.
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        // Safety: the TaskbarList class object is created on the UI thread and retained for later taskbar updates.
        let taskbar_result: windows::core::Result<ITaskbarList3> =
            unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER) };
        let Ok(taskbar) = taskbar_result else {
            return;
        };
        // Safety: the taskbar COM object has just been created on the UI thread and must be initialized before use.
        if unsafe { taskbar.HrInit() }.is_ok() {
            self.taskbar = Some(taskbar);
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

#[derive(Clone, Copy, Debug, PartialEq)]
struct PendingTimelineSelection {
    origin: ClientPoint,
    anchor_time: TimelineTimeNs,
    anchor_track_y: i32,
    include_all_tracks: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PendingTimelineTextBlock {
    origin: ClientPoint,
    anchor_time: TimelineTimeNs,
    track_index: usize,
    track_id: TimelineTrackId,
}

#[derive(Clone, Debug, PartialEq)]
struct PendingTimelineTrackReorder {
    source_index: usize,
    current_index: usize,
    original_track_order: Vec<TimelineTrackId>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TimelinePanDrag {
    origin: ClientPoint,
    origin_viewport: TimelineViewport,
    origin_vertical_scroll_offset: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TimelinePlaygroundPanDrag {
    origin: ClientPoint,
    origin_visible_start_ns: i64,
    origin_visible_end_ns: i64,
    origin_vertical_scroll_offset: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TimelineZoomAnimation {
    start_viewport: TimelineViewport,
    target_viewport: TimelineViewport,
    started_at: Instant,
}

struct SceneSelectableTextTarget {
    rect: ClientRect,
    text: String,
    cell_width: i32,
    cell_height: i32,
}

static TOAST_HOST_STARTED: OnceLock<()> = OnceLock::new();

struct ToastHostState {
    hwnd: WindowHandle,
    renderer: RenderThreadProxy,
    last_seen_log_id: u64,
    toasts: Vec<FloatingToast>,
    terminal_cell_width: i32,
    terminal_cell_height: i32,
}

#[derive(Clone, Debug, PartialEq)]
struct FloatingToast {
    id: u64,
    level: logs::LogRecordLevel,
    message: String,
    source_hwnd: Option<isize>,
    created_at: Instant,
    expires_at: Instant,
    visual_y: f32,
}

thread_local! {
    static TOAST_HOST_STATE: RefCell<Option<ToastHostState>> = const { RefCell::new(None) };
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
    CopyDiagnostic(String),
    NotTerminal,
    CopySelection(String),
    QueryClipboard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SceneActionDisposition {
    KeepOpen,
    CloseWindow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScenePointerAction {
    NotHandled,
    Handled,
    RenderOnly,
    Invoke(SceneAction),
    OpenTimelineTranscriptionSettings(usize),
    ChooseAudioInputDevice(usize),
    ToggleAudioInputRecording,
    ToggleAudioInputTranscription,
    FlushAudioInputTranscription,
    RunAudioDaemonCudaCheck,
    SelectAudioDaemonModel(usize),
    ToggleAudioInputPlayback,
    ToggleAudioInputLoopback,
    BeginAudioInputTimeline,
    OpenLegacyRecordingDevices,
    WindowChrome(WindowChromeButton),
}

#[derive(Debug, PartialEq, Eq)]
enum SceneKeyAction {
    NotHandled,
    Handled,
    CloseWindow,
    CommitAudioInputPickerSelection(Option<AudioInputDeviceSummary>),
    InvokeSceneAction(SceneAction),
    CopySelectedText(String),
    ToggleAudioInputRecording,
    ToggleAudioInputTranscription,
    FlushAudioInputTranscription,
    ToggleAudioInputPlayback,
    PauseAudioInputPlayback,
    AudioInputPlaybackForward,
    AudioInputPlaybackBackward,
    OpenLegacyRecordingDevices,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowChromePointerAction {
    NotHandled,
    Handled,
    RenderOnly,
    Execute(WindowChromeButton),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SceneWindowShortcutAction {
    CopySelection,
    CloseWindow,
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

fn shift_key_is_down() -> bool {
    // Safety: querying key state for VK_SHIFT does not require additional invariants.
    let state = unsafe { GetKeyState(i32::from(VK_SHIFT.0)) };
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
/// behavior[impl window.appearance.shell]
/// behavior[impl window.appearance.shell-configured-default]
/// behavior[impl window.appearance.shell-starts-in-workspace-cell-dir]
/// os[impl window.appearance.translucent]
///
/// # Errors
///
/// This function will return an error if the window class, font, terminal session, or message loop fails.
#[instrument(level = "info", skip_all, fields(has_command_argv = command_argv.is_some(), has_initial_stdin = initial_stdin.is_some(), has_title = title.is_some()))]
pub fn run(
    app_home: &AppHome,
    working_dir: &Path,
    command_argv: Option<&[String]>,
    initial_stdin: Option<&str>,
    title: Option<&str>,
    vt_engine: VtEngineChoice,
) -> eyre::Result<()> {
    initialize_bell_source(app_home)?;
    let launch_command_argv = match command_argv {
        Some(command_argv) => command_argv.to_vec(),
        None => crate::shell_default::load_effective_argv(app_home)?,
    };
    let terminal = info_span!("create_terminal_session").in_scope(|| match command_argv {
        Some(_) => {
            let mut command =
                crate::shell_default::command_builder_from_argv(&launch_command_argv)?;
            command.cwd(working_dir);
            TerminalSession::new_with_command(command, vt_engine).map(HostedTerminalSession::Pty)
        }
        None => TerminalSession::new(app_home, Some(working_dir), vt_engine)
            .map(HostedTerminalSession::Pty),
    })?;
    run_with_terminal_session(terminal, launch_command_argv.len(), initial_stdin, title)
}

pub fn run_launcher(app_home: &AppHome, vt_engine: VtEngineChoice) -> eyre::Result<()> {
    initialize_bell_source(app_home)?;
    run_scene_window(
        app_home,
        SceneWindowKind::Launcher,
        vt_engine,
        SceneWindowInitialization::default(),
    )
}

pub fn list_terminal_windows() -> eyre::Result<Vec<TerminalWindowSummary>> {
    unsafe extern "system" fn enumerate_terminal_windows(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // Safety: `lparam` is initialized from a live `Vec<TerminalWindowSummary>` pointer for the duration of `EnumWindows`.
        let windows = unsafe { &mut *(lparam.0 as *mut Vec<TerminalWindowSummary>) };
        // Safety: `hwnd` is provided by `EnumWindows` while enumerating live top-level windows.
        if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
            return BOOL(1);
        }

        let mut class_name = [0_u16; 256];
        // Safety: `class_name` is a valid writable buffer and `hwnd` is the live window under enumeration.
        let class_name_len = unsafe { GetClassNameW(hwnd, &mut class_name) };
        let class_name = String::from_utf16_lossy(
            &class_name[..usize::try_from(class_name_len.max(0)).unwrap_or_default()],
        );
        if class_name != TERMINAL_WINDOW_CLASS_NAME {
            return BOOL(1);
        }

        // Safety: querying the caption length for the enumerated window is valid.
        let title_len = unsafe { GetWindowTextLengthW(hwnd) };
        let mut title = vec![0_u16; usize::try_from(title_len.max(0)).unwrap_or_default() + 1];
        // Safety: `title` is a valid writable buffer sized from the reported caption length.
        let copied = unsafe { GetWindowTextW(hwnd, &mut title) };
        let title =
            String::from_utf16_lossy(&title[..usize::try_from(copied.max(0)).unwrap_or_default()]);

        let mut pid = 0_u32;
        // Safety: `pid` is a valid out-pointer for the enumerated window's owning process id.
        let _ = unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut pid)) };
        windows.push(TerminalWindowSummary {
            hwnd: hwnd.0 as usize,
            pid,
            title,
        });
        BOOL(1)
    }

    let mut windows: Vec<TerminalWindowSummary> = Vec::new();
    let windows_ptr = &raw mut windows;
    // Safety: `windows_ptr` remains valid for the full `EnumWindows` call and the callback only appends summaries.
    unsafe {
        EnumWindows(
            Some(enumerate_terminal_windows),
            LPARAM(windows_ptr as isize),
        )
    }
    .wrap_err("failed to enumerate Teamy Studio terminal windows")?;
    windows.sort_by(|left, right| {
        left.pid
            .cmp(&right.pid)
            .then_with(|| left.hwnd.cmp(&right.hwnd))
    });
    Ok(windows)
}

#[instrument(level = "info", skip_all, fields(argc = launch_command_argc, has_initial_stdin = initial_stdin.is_some(), has_title = title.is_some()))]
fn run_with_terminal_session(
    terminal: HostedTerminalSession,
    launch_command_argc: usize,
    initial_stdin: Option<&str>,
    title: Option<&str>,
) -> eyre::Result<()> {
    let _ = launch_command_argc;
    ensure_toast_host_started();
    let window_thread = WindowThread::current();
    let dpi = system_dpi();
    let terminal_font_height = scaled_font_height(TERMINAL_FONT_HEIGHT, dpi);
    let (terminal_cell_width, terminal_cell_height) = info_span!(
        "measure_terminal_cell_size",
        kind = "terminal",
        font_height = terminal_font_height,
    )
    .in_scope(|| measure_terminal_cell_size(terminal_font_height))?;
    let diagnostic_font_height = scaled_font_height(DIAGNOSTIC_FONT_HEIGHT, dpi);
    let (diagnostic_cell_width, diagnostic_cell_height) = info_span!(
        "measure_terminal_cell_size",
        kind = "diagnostic",
        font_height = diagnostic_font_height,
    )
    .in_scope(|| measure_terminal_cell_size(diagnostic_font_height))?;
    let focused_render_interval_ms = measure_focused_render_interval_ms();

    APP_STATE.with(|state| {
        *state.borrow_mut() = Some(AppState {
            hwnd: None,
            dpi,
            launch_title: title.map(ToOwned::to_owned),
            terminal_chrome: TerminalChromeState::default(),
            last_applied_window_title: title.unwrap_or(WINDOW_TITLE).to_owned(),
            taskbar_progress: TaskbarProgressController::default(),
            pointer_position: None,
            pending_window_drag: None,
            diagnostic_panel_visible: false,
            diagnostic_selection: None,
            pending_diagnostic_selection: None,
            diagnostic_selection_drag_point: None,
            pressed_chrome_button: None,
            pin_button_last_clicked_at: None,
            pinned_topmost: false,
            diagnostics_button_last_clicked_at: None,
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
            diagnostic_font_height,
            diagnostic_cell_width,
            diagnostic_cell_height,
            chrome_tooltip: ChromeTooltipController::default(),
            terminal,
            renderer: None,
        });
    });

    let hwnd = info_span!("create_terminal_window")
        .in_scope(|| create_window(window_thread, title.unwrap_or(WINDOW_TITLE)))?;
    let renderer = info_span!("create_d3d12_renderer_thread")
        .in_scope(|| RenderThreadProxy::new(hwnd.raw()))?;
    let chrome_tooltip = ChromeTooltipController::create(hwnd)?;
    with_app_state(|state| {
        state.hwnd = Some(hwnd);
        state.terminal.set_wake_window(hwnd.raw());
        state.chrome_tooltip = chrome_tooltip;
        state.renderer = Some(renderer);
        Ok(())
    })?;
    info_span!("show_window_and_resize_terminal").in_scope(|| -> eyre::Result<()> {
        hwnd.show();

        with_app_state(|state| {
            let layout = terminal_client_layout(hwnd, state)?;
            apply_terminal_resize(state, layout)?;
            if let Some(initial_stdin) = initial_stdin.filter(|text| !text.is_empty()) {
                state
                    .terminal
                    .handle_paste(&normalize_initial_stdin(initial_stdin))?;
            }
            Ok(())
        })
    })?;

    with_app_state(|state| render_current_frame(state, hwnd, None))?;

    let _window_span = info_span!("terminal_window", source_hwnd = hwnd.raw().0 as isize).entered();
    info!("Teamy Studio terminal window shown");
    message_loop()
}

#[expect(
    clippy::too_many_lines,
    reason = "scene window startup centralizes state initialization, native window creation, and first render"
)]
fn run_scene_window(
    app_home: &AppHome,
    scene_kind: SceneWindowKind,
    vt_engine: VtEngineChoice,
    mut initialization: SceneWindowInitialization,
) -> eyre::Result<()> {
    ensure_toast_host_started();
    initialize_demo_mode_state(app_home)?;
    let window_thread = WindowThread::current();
    let dpi = system_dpi();
    let focused_render_interval_ms = measure_focused_render_interval_ms();
    let (terminal_cell_width, terminal_cell_height) =
        measure_terminal_cell_size(scaled_font_height(TERMINAL_FONT_HEIGHT, dpi))?;
    let (diagnostic_cell_width, diagnostic_cell_height) =
        measure_terminal_cell_size(scaled_font_height(DIAGNOSTIC_FONT_HEIGHT, dpi))?;
    let audio_input_picker = if matches!(
        scene_kind,
        SceneWindowKind::AudioInputDevicePicker
            | SceneWindowKind::Timeline
            | SceneWindowKind::TimelineStart
            | SceneWindowKind::TimelineAddTrack
    ) {
        AudioInputPickerState::new(list_active_audio_input_devices().unwrap_or_default())
    } else {
        AudioInputPickerState::default()
    };
    let (local_timeline_command_sender, local_timeline_command_receiver) = mpsc::channel();
    let timeline_document_command_sender = initialization
        .timeline_document_command_sender
        .take()
        .or_else(|| Some(local_timeline_command_sender.clone()));
    let timeline_playground = if scene_kind == SceneWindowKind::TimelinePlayground {
        Some(TimelinePlaygroundState::new()?)
    } else {
        None
    };

    SCENE_APP_STATE.with(|state| {
        *state.borrow_mut() = Some(SceneAppState {
            app_home: app_home.clone(),
            hwnd: None,
            dpi,
            scene_kind,
            scene_opened_at: Instant::now(),
            vt_engine,
            audio_input_picker,
            audio_input_picker_completion: AudioInputPickerCompletion::default(),
            audio_input_device_window: initialization.audio_input_device_window,
            timeline_document: initialization.timeline_document,
            timeline_transcription_settings: initialization.timeline_transcription_settings,
            timeline_document_command_sender,
            timeline_document_command_receiver: Some(local_timeline_command_receiver),
            timeline_document_command_target: initialization.timeline_document_command_target,
            model_warning: initialization.model_warning,
            model_warning_prepare_started_at: None,
            timeline_playground,
            timeline_playground_detail: initialization.timeline_playground_detail,
            timeline_tool: TimelineInteractionTool::default(),
            timeline_selection: None,
            pending_timeline_selection: None,
            pending_timeline_text_block: None,
            pending_timeline_track_reorder: None,
            timeline_pan_drag: None,
            timeline_playground_pan_drag: None,
            timeline_zoom_animation: None,
            timeline_vertical_scroll_offset: 0,
            demo_mode_scramble_input_device_identifiers:
                DemoModeInputDeviceIdentifierScramble::from_enabled(
                    current_demo_mode_state().scramble_input_device_identifiers,
                ),
            demo_mode_scramble_toggle_last_changed_at: None,
            scene_action_selected_index: 0,
            scene_virtual_cursor: None,
            pointer_position: None,
            pressed_target: None,
            pin_button_last_clicked_at: None,
            pinned_topmost: false,
            last_clicked_action: None,
            diagnostics_button_last_clicked_at: None,
            diagnostics_visible: false,
            diagnostic_selection: None,
            pending_diagnostic_selection: None,
            diagnostic_selection_drag_point: None,
            in_move_size_loop: false,
            window_focused: false,
            focused_render_interval_ms,
            terminal_cell_width,
            terminal_cell_height,
            diagnostic_cell_width,
            diagnostic_cell_height,
            logs_scroll_offset: 0,
            logs_follow_tail: true,
            last_applied_scene_window_title: scene_kind.title().to_owned(),
            chrome_tooltip: ChromeTooltipController::default(),
            renderer: None,
        });
    });

    let hwnd = create_scene_window(window_thread, scene_kind)?;
    register_scene_window(hwnd);
    let renderer = RenderThreadProxy::new(hwnd.raw())?;
    let chrome_tooltip = ChromeTooltipController::create(hwnd)?;
    with_scene_app_state(|state| {
        state.hwnd = Some(hwnd);
        if state.timeline_document_command_target.is_none()
            && scene_kind_owns_timeline_document_commands(state.scene_kind)
        {
            state.timeline_document_command_target = Some(hwnd.raw().0 as isize);
        }
        if let Some(detail) = &state.timeline_playground_detail
            && let Ok(mut detail_hwnd) = detail.hwnd.lock()
        {
            *detail_hwnd = Some(hwnd.raw().0 as isize);
        }
        state.chrome_tooltip = chrome_tooltip;
        state.renderer = Some(renderer);
        Ok(())
    })?;

    if let Some(position) = initialization.initial_position {
        hwnd.set_position_no_activate(position)?;
        hwnd.show_no_activate();
    } else {
        hwnd.show();
    }
    with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
    let _window_span = info_span!(
        "scene_window",
        source_hwnd = hwnd.raw().0 as isize,
        scene_kind = scene_kind.title(),
    )
    .entered();
    message_loop()
}

fn timeline_document_handle(state: &SceneAppState) -> Option<&TimelineDocument> {
    state.timeline_document.as_ref()
}

fn timeline_playground_detail_window_state(
    state: &SceneAppState,
) -> TimelinePlaygroundDetailWindowState {
    state
        .timeline_playground_detail
        .as_ref()
        .and_then(|detail| detail.shared.lock().ok().map(|state| state.clone()))
        .unwrap_or(TimelinePlaygroundDetailWindowState {
            detail: None,
            pinned: false,
        })
}

fn scene_kind_owns_timeline_document_commands(scene_kind: SceneWindowKind) -> bool {
    matches!(
        scene_kind,
        SceneWindowKind::Timeline
            | SceneWindowKind::TimelineStart
            | SceneWindowKind::TimelineAddTrack
    )
}

fn timeline_document_handle_mut(state: &mut SceneAppState) -> Option<&mut TimelineDocument> {
    state.timeline_document.as_mut()
}

fn ensure_timeline_document_handle(state: &mut SceneAppState) -> &mut TimelineDocument {
    state
        .timeline_document
        .get_or_insert_with(TimelineDocument::blank)
}

fn replace_timeline_document(state: &mut SceneAppState, document: TimelineDocument) {
    state.timeline_document = Some(document);
}

fn timeline_document_snapshot(state: &SceneAppState) -> Option<TimelineDocument> {
    state.timeline_document.clone()
}

fn timeline_playground_target_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> eyre::Result<Option<windows_scene::TimelinePlaygroundHitTarget>> {
    let Some(playground) = state.timeline_playground.as_ref() else {
        return Ok(None);
    };
    let playground_layout = windows_scene::timeline_playground_layout(
        layout.terminal_panel_rect().inset(24),
        playground.vertical_scroll_offset,
    );
    let query = playground
        .query(u32::try_from(playground_layout.content_rect.width().max(1)).unwrap_or(1))?;
    let render_plan = playground.dataset.render_plan(&query);
    Ok(windows_scene::timeline_playground_hit_target_at_point(
        layout,
        &render_plan,
        playground.view_state(None),
        point,
    ))
}

fn update_timeline_playground_hover_detail_from_target(
    state: &mut SceneAppState,
    hwnd: WindowHandle,
    target: windows_scene::TimelinePlaygroundHitTarget,
) -> eyre::Result<()> {
    let detail = timeline_playground_detail_from_target(state, target)?;
    if let Some(handle) = state
        .timeline_playground
        .as_ref()
        .and_then(|playground| playground.hover_detail_window.clone())
    {
        handle.update(detail);
        return Ok(());
    }
    let handle = TimelinePlaygroundDetailWindowHandle::new(detail, false);
    open_timeline_playground_detail_window(state, hwnd, handle.clone())?;
    if let Some(playground) = state.timeline_playground.as_mut() {
        playground.hover_detail_window = Some(handle);
    }
    Ok(())
}

fn pin_timeline_playground_detail_from_target(
    state: &mut SceneAppState,
    hwnd: WindowHandle,
    target: windows_scene::TimelinePlaygroundHitTarget,
) -> eyre::Result<()> {
    let detail = timeline_playground_detail_from_target(state, target)?;
    let handle = TimelinePlaygroundDetailWindowHandle::new(detail, true);
    open_timeline_playground_detail_window(state, hwnd, handle)
}

fn timeline_playground_detail_from_target(
    state: &SceneAppState,
    target: windows_scene::TimelinePlaygroundHitTarget,
) -> eyre::Result<TimelinePlaygroundDetail> {
    let Some(playground) = state.timeline_playground.as_ref() else {
        eyre::bail!("timeline playground state is missing");
    };
    timeline_playground_detail_for_render_item(&playground.dataset, target.render_item)
        .ok_or_else(|| eyre::eyre!("timeline playground target no longer resolves"))
}

fn open_timeline_playground_detail_window(
    state: &SceneAppState,
    hwnd: WindowHandle,
    detail: TimelinePlaygroundDetailWindowHandle,
) -> eyre::Result<()> {
    let app_home = state.app_home.clone();
    let vt_engine = state.vt_engine;
    let position = timeline_playground_detail_window_position(hwnd).ok();
    thread::Builder::new()
        .name("teamy-studio-timeline-playground-detail".to_owned())
        .spawn_with_current_span(move || {
            if let Err(error) = run_scene_window(
                &app_home,
                SceneWindowKind::TimelinePlaygroundDetail,
                vt_engine,
                SceneWindowInitialization {
                    timeline_playground_detail: Some(detail),
                    initial_position: position,
                    ..Default::default()
                },
            ) {
                error!(?error, "failed to open timeline playground detail window");
            }
        })
        .wrap_err("failed to spawn Teamy Studio timeline playground detail thread")?;
    Ok(())
}

fn timeline_playground_detail_window_position(hwnd: WindowHandle) -> eyre::Result<ScreenRect> {
    let owner = hwnd.window_rect()?;
    let width = 460;
    let height = owner.height().clamp(360, 760);
    Ok(timeline_playground_detail_window_position_in_bounds(
        owner,
        virtual_screen_rect(),
        width,
        height,
    ))
}

// timeline[impl playground.detail-window-clamped]
fn timeline_playground_detail_window_position_in_bounds(
    owner: ScreenRect,
    bounds: ScreenRect,
    width: i32,
    height: i32,
) -> ScreenRect {
    let margin = 12;
    let right_side = ScreenRect::new(
        owner.right() + margin,
        owner.top(),
        owner.right() + margin + width,
        owner.top() + height,
    );
    let left_side = ScreenRect::new(
        owner.left() - margin - width,
        owner.top(),
        owner.left() - margin,
        owner.top() + height,
    );
    let candidate = if right_side.right() <= bounds.right() {
        right_side
    } else if left_side.left() >= bounds.left() {
        left_side
    } else {
        right_side
    };
    clamp_screen_rect_to_bounds(candidate, bounds, margin)
}

fn clamp_screen_rect_to_bounds(rect: ScreenRect, bounds: ScreenRect, margin: i32) -> ScreenRect {
    let width = rect.width().min((bounds.width() - (margin * 2)).max(1));
    let height = rect.height().min((bounds.height() - (margin * 2)).max(1));
    let min_left = bounds.left() + margin;
    let min_top = bounds.top() + margin;
    let left = rect
        .left()
        .clamp(min_left, (bounds.right() - margin - width).max(min_left));
    let top = rect
        .top()
        .clamp(min_top, (bounds.bottom() - margin - height).max(min_top));
    ScreenRect::new(left, top, left + width, top + height)
}

fn set_timeline_playground_grouping(grouping_mode: TimelineGroupingMode) -> eyre::Result<()> {
    // timeline[impl playground.query-controls]
    with_scene_app_state(|state| {
        if let Some(playground) = state.timeline_playground.as_mut() {
            playground.grouping_mode = grouping_mode;
            playground.hovered_item = None;
        }
        Ok(())
    })
}

fn normalize_initial_stdin(text: &str) -> String {
    if text.ends_with(['\r', '\n']) {
        text.to_owned()
    } else {
        format!("{text}\r")
    }
}

fn resolved_visible_title<'a>(
    launch_title: Option<&'a str>,
    chrome_state: &'a TerminalChromeState,
) -> Option<&'a str> {
    chrome_state.runtime_title.as_deref().or(launch_title)
}

fn resolved_window_caption<'a>(
    launch_title: Option<&'a str>,
    chrome_state: &'a TerminalChromeState,
) -> &'a str {
    resolved_visible_title(launch_title, chrome_state).unwrap_or(WINDOW_TITLE)
}

// behavior[impl window.appearance.chrome.runtime-terminal-title]
fn sync_window_chrome(state: &mut AppState, hwnd: WindowHandle) -> eyre::Result<()> {
    state.terminal_chrome = state.terminal.chrome_state();

    let caption = resolved_window_caption(state.launch_title.as_deref(), &state.terminal_chrome);
    if state.last_applied_window_title != caption {
        hwnd.set_title(caption)?;
        state.last_applied_window_title = caption.to_owned();
    }

    state
        .taskbar_progress
        .apply(hwnd, state.terminal_chrome.progress)
}

// os[impl window.taskbar.progress.osc-9-4]
fn taskbar_progress_flag(progress: TerminalProgressState) -> TBPFLAG {
    match progress {
        TerminalProgressState::Hidden => TBPF_NOPROGRESS,
        TerminalProgressState::Normal(_) => TBPF_NORMAL,
        TerminalProgressState::Error(_) => TBPF_ERROR,
        TerminalProgressState::Indeterminate => TBPF_INDETERMINATE,
        TerminalProgressState::Warning(_) => TBPF_PAUSED,
    }
}

fn taskbar_progress_value(progress: TerminalProgressState) -> Option<u64> {
    match progress {
        TerminalProgressState::Normal(value)
        | TerminalProgressState::Error(value)
        | TerminalProgressState::Warning(value) => Some(u64::from(value)),
        TerminalProgressState::Hidden | TerminalProgressState::Indeterminate => None,
    }
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
pub struct TerminalThroughputBenchmarkResultsReport {
    results_path: String,
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
) -> eyre::Result<TerminalThroughputBenchmarkResultsReport> {
    let benchmark_plans = terminal_throughput_benchmark_plans(mode, line_count);
    let mut scenario_results = Vec::with_capacity(benchmark_plans.len());

    for plan in benchmark_plans {
        let mut sample_results = Vec::with_capacity(samples);
        for sample_index in 0..samples {
            let result = run_terminal_throughput_self_test_sample(plan)?;
            let _ = sample_index;
            sample_results.push(result);
        }

        let scenario_result = TerminalThroughputBenchmarkScenarioResult {
            plan,
            sample_results,
        };
        scenario_results.push(scenario_result);
    }

    write_terminal_throughput_results(app_home, cache_home, &scenario_results)
}

#[expect(
    clippy::too_many_lines,
    reason = "the benchmark loop keeps terminal, window, and renderer state transitions in one place"
)]
fn run_terminal_throughput_self_test_sample(
    plan: TerminalThroughputBenchmarkPlan,
) -> eyre::Result<TerminalThroughputBenchmarkSampleResult> {
    let window_thread = WindowThread::current();
    let dpi = system_dpi();
    let terminal_font_height = scaled_font_height(TERMINAL_FONT_HEIGHT, dpi);
    let (terminal_cell_width, terminal_cell_height) = info_span!(
        "measure_terminal_cell_size",
        kind = "terminal-benchmark",
        font_height = terminal_font_height,
    )
    .in_scope(|| measure_terminal_cell_size(terminal_font_height))?;
    let diagnostic_font_height = scaled_font_height(DIAGNOSTIC_FONT_HEIGHT, dpi);
    let (diagnostic_cell_width, diagnostic_cell_height) = info_span!(
        "measure_terminal_cell_size",
        kind = "diagnostic-benchmark",
        font_height = diagnostic_font_height,
    )
    .in_scope(|| measure_terminal_cell_size(diagnostic_font_height))?;
    let command = terminal_throughput_benchmark_command(plan.mode, plan.line_count)?;
    let mut terminal = info_span!("create_terminal_benchmark_session")
        .in_scope(|| TerminalSession::new_with_command(command, VtEngineChoice::Ghostty))?;
    let hwnd = info_span!("create_terminal_benchmark_window")
        .in_scope(|| create_benchmark_window(window_thread))?;
    let renderer = info_span!("create_terminal_benchmark_renderer")
        .in_scope(|| RenderThreadProxy::new(hwnd.raw()))?;

    let benchmark_result = (|| -> eyre::Result<TerminalThroughputBenchmarkSampleResult> {
        terminal.set_wake_window(hwnd.raw());
        let layout = client_layout(hwnd, terminal_cell_width, terminal_cell_height, true)?;
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
                    diagnostic_cell_width,
                    diagnostic_cell_height,
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
                resize_window_client(
                    hwnd,
                    i32::try_from(client_width).unwrap_or(i32::MAX),
                    i32::try_from(client_height).unwrap_or(i32::MAX),
                )?;
                let layout = client_layout(hwnd, terminal_cell_width, terminal_cell_height, true)?;
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
fn create_window(window_thread: WindowThread, window_title: &str) -> eyre::Result<WindowHandle> {
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
    let initial_window_width = scaled_window_dimension(INITIAL_WINDOW_WIDTH, system_dpi());
    let initial_window_height = scaled_window_dimension(INITIAL_WINDOW_HEIGHT, system_dpi());
    let x = (screen_width - initial_window_width) / 2;
    let y = (screen_height - initial_window_height) / 2;
    let title = window_title.easy_pcwstr()?;

    // Safety: all pointers and handles passed to CreateWindowExW are valid for the duration of the call.
    let hwnd = unsafe {
        CreateWindowExW(
            custom_window_ex_style(),
            WINDOW_CLASS_NAME,
            title.as_ref(),
            visible_custom_window_style(),
            x,
            y,
            initial_window_width,
            initial_window_height,
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

fn custom_window_ex_style() -> WINDOW_EX_STYLE {
    WS_EX_APPWINDOW | WS_EX_NOREDIRECTIONBITMAP
}

fn visible_custom_window_style() -> WINDOW_STYLE {
    base_custom_window_style() | WS_VISIBLE
}

fn base_custom_window_style() -> WINDOW_STYLE {
    WS_POPUP | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX
}

#[instrument(level = "info", skip_all)]
fn create_scene_window(
    window_thread: WindowThread,
    scene_kind: SceneWindowKind,
) -> eyre::Result<WindowHandle> {
    let instance = get_current_module().wrap_err("failed to get module handle")?;

    let class = WNDCLASSEXW {
        cbSize: u32::try_from(std::mem::size_of::<WNDCLASSEXW>())
            .expect("WNDCLASSEXW size must fit in u32"),
        hInstance: instance.into(),
        lpszClassName: SCENE_WINDOW_CLASS_NAME,
        lpfnWndProc: Some(scene_window_proc),
        hCursor: load_cursor(IDC_ARROW),
        ..Default::default()
    };
    let atom = register_window_class(&class);
    if atom == 0 {
        debug!("scene window class already registered or registration deferred");
    }

    let screen_width = system_metric(SM_CXSCREEN);
    let screen_height = system_metric(SM_CYSCREEN);
    let initial_window_width = scaled_window_dimension(INITIAL_WINDOW_WIDTH, system_dpi());
    let initial_window_height = scaled_window_dimension(INITIAL_WINDOW_HEIGHT, system_dpi());
    let x = (screen_width - initial_window_width) / 2;
    let y = (screen_height - initial_window_height) / 2;
    let title = scene_kind.title().easy_pcwstr()?;

    // Safety: all pointers and handles passed to CreateWindowExW are valid for the duration of the call.
    let hwnd = unsafe {
        CreateWindowExW(
            scene_window_ex_style(scene_kind),
            SCENE_WINDOW_CLASS_NAME,
            title.as_ref(),
            visible_custom_window_style(),
            x,
            y,
            initial_window_width,
            initial_window_height,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
    .wrap_err("failed to create scene window")?;

    Ok(WindowHandle::new(window_thread, hwnd))
}

fn scene_window_ex_style(scene_kind: SceneWindowKind) -> WINDOW_EX_STYLE {
    if scene_kind == SceneWindowKind::TimelinePlaygroundDetail {
        // timeline[impl playground.hover-detail-no-activate]
        WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP
    } else {
        custom_window_ex_style()
    }
}

fn ensure_toast_host_started() {
    TOAST_HOST_STARTED.get_or_init(|| {
        if let Err(error) = thread::Builder::new()
            .name("teamy-studio-toast-host".to_owned())
            .spawn_with_current_span(|| {
                if let Err(error) = run_toast_host_window() {
                    error!(?error, "toast host window failed");
                }
            })
        {
            error!(?error, "failed to spawn Teamy Studio toast host thread");
        }
    });
}

fn run_toast_host_window() -> eyre::Result<()> {
    let window_thread = WindowThread::current();
    let hwnd = create_toast_window(window_thread)?;
    let renderer = RenderThreadProxy::new(hwnd.raw())?;
    hwnd.set_timer(TOAST_RENDER_TIMER_ID, TOAST_RENDER_INTERVAL_MS)?;
    hwnd.hide();
    TOAST_HOST_STATE.with(|state| {
        *state.borrow_mut() = Some(ToastHostState {
            hwnd,
            renderer,
            last_seen_log_id: logs::latest_log_id(),
            toasts: Vec::new(),
            terminal_cell_width: 9,
            terminal_cell_height: 16,
        });
    });
    message_loop()
}

fn create_toast_window(window_thread: WindowThread) -> eyre::Result<WindowHandle> {
    let instance = get_current_module().wrap_err("failed to get module handle")?;
    let class = WNDCLASSEXW {
        cbSize: u32::try_from(std::mem::size_of::<WNDCLASSEXW>())
            .expect("WNDCLASSEXW size must fit in u32"),
        hInstance: instance.into(),
        lpszClassName: TOAST_WINDOW_CLASS_NAME,
        lpfnWndProc: Some(toast_window_proc),
        hCursor: load_cursor(IDC_ARROW),
        ..Default::default()
    };
    let atom = register_window_class(&class);
    if atom == 0 {
        debug!("toast window class already registered or registration deferred");
    }

    let title = "Teamy Studio Toasts".easy_pcwstr()?;
    // Safety: all pointers and handles passed to CreateWindowExW are valid for the duration of the call.
    let hwnd = unsafe {
        CreateWindowExW(
            toast_window_ex_style(),
            TOAST_WINDOW_CLASS_NAME,
            title.as_ref(),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
    .wrap_err("failed to create toast window")?;
    Ok(WindowHandle::new(window_thread, hwnd))
}

fn toast_window_ex_style() -> WINDOW_EX_STYLE {
    WS_EX_TOPMOST
        | WS_EX_TOOLWINDOW
        | WS_EX_NOACTIVATE
        | WS_EX_TRANSPARENT
        | WS_EX_NOREDIRECTIONBITMAP
}

extern "system" fn toast_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let hwnd = WindowHandle::new(WindowThread::current(), hwnd);
    match message {
        WM_TIMER if wparam.0 == TOAST_RENDER_TIMER_ID => handle_toast_render_timer(hwnd),
        WM_NCHITTEST => LRESULT(isize::try_from(HTTRANSPARENT).unwrap_or_default()),
        WM_ERASEBKGND => LRESULT(1),
        WM_DESTROY => {
            TOAST_HOST_STATE.with(|state| *state.borrow_mut() = None);
            hwnd.post_quit_message();
            LRESULT(0)
        }
        _ => def_window_proc(hwnd, message, wparam, lparam),
    }
}

fn handle_toast_render_timer(hwnd: WindowHandle) -> LRESULT {
    match TOAST_HOST_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(state) = state.as_mut() else {
            return Ok(());
        };
        sync_floating_toasts(state);
        if state.toasts.is_empty() {
            hwnd.hide();
            return Ok(());
        }
        render_toast_host(state)
    }) {
        Ok(()) => LRESULT(0),
        Err(error) => fail_and_close(hwnd, &error),
    }
}

fn sync_floating_toasts(state: &mut ToastHostState) {
    let now = Instant::now();
    for record in logs::toast_log_snapshots_after(state.last_seen_log_id) {
        state.toasts.push(FloatingToast {
            id: record.id,
            level: record.level,
            message: log_single_line(&record.message),
            source_hwnd: record.source_hwnd,
            created_at: now,
            expires_at: now + LOG_TOAST_DURATION,
            visual_y: 0.0,
        });
    }
    state.last_seen_log_id = logs::latest_log_id();
    state
        .toasts
        .retain(|toast| now < toast.expires_at + LOG_TOAST_FADE_DURATION);
    let overflow = state.toasts.len().saturating_sub(5);
    if overflow > 0 {
        state.toasts.drain(0..overflow);
    }
}

// observability[impl toasts.floating-source]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "toast host height tracks bounded animated stack offsets in pixels"
)]
fn render_toast_host(state: &mut ToastHostState) -> eyre::Result<()> {
    let width = LOG_TOAST_WIDTH + LOG_TOAST_HOST_PADDING * 2;
    let toast_height = LOG_TOAST_HEIGHT;
    let gap = LOG_TOAST_GAP;
    let visible_count = state.toasts.len().min(5);
    let visible_count_i32 = i32::try_from(visible_count).unwrap_or_default();
    let stack_step = (toast_height + gap) as f32;
    let animated_stack_offset = state
        .toasts
        .iter()
        .rev()
        .take(visible_count)
        .enumerate()
        .map(|(index, toast)| {
            let target_y = index as f32 * stack_step;
            toast.visual_y.max(target_y)
        })
        .fold(0.0_f32, f32::max);
    let stack_height = (toast_height + animated_stack_offset.ceil() as i32)
        .max(visible_count_i32 * toast_height + visible_count_i32.saturating_sub(1) * gap);
    let height = stack_height + LOG_TOAST_HOST_PADDING * 2;
    let source_hwnd = state.toasts.last().and_then(|toast| toast.source_hwnd);
    let rect = toast_window_rect(source_hwnd, width, height);
    state.hwnd.set_position_no_activate(rect)?;
    state.hwnd.show_no_activate();
    state.renderer.resize(
        u32::try_from(width.max(1)).unwrap_or(1),
        u32::try_from(height.max(1)).unwrap_or(1),
    )?;

    let layout = TerminalLayout {
        client_width: width,
        client_height: height,
        cell_width: state.terminal_cell_width,
        cell_height: state.terminal_cell_height,
        diagnostic_panel_visible: false,
    };
    let now = Instant::now();
    let mut scene = RenderScene {
        panels: Vec::new(),
        glyphs: Vec::new(),
        sprites: Vec::new(),
        overlay_panels: Vec::new(),
    };
    windows_scene::push_info_toasts(&mut scene, layout, &toast_view_states(state, now));
    let frame = RenderFrameModel {
        layout,
        title: None,
        diagnostic_text: String::new(),
        diagnostic_selection: None,
        window_chrome_buttons_state: WindowChromeButtonsState::default(),
        diagnostic_cell_width: state.terminal_cell_width,
        diagnostic_cell_height: state.terminal_cell_height,
        scene: Some(scene),
        terminal_cell_width: state.terminal_cell_width,
        terminal_cell_height: state.terminal_cell_height,
        terminal_display: Arc::new(TerminalDisplayState::default()),
        terminal_visual_state: RendererTerminalVisualState::default(),
    };
    state.renderer.render_frame_model_force_redraw(frame)
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "toast animation offsets are small pixel values bounded by the visible toast count"
)]
// observability[impl toasts.animation]
fn toast_view_states(
    state: &mut ToastHostState,
    now: Instant,
) -> Vec<windows_scene::ToastViewState> {
    let toast_height = 60.0_f32;
    let gap = 10.0_f32;
    let total_toasts = state.toasts.len();
    state
        .toasts
        .iter_mut()
        .enumerate()
        .map(|(index, toast)| {
            let target_y = (total_toasts - 1 - index) as f32 * (toast_height + gap);
            toast.visual_y += (target_y - toast.visual_y) * 0.32;
            let age = now.duration_since(toast.created_at);
            let appear = (age.as_secs_f32() / 0.22).clamp(0.0, 1.0);
            let fade = if now > toast.expires_at {
                1.0 - (now.duration_since(toast.expires_at).as_secs_f32()
                    / LOG_TOAST_FADE_DURATION.as_secs_f32())
                .clamp(0.0, 1.0)
            } else {
                1.0
            };
            let progress_remaining = if now >= toast.expires_at {
                0.0
            } else {
                (toast.expires_at.duration_since(now).as_secs_f32()
                    / LOG_TOAST_DURATION.as_secs_f32())
                .clamp(0.0, 1.0)
            };
            windows_scene::ToastViewState {
                level: toast.level,
                message: toast.message.clone(),
                progress_remaining,
                opacity: (appear * fade).clamp(0.0, 1.0),
                translate_x: ((1.0 - appear) * 28.0) as i32,
                translate_y: (target_y - toast.visual_y) as i32,
            }
        })
        .collect()
}

// observability[impl toasts.floating-source]
fn toast_window_rect(source_hwnd: Option<isize>, width: i32, height: i32) -> ScreenRect {
    let virtual_bounds = virtual_screen_rect();
    let margin = 12;
    let Some(source_rect) = source_hwnd.and_then(window_rect_from_raw_hwnd) else {
        return ScreenRect::new(
            virtual_bounds.right() - width - margin,
            virtual_bounds.bottom() - height - margin,
            virtual_bounds.right() - margin,
            virtual_bounds.bottom() - margin,
        );
    };
    let bounds = monitor_work_rect_for_raw_hwnd(source_hwnd).unwrap_or(virtual_bounds);
    let top = (source_rect.bottom() - height)
        .clamp(bounds.top() + margin, bounds.bottom() - height - margin);
    let right_side = ScreenRect::new(
        source_rect.right() + margin,
        top,
        source_rect.right() + margin + width,
        top + height,
    );
    if right_side.right() <= bounds.right() {
        return right_side;
    }
    let left_side = ScreenRect::new(
        source_rect.left() - margin - width,
        top,
        source_rect.left() - margin,
        top + height,
    );
    if left_side.left() >= bounds.left() {
        return left_side;
    }
    let overflow_right = ScreenRect::new(
        source_rect.right() + margin,
        top,
        source_rect.right() + margin + width,
        top + height,
    );
    if overflow_right.right() <= virtual_bounds.right() {
        return overflow_right;
    }
    ScreenRect::new(
        virtual_bounds.right() - width - margin,
        (source_rect.bottom() - height).clamp(
            virtual_bounds.top() + margin,
            virtual_bounds.bottom() - height - margin,
        ),
        virtual_bounds.right() - margin,
        (source_rect.bottom()).clamp(
            virtual_bounds.top() + height + margin,
            virtual_bounds.bottom() - margin,
        ),
    )
}

fn window_rect_from_raw_hwnd(raw_hwnd: isize) -> Option<ScreenRect> {
    let hwnd = HWND(raw_hwnd as *mut c_void);
    // Safety: this only queries visibility for a raw HWND captured from a tracing field.
    if hwnd.0.is_null() || !unsafe { IsWindowVisible(hwnd) }.as_bool() {
        return None;
    }
    let mut rect = RECT::default();
    // Safety: `rect` is writable storage and the raw HWND is checked by the OS during the query.
    unsafe { GetWindowRect(hwnd, &raw mut rect) }.ok()?;
    Some(ScreenRect::from_win32_rect(rect))
}

fn monitor_work_rect_for_raw_hwnd(raw_hwnd: Option<isize>) -> Option<ScreenRect> {
    let hwnd = HWND(raw_hwnd? as *mut c_void);
    // Safety: resolving the nearest monitor for a raw HWND is a read-only OS query.
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_FROM_FLAGS(2)) };
    if monitor.0.is_null() {
        return None;
    }
    let mut info = MONITORINFO {
        cbSize: u32::try_from(std::mem::size_of::<MONITORINFO>()).ok()?,
        ..Default::default()
    };
    // Safety: `info` is valid writable storage for the monitor handle returned above.
    unsafe { GetMonitorInfoW(monitor, &raw mut info) }
        .as_bool()
        .then_some(ScreenRect::from_win32_rect(info.rcWork))
}

fn virtual_screen_rect() -> ScreenRect {
    let left = system_metric(SM_XVIRTUALSCREEN);
    let top = system_metric(SM_YVIRTUALSCREEN);
    let width = system_metric(SM_CXVIRTUALSCREEN);
    let height = system_metric(SM_CYVIRTUALSCREEN);
    ScreenRect::new(left, top, left + width, top + height)
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

    let title = WINDOW_TITLE.easy_pcwstr()?;
    let initial_window_width = scaled_window_dimension(INITIAL_WINDOW_WIDTH, system_dpi());
    let initial_window_height = scaled_window_dimension(INITIAL_WINDOW_HEIGHT, system_dpi());
    // Safety: all pointers and handles passed to CreateWindowExW are valid for the duration of the call.
    let hwnd = unsafe {
        CreateWindowExW(
            custom_window_ex_style(),
            BENCHMARK_WINDOW_CLASS_NAME,
            title.as_ref(),
            base_custom_window_style(),
            0,
            0,
            initial_window_width,
            initial_window_height,
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
        WM_DPICHANGED => handle_dpi_changed(hwnd, lparam),
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

extern "system" fn scene_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let hwnd = WindowHandle::new(WindowThread::current(), hwnd);
    match message {
        WM_NCCALCSIZE => LRESULT(0),
        WM_SETFOCUS => handle_scene_focus_changed(hwnd, true),
        WM_KILLFOCUS => handle_scene_focus_changed(hwnd, false),
        WM_ENTERSIZEMOVE => handle_scene_enter_size_move(hwnd),
        WM_EXITSIZEMOVE => handle_scene_exit_size_move(hwnd),
        WM_DPICHANGED => handle_scene_dpi_changed(hwnd, lparam),
        WM_SIZE => handle_scene_size(hwnd),
        WM_TIMER if wparam.0 == FOCUSED_RENDER_TIMER_ID => handle_scene_focused_render_timer(hwnd),
        DEMO_MODE_STATE_CHANGED_MESSAGE => handle_scene_demo_mode_state_changed(hwnd),
        TIMELINE_DOCUMENT_CHANGED_MESSAGE => handle_scene_timeline_document_changed(hwnd),
        TIMELINE_DOCUMENT_COMMAND_MESSAGE => handle_scene_timeline_document_command(hwnd),
        TIMELINE_TRANSCRIPTION_WORKER_COMPLETED_MESSAGE => {
            handle_scene_timeline_transcription_worker_completed(hwnd)
        }
        TIMELINE_PLAYGROUND_DETAIL_CHANGED_MESSAGE => {
            handle_scene_timeline_playground_detail_changed(hwnd)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => handle_scene_key_down_message(hwnd, message, wparam, lparam),
        WM_LBUTTONDOWN => handle_bool_message(hwnd, message, wparam, lparam, |hwnd| {
            handle_scene_left_button_down(hwnd, lparam)
        }),
        WM_MOUSEMOVE => handle_bool_message(hwnd, message, wparam, lparam, |hwnd| {
            handle_scene_mouse_move(hwnd, wparam, lparam)
        }),
        WM_PAINT => match acknowledge_paint(hwnd) {
            Ok(()) => LRESULT(0),
            Err(error) => fail_and_close(hwnd, &error),
        },
        WM_LBUTTONUP => handle_bool_message(hwnd, message, wparam, lparam, |hwnd| {
            handle_scene_left_button_up(hwnd, lparam)
        }),
        WM_RBUTTONDOWN => handle_bool_message(hwnd, message, wparam, lparam, |hwnd| {
            handle_scene_right_button_down(hwnd, lparam)
        }),
        WM_RBUTTONUP => handle_bool_message(hwnd, message, wparam, lparam, |hwnd| {
            handle_scene_right_button_up(hwnd, lparam)
        }),
        WM_MOUSEWHEEL => handle_bool_message(hwnd, message, wparam, lparam, |hwnd| {
            handle_scene_mouse_wheel(hwnd, wparam, lparam)
        }),
        WM_CLOSE => handle_scene_close_message(hwnd),
        WM_SETCURSOR => match handle_scene_set_cursor(hwnd, lparam) {
            Ok(true) => LRESULT(1),
            Ok(false) => def_window_proc(hwnd, message, wparam, lparam),
            Err(error) => fail_and_close(hwnd, &error),
        },
        WM_NCHITTEST => handle_non_client_hit_test(hwnd, lparam),
        WM_ERASEBKGND => LRESULT(1),
        WM_DESTROY => handle_scene_destroy_message(hwnd),
        _ => def_window_proc(hwnd, message, wparam, lparam),
    }
}

fn handle_scene_close_message(hwnd: WindowHandle) -> LRESULT {
    let close_action = with_scene_app_state(|state| {
        let warning = (state.scene_kind == SceneWindowKind::TimelineTranscriptionSettings)
            .then(|| transcription_model_warning_for_selected_settings(state))
            .flatten();
        Ok((state.app_home.clone(), state.vt_engine, warning))
    });
    match close_action {
        Ok((app_home, vt_engine, warning)) => {
            if let Some(warning) = warning {
                open_model_warning_window(&app_home, vt_engine, warning);
            }
            hwnd.destroy();
            LRESULT(0)
        }
        Err(error) => fail_and_close(hwnd, &error),
    }
}

fn handle_scene_enter_size_move(hwnd: WindowHandle) -> LRESULT {
    match with_scene_app_state(|state| {
        state.in_move_size_loop = true;
        render_scene_window_frame(state, hwnd, None, false)?;
        Ok(())
    }) {
        Ok(()) => LRESULT(0),
        Err(error) => fail_and_close(hwnd, &error),
    }
}

fn handle_scene_exit_size_move(hwnd: WindowHandle) -> LRESULT {
    match with_scene_app_state(|state| {
        state.in_move_size_loop = false;
        render_scene_window_frame(state, hwnd, None, false)?;
        Ok(())
    }) {
        Ok(()) => LRESULT(0),
        Err(error) => fail_and_close(hwnd, &error),
    }
}

fn handle_scene_size(hwnd: WindowHandle) -> LRESULT {
    match with_scene_app_state(|state| {
        let layout = scene_client_layout(hwnd, state)?;
        render_scene_window_frame(
            state,
            hwnd,
            Some((
                layout.client_width.cast_unsigned(),
                layout.client_height.cast_unsigned(),
            )),
            false,
        )?;
        Ok(())
    }) {
        Ok(()) => LRESULT(0),
        Err(error) => fail_and_close(hwnd, &error),
    }
}

fn handle_scene_dpi_changed(hwnd: WindowHandle, lparam: LPARAM) -> LRESULT {
    let result = with_scene_app_state(|state| apply_scene_dpi(state, window_dpi(hwnd)))
        .and_then(|()| apply_suggested_dpi_rect(hwnd, lparam));

    match result {
        Ok(()) => LRESULT(0),
        Err(error) => fail_and_close(hwnd, &error),
    }
}

fn handle_scene_focused_render_timer(hwnd: WindowHandle) -> LRESULT {
    match with_scene_app_state(|state| {
        if !state.window_focused {
            return Ok(());
        }

        maybe_complete_model_warning_prepare(state);

        render_scene_window_frame(state, hwnd, None, true)?;
        Ok(())
    }) {
        Ok(()) => LRESULT(0),
        Err(error) => fail_and_close(hwnd, &error),
    }
}

fn maybe_complete_model_warning_prepare(state: &mut SceneAppState) {
    let Some(started_at) = state.model_warning_prepare_started_at else {
        return;
    };
    if started_at.elapsed() < MODEL_WARNING_PREPARE_HOLD_DURATION {
        return;
    }
    state.model_warning_prepare_started_at = None;
    let Some(warning) = state.model_warning.as_ref() else {
        return;
    };
    let app_home = state.app_home.clone();
    let model_name = warning.status.model_name.clone();
    ring_terminal_bell();
    let _ = thread::Builder::new()
        .name("teamy-studio-prepare-warning-model".to_owned())
        .spawn_with_current_span(move || {
            let job = super::jobs::start_job(
                "Prepare Whisper model",
                format!("Preparing {model_name} for Rust transcription"),
            );
            job.update(format!("Downloading or converting {model_name}"));
            match crate::model::prepare_known_whisper_model(
                &app_home,
                &crate::paths::CACHE_DIR,
                &model_name,
                false,
            ) {
                Ok(prepared) => job.complete(format!(
                    "Prepared {} at {}",
                    model_name,
                    prepared.managed_dir.display()
                )),
                Err(error) => job.fail(format!("Failed to prepare {model_name}: {error}")),
            }
        });
}

fn handle_scene_focus_changed(hwnd: WindowHandle, focused: bool) -> LRESULT {
    match with_scene_app_state(|state| {
        state.window_focused = focused;
        if focused {
            hwnd.set_focused_render_timer(state.focused_render_interval_ms)?;
            render_scene_window_frame(state, hwnd, None, true)?;
        } else {
            hwnd.clear_focused_render_timer();
            state.chrome_tooltip.hide(hwnd);
            render_scene_window_frame(state, hwnd, None, true)?;
        }
        Ok(())
    }) {
        Ok(()) => LRESULT(0),
        Err(error) => fail_and_close(hwnd, &error),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "scene keyboard dispatch is centralized for chrome, picker, and microphone transport actions"
)]
fn handle_scene_key_down_message(
    hwnd: WindowHandle,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let virtual_key = match wparam_to_u32(wparam) {
        Ok(virtual_key) => virtual_key,
        Err(error) => return fail_and_close(hwnd, &error),
    };
    let action = with_scene_app_state(|state| {
        if alt_key_is_down() && virtual_key == u32::from(b'X') {
            // audio[impl gui.diagnostics-toggle]
            scene_toggle_diagnostics_panel(state);
            render_scene_window_frame(state, hwnd, None, false)?;
            return Ok(SceneKeyAction::Handled);
        }

        if alt_key_is_down()
            && virtual_key == u32::from(b'R')
            && (state.scene_kind == SceneWindowKind::AudioInputDevicePicker
                || state.scene_kind == SceneWindowKind::AudioInputDeviceDetails)
        {
            // audio[impl gui.legacy-recording-dialog]
            return Ok(SceneKeyAction::OpenLegacyRecordingDevices);
        }

        if let Some(shortcut) = scene_window_shortcut_action(control_key_is_down(), virtual_key) {
            match shortcut {
                SceneWindowShortcutAction::CopySelection => {
                    // windowing[impl scene.text.keyboard-copy]
                    let layout = scene_client_layout(hwnd, state)?;
                    if let Some(text) = scene_selected_text_for_copy(state, layout) {
                        return Ok(SceneKeyAction::CopySelectedText(text));
                    }
                    return Ok(SceneKeyAction::Handled);
                }
                SceneWindowShortcutAction::CloseWindow => {
                    // windowing[impl scene.keyboard-close]
                    return Ok(SceneKeyAction::CloseWindow);
                }
            }
        }

        if state.scene_kind == SceneWindowKind::AudioInputDeviceDetails {
            if virtual_key == u32::from(VK_ESCAPE.0) {
                return Ok(SceneKeyAction::CloseWindow);
            }
            if virtual_key == u32::from(VK_RETURN.0) {
                return Ok(SceneKeyAction::ToggleAudioInputRecording);
            }
            if virtual_key == u32::from(VK_SPACE.0) {
                return Ok(SceneKeyAction::ToggleAudioInputPlayback);
            }
            if virtual_key == u32::from(b'T') {
                return Ok(SceneKeyAction::ToggleAudioInputTranscription);
            }
            if virtual_key == u32::from(b'F') {
                return Ok(SceneKeyAction::FlushAudioInputTranscription);
            }
            if virtual_key == u32::from(b'K') {
                return Ok(SceneKeyAction::PauseAudioInputPlayback);
            }
            if virtual_key == u32::from(b'L') {
                return Ok(SceneKeyAction::AudioInputPlaybackForward);
            }
            if virtual_key == u32::from(b'J') {
                return Ok(SceneKeyAction::AudioInputPlaybackBackward);
            }
        }

        if state.scene_kind == SceneWindowKind::Timeline {
            if virtual_key == u32::from(VK_ESCAPE.0)
                && let Some(pending_reorder) = state.pending_timeline_track_reorder.take()
            {
                if let Some(document) = timeline_document_handle_mut(state) {
                    let restored =
                        document.restore_track_order(&pending_reorder.original_track_order);
                    if restored {
                        broadcast_timeline_document_changed();
                    }
                }
                state.pressed_target = None;
                render_scene_window_frame(state, hwnd, None, false)?;
                return Ok(SceneKeyAction::Handled);
            }
            // timeline[impl transport.spacebar-playback]
            if virtual_key == u32::from(VK_SPACE.0) {
                return Ok(SceneKeyAction::ToggleAudioInputPlayback);
            }
            if virtual_key == u32::from(VK_LEFT.0) {
                return Ok(SceneKeyAction::InvokeSceneAction(
                    SceneAction::PanTimelineLeft,
                ));
            }
            if virtual_key == u32::from(VK_RIGHT.0) {
                return Ok(SceneKeyAction::InvokeSceneAction(
                    SceneAction::PanTimelineRight,
                ));
            }
            if virtual_key == u32::from(VK_OEM_PLUS.0) || virtual_key == u32::from(VK_ADD.0) {
                return Ok(SceneKeyAction::InvokeSceneAction(
                    SceneAction::ZoomTimelineIn,
                ));
            }
            if virtual_key == u32::from(VK_OEM_MINUS.0) || virtual_key == u32::from(VK_SUBTRACT.0) {
                return Ok(SceneKeyAction::InvokeSceneAction(
                    SceneAction::ZoomTimelineOut,
                ));
            }
        }

        if state.scene_kind == SceneWindowKind::TimelineTranscriptionSettings {
            if virtual_key == u32::from(VK_ESCAPE.0) {
                return Ok(SceneKeyAction::CloseWindow);
            }

            let input_row_count = timeline_transcription_settings_input_row_count(state);
            let output_row_count = timeline_transcription_settings_output_row_count(state);
            let Some(settings) = state.timeline_transcription_settings.as_mut() else {
                return Ok(SceneKeyAction::NotHandled);
            };
            let mut changed = false;
            let mut dock_column = None;
            if virtual_key == u32::from(VK_LEFT.0) {
                settings.selected_column = match settings.selected_column {
                    windows_scene::TimelineTranscriptionSettingsColumn::Model
                    | windows_scene::TimelineTranscriptionSettingsColumn::InputAudioTrack => {
                        windows_scene::TimelineTranscriptionSettingsColumn::Model
                    }
                    windows_scene::TimelineTranscriptionSettingsColumn::OutputTextTrack => {
                        windows_scene::TimelineTranscriptionSettingsColumn::InputAudioTrack
                    }
                };
                changed = true;
            } else if virtual_key == u32::from(VK_RIGHT.0) {
                settings.selected_column = match settings.selected_column {
                    windows_scene::TimelineTranscriptionSettingsColumn::Model => {
                        windows_scene::TimelineTranscriptionSettingsColumn::InputAudioTrack
                    }
                    windows_scene::TimelineTranscriptionSettingsColumn::InputAudioTrack
                    | windows_scene::TimelineTranscriptionSettingsColumn::OutputTextTrack => {
                        windows_scene::TimelineTranscriptionSettingsColumn::OutputTextTrack
                    }
                };
                changed = true;
            } else if virtual_key == u32::from(VK_UP.0) {
                match settings.selected_column {
                    windows_scene::TimelineTranscriptionSettingsColumn::Model => {
                        settings.selected_model_index =
                            settings.selected_model_index.saturating_sub(1);
                        dock_column =
                            Some(windows_scene::TimelineTranscriptionSettingsColumn::Model);
                    }
                    windows_scene::TimelineTranscriptionSettingsColumn::InputAudioTrack => {
                        settings.selected_input_audio_track_index =
                            settings.selected_input_audio_track_index.saturating_sub(1);
                        dock_column = Some(
                            windows_scene::TimelineTranscriptionSettingsColumn::InputAudioTrack,
                        );
                    }
                    windows_scene::TimelineTranscriptionSettingsColumn::OutputTextTrack => {
                        settings.selected_output_text_track_index =
                            settings.selected_output_text_track_index.saturating_sub(1);
                        dock_column = Some(
                            windows_scene::TimelineTranscriptionSettingsColumn::OutputTextTrack,
                        );
                    }
                }
                changed = true;
            } else if virtual_key == u32::from(VK_DOWN.0) {
                match settings.selected_column {
                    windows_scene::TimelineTranscriptionSettingsColumn::Model => {
                        settings.selected_model_index = (settings.selected_model_index + 1)
                            .min(KNOWN_WHISPER_MODELS.len().saturating_sub(1));
                        dock_column =
                            Some(windows_scene::TimelineTranscriptionSettingsColumn::Model);
                    }
                    windows_scene::TimelineTranscriptionSettingsColumn::InputAudioTrack => {
                        settings.selected_input_audio_track_index =
                            (settings.selected_input_audio_track_index + 1)
                                .min(input_row_count.saturating_sub(1));
                        dock_column = Some(
                            windows_scene::TimelineTranscriptionSettingsColumn::InputAudioTrack,
                        );
                    }
                    windows_scene::TimelineTranscriptionSettingsColumn::OutputTextTrack => {
                        settings.selected_output_text_track_index =
                            (settings.selected_output_text_track_index + 1)
                                .min(output_row_count.saturating_sub(1));
                        dock_column = Some(
                            windows_scene::TimelineTranscriptionSettingsColumn::OutputTextTrack,
                        );
                    }
                }
                changed = true;
            } else if virtual_key == u32::from(VK_RETURN.0) || virtual_key == u32::from(VK_SPACE.0)
            {
                dock_column = Some(settings.selected_column);
                changed = true;
            }

            if let Some(column) = dock_column {
                if let Some(settings) = state.timeline_transcription_settings.as_mut() {
                    match column {
                        windows_scene::TimelineTranscriptionSettingsColumn::Model => {
                            settings.model_target_docked = true;
                        }
                        windows_scene::TimelineTranscriptionSettingsColumn::InputAudioTrack => {
                            settings.input_target_docked = true;
                        }
                        windows_scene::TimelineTranscriptionSettingsColumn::OutputTextTrack => {
                            settings.output_target_docked = true;
                        }
                    }
                }
                apply_timeline_transcription_settings_selection(state);
            }

            if changed {
                render_scene_window_frame(state, hwnd, None, false)?;
                return Ok(SceneKeyAction::Handled);
            }
        }

        if matches!(
            state.scene_kind,
            SceneWindowKind::Launcher
                | SceneWindowKind::TimelineStart
                | SceneWindowKind::TimelineAddTrack
        ) {
            // windowing[impl launcher.keyboard-navigation]
            if let Some(navigation) =
                launcher_menu_navigation_from_virtual_key(virtual_key, shift_key_is_down())
            {
                let layout = scene_client_layout(hwnd, state)?;
                let rects = scene_action_navigation_rects(state, layout);
                let (selected_index, virtual_cursor) = next_scene_action_target(
                    &rects,
                    state.scene_action_selected_index,
                    state.scene_virtual_cursor,
                    navigation,
                );
                state.scene_action_selected_index = selected_index;
                state.scene_virtual_cursor = Some(virtual_cursor);
                render_scene_window_frame(state, hwnd, None, false)?;
                update_scene_virtual_cursor_tooltip(state, hwnd)?;
                return Ok(SceneKeyAction::Handled);
            }
            if virtual_key == u32::from(VK_RETURN.0) || virtual_key == u32::from(VK_SPACE.0) {
                let Some(action) = selected_scene_action(state) else {
                    return Ok(SceneKeyAction::NotHandled);
                };
                state.last_clicked_action = Some(ClickState {
                    action,
                    clicked_at: Instant::now(),
                });
                return Ok(SceneKeyAction::InvokeSceneAction(action));
            }
        }

        if state.scene_kind == SceneWindowKind::CursorGallery {
            // windowing[impl cursor-gallery.virtual-navigation]
            if let Some(navigation) =
                launcher_menu_navigation_from_virtual_key(virtual_key, shift_key_is_down())
            {
                let layout = scene_client_layout(hwnd, state)?;
                let rects = cursor_gallery_navigation_rects(layout);
                let (selected_index, virtual_cursor) = next_scene_action_target(
                    &rects,
                    state.scene_action_selected_index,
                    state.scene_virtual_cursor,
                    navigation,
                );
                state.scene_action_selected_index = selected_index;
                state.scene_virtual_cursor = Some(virtual_cursor);
                render_scene_window_frame(state, hwnd, None, false)?;
                update_scene_virtual_cursor_tooltip(state, hwnd)?;
                return Ok(SceneKeyAction::Handled);
            }
        }

        if state.scene_kind == SceneWindowKind::DemoMode {
            // windowing[impl demo-mode.window]
            if let Some(navigation) =
                launcher_menu_navigation_from_virtual_key(virtual_key, shift_key_is_down())
            {
                let layout = scene_client_layout(hwnd, state)?;
                let rects = demo_mode_navigation_rects(layout);
                let (selected_index, virtual_cursor) = next_scene_action_target(
                    &rects,
                    state.scene_action_selected_index,
                    state.scene_virtual_cursor,
                    navigation,
                );
                state.scene_action_selected_index = selected_index;
                state.scene_virtual_cursor = Some(virtual_cursor);
                render_scene_window_frame(state, hwnd, None, false)?;
                update_scene_virtual_cursor_tooltip(state, hwnd)?;
                return Ok(SceneKeyAction::Handled);
            }
            if virtual_key == u32::from(VK_RETURN.0) || virtual_key == u32::from(VK_SPACE.0) {
                if state.scene_action_selected_index == 1 {
                    toggle_demo_mode_scramble_input_device_identifiers(state)?;
                }
                render_scene_window_frame(state, hwnd, None, false)?;
                update_scene_virtual_cursor_tooltip(state, hwnd)?;
                return Ok(SceneKeyAction::Handled);
            }
        }

        if state.scene_kind != SceneWindowKind::AudioInputDevicePicker {
            return Ok(SceneKeyAction::NotHandled);
        }

        let Some(key) = audio_input_picker_key_from_virtual_key(virtual_key) else {
            return Ok(SceneKeyAction::NotHandled);
        };
        match state.audio_input_picker.handle_key(key) {
            AudioInputPickerKeyResult::Handled => {
                render_scene_window_frame(state, hwnd, None, false)?;
                Ok(SceneKeyAction::Handled)
            }
            AudioInputPickerKeyResult::Choose => {
                let description = state.audio_input_picker.selected_device().cloned();
                render_scene_window_frame(state, hwnd, None, false)?;
                Ok(SceneKeyAction::CommitAudioInputPickerSelection(description))
            }
            AudioInputPickerKeyResult::OpenLegacyRecordingDevices => {
                render_scene_window_frame(state, hwnd, None, false)?;
                Ok(SceneKeyAction::OpenLegacyRecordingDevices)
            }
            AudioInputPickerKeyResult::Close => {
                if state.audio_input_picker_completion
                    == AudioInputPickerCompletion::TimelineMicrophoneTrack
                {
                    state.audio_input_picker_completion =
                        AudioInputPickerCompletion::OpenDeviceWindow;
                    state.scene_kind = SceneWindowKind::TimelineAddTrack;
                    state.scene_action_selected_index = 0;
                    state.scene_virtual_cursor = None;
                    render_scene_window_frame(state, hwnd, None, false)?;
                    Ok(SceneKeyAction::Handled)
                } else {
                    Ok(SceneKeyAction::CloseWindow)
                }
            }
        }
    });

    match action {
        Ok(SceneKeyAction::NotHandled) => {
            trace!(
                message = if message == WM_SYSKEYDOWN {
                    "WM_SYSKEYDOWN"
                } else {
                    "WM_KEYDOWN"
                },
                vkey = virtual_key,
                lparam = lparam.0,
                consumed = false,
                "processed scene keyboard down message"
            );
            def_window_proc(hwnd, message, wparam, lparam)
        }
        Ok(SceneKeyAction::Handled) => LRESULT(0),
        Ok(SceneKeyAction::CloseWindow) => {
            hwnd.post_close();
            LRESULT(0)
        }
        Ok(SceneKeyAction::CommitAudioInputPickerSelection(device)) => {
            commit_audio_input_picker_selection_from_scene(hwnd, device);
            LRESULT(0)
        }
        Ok(SceneKeyAction::InvokeSceneAction(action)) => {
            let result = invoke_scene_action(hwnd, action);
            match result {
                Ok(SceneActionDisposition::KeepOpen) => LRESULT(0),
                Ok(SceneActionDisposition::CloseWindow) => {
                    hwnd.post_close();
                    LRESULT(0)
                }
                Err(error) => fail_and_close(hwnd, &error),
            }
        }
        Ok(SceneKeyAction::CopySelectedText(text)) => {
            if !text.is_empty()
                && let Err(error) = write_clipboard(&text)
            {
                error!(
                    ?error,
                    "failed to copy scene selected text to the clipboard"
                );
            }
            LRESULT(0)
        }
        Ok(SceneKeyAction::ToggleAudioInputRecording) => {
            let result = with_scene_app_state(|state| {
                if let Some(device_window) = state.audio_input_device_window.as_mut() {
                    device_window.toggle_recording()?;
                }
                render_scene_window_frame(state, hwnd, None, false)
            });
            match result {
                Ok(()) => LRESULT(0),
                Err(error) => fail_and_close(hwnd, &error),
            }
        }
        Ok(SceneKeyAction::ToggleAudioInputTranscription) => {
            let result = with_scene_app_state(|state| {
                toggle_audio_input_transcription_from_scene_state(state);
                render_scene_window_frame(state, hwnd, None, false)
            });
            match result {
                Ok(()) => LRESULT(0),
                Err(error) => fail_and_close(hwnd, &error),
            }
        }
        Ok(SceneKeyAction::FlushAudioInputTranscription) => {
            let result = with_scene_app_state(|state| {
                if let Some(device_window) = state.audio_input_device_window.as_mut() {
                    device_window.flush_transcription_chunk();
                }
                render_scene_window_frame(state, hwnd, None, false)
            });
            match result {
                Ok(()) => LRESULT(0),
                Err(error) => fail_and_close(hwnd, &error),
            }
        }
        Ok(SceneKeyAction::ToggleAudioInputPlayback) => {
            let result = with_scene_app_state(|state| {
                if let Some(device_window) = state.audio_input_device_window.as_mut() {
                    device_window.toggle_playback()?;
                }
                render_scene_window_frame(state, hwnd, None, false)
            });
            match result {
                Ok(()) => LRESULT(0),
                Err(error) => fail_and_close(hwnd, &error),
            }
        }
        Ok(SceneKeyAction::PauseAudioInputPlayback) => {
            let result = with_scene_app_state(|state| {
                if let Some(device_window) = state.audio_input_device_window.as_mut() {
                    device_window.pause_playback();
                }
                render_scene_window_frame(state, hwnd, None, false)
            });
            match result {
                Ok(()) => LRESULT(0),
                Err(error) => fail_and_close(hwnd, &error),
            }
        }
        Ok(SceneKeyAction::AudioInputPlaybackForward) => {
            let result = with_scene_app_state(|state| {
                if let Some(device_window) = state.audio_input_device_window.as_mut() {
                    device_window.playback_forward()?;
                }
                render_scene_window_frame(state, hwnd, None, false)
            });
            match result {
                Ok(()) => LRESULT(0),
                Err(error) => fail_and_close(hwnd, &error),
            }
        }
        Ok(SceneKeyAction::AudioInputPlaybackBackward) => {
            let result = with_scene_app_state(|state| {
                if let Some(device_window) = state.audio_input_device_window.as_mut() {
                    device_window.playback_backward()?;
                }
                render_scene_window_frame(state, hwnd, None, false)
            });
            match result {
                Ok(()) => LRESULT(0),
                Err(error) => fail_and_close(hwnd, &error),
            }
        }
        Ok(SceneKeyAction::OpenLegacyRecordingDevices) => {
            open_legacy_recording_devices_from_scene(hwnd);
            LRESULT(0)
        }
        Err(error) => fail_and_close(hwnd, &error),
    }
}

fn handle_scene_destroy_message(hwnd: WindowHandle) -> LRESULT {
    unregister_scene_window(hwnd);
    SCENE_APP_STATE.with(|state| {
        if let Some(state) = state.borrow_mut().as_mut() {
            state.chrome_tooltip.destroy();
        }
        let _ = state.borrow_mut().take();
    });
    hwnd.post_quit_message();
    LRESULT(0)
}

fn handle_scene_demo_mode_state_changed(hwnd: WindowHandle) -> LRESULT {
    // windowing[impl demo-mode.live-audio-device-scramble]
    match with_scene_app_state(|state| {
        sync_demo_mode_state(state);
        render_scene_window_frame(state, hwnd, None, true)
    }) {
        Ok(()) => LRESULT(0),
        Err(error) => fail_and_close(hwnd, &error),
    }
}

fn handle_scene_timeline_document_changed(hwnd: WindowHandle) -> LRESULT {
    match with_scene_app_state(|state| {
        if state.timeline_document.is_some() {
            render_scene_window_frame(state, hwnd, None, true)?;
        }
        Ok(())
    }) {
        Ok(()) => LRESULT(0),
        Err(error) => fail_and_close(hwnd, &error),
    }
}

fn handle_scene_timeline_document_command(hwnd: WindowHandle) -> LRESULT {
    match with_scene_app_state(|state| {
        let commands = state
            .timeline_document_command_receiver
            .as_ref()
            .map(|receiver| receiver.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        let mut changed = false;
        for command in commands {
            changed |= apply_timeline_document_command(state, command);
        }
        if changed {
            render_scene_window_frame(state, hwnd, None, true)?;
        }
        Ok(())
    }) {
        Ok(()) => LRESULT(0),
        Err(error) => fail_and_close(hwnd, &error),
    }
}

fn handle_scene_timeline_transcription_worker_completed(hwnd: WindowHandle) -> LRESULT {
    // timeline[impl transcription.completion-refresh]
    match with_scene_app_state(|state| {
        if state.scene_kind == SceneWindowKind::Timeline && state.timeline_document.is_some() {
            render_scene_window_frame(state, hwnd, None, true)?;
        }
        Ok(())
    }) {
        Ok(()) => LRESULT(0),
        Err(error) => fail_and_close(hwnd, &error),
    }
}

fn post_timeline_document_command(target: isize) {
    let hwnd = HWND(target as *mut c_void);
    // Safety: the target value comes from a live scene HWND; stale windows are tolerated by PostMessageW.
    let _ = unsafe {
        PostMessageW(
            Some(hwnd),
            TIMELINE_DOCUMENT_COMMAND_MESSAGE,
            WPARAM(0),
            LPARAM(0),
        )
    };
}

fn send_timeline_document_command(state: &SceneAppState, command: TimelineDocumentCommand) -> bool {
    let Some(sender) = state.timeline_document_command_sender.as_ref() else {
        return false;
    };
    let Some(target) = state.timeline_document_command_target else {
        return false;
    };
    if sender.send(command).is_err() {
        return false;
    }
    post_timeline_document_command(target);
    true
}

fn apply_timeline_document_command(
    state: &mut SceneAppState,
    command: TimelineDocumentCommand,
) -> bool {
    match command {
        TimelineDocumentCommand::ApplyTranscriptionSettings(settings) => {
            apply_timeline_transcription_settings_to_document(state, settings)
        }
        TimelineDocumentCommand::AppendTextTrackForTranscriptionSettings { track_id } => {
            let Some(document) = timeline_document_handle_mut(state) else {
                return false;
            };
            let text_track_id = document.append_text_track();
            document.set_transcription_track_target_text_track(track_id, Some(text_track_id))
        }
        TimelineDocumentCommand::AppendMicrophoneTrackForTranscriptionSettings {
            track_id,
            device_name,
            device_id,
        } => {
            let Some(document) = timeline_document_handle_mut(state) else {
                return false;
            };
            let audio_track_id =
                document.append_microphone_track_for_device_id(device_name, device_id);
            document.set_transcription_track_target_audio_track(track_id, Some(audio_track_id))
        }
        TimelineDocumentCommand::SetTranscriptionAutomation {
            track_id,
            advance_boundaries,
            submit_chunks,
        } => timeline_document_handle_mut(state).is_some_and(|document| {
            document.set_transcription_track_automation(track_id, advance_boundaries, submit_chunks)
        }),
    }
}

fn handle_scene_timeline_playground_detail_changed(hwnd: WindowHandle) -> LRESULT {
    match with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false)) {
        Ok(()) => LRESULT(0),
        Err(error) => fail_and_close(hwnd, &error),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "scene pointer dispatch keeps the chrome, diagnostics, launcher, and picker hit paths together"
)]
fn handle_scene_left_button_down(hwnd: WindowHandle, lparam: LPARAM) -> eyre::Result<bool> {
    let point = ClientPoint::from_lparam(lparam);
    let selection_mode = if alt_key_is_down() {
        TerminalSelectionMode::Block
    } else {
        TerminalSelectionMode::Linear
    };

    let action = with_scene_app_state(|state| {
        state.pointer_position = Some(point);
        state.chrome_tooltip.hide(hwnd);
        state.pressed_target = None;
        state.diagnostic_selection_drag_point = None;
        state.pending_timeline_selection = None;
        state.pending_timeline_text_block = None;
        state.timeline_pan_drag = None;
        let layout = scene_client_layout(hwnd, state)?;

        if let Some(button) = window_chrome_button_at_point(layout, point) {
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::ChromeButton(button));
            hwnd.capture_mouse();
            if button == WindowChromeButton::Diagnostics {
                scene_toggle_diagnostics_panel(state);
                return Ok(ScenePointerAction::RenderOnly);
            }
            if button == WindowChromeButton::Pin {
                scene_toggle_pin(state, hwnd)?;
                return Ok(ScenePointerAction::RenderOnly);
            }
            return Ok(ScenePointerAction::WindowChrome(button));
        }

        if state.diagnostics_visible
            && let Some(cell) = scene_diagnostic_cell_from_client_point(
                layout,
                point,
                state.diagnostic_cell_width,
                state.diagnostic_cell_height,
                false,
            )
        {
            state.diagnostic_selection = None;
            state.pending_diagnostic_selection = Some(PendingTerminalSelection {
                origin: point,
                anchor: cell,
                mode: selection_mode,
            });
            state.diagnostic_selection_drag_point = Some(point);
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::RenderOnly);
        }

        if state.scene_kind == SceneWindowKind::TimelinePlayground
            && !state.diagnostics_visible
            && let Some(target) = timeline_playground_target_at_point(state, layout, point)?
        {
            pin_timeline_playground_detail_from_target(state, hwnd, target)?;
            state.pressed_target = Some(ScenePressedTarget::Action(
                SceneAction::OpenTimelinePlayground,
            ));
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::RenderOnly);
        }

        if state.scene_kind == SceneWindowKind::ModelWarning {
            if windows_scene::model_warning_prepare_rect(layout).contains(point) {
                state.model_warning_prepare_started_at = Some(Instant::now());
                state.pressed_target = Some(ScenePressedTarget::ModelWarningPrepare);
                hwnd.capture_mouse();
                return Ok(ScenePointerAction::RenderOnly);
            }
            if let Some(index) = model_warning_open_button_at_point(state, layout, point) {
                open_model_warning_location(state, index);
                return Ok(ScenePointerAction::RenderOnly);
            }
            if let Some(index) = model_warning_copy_button_at_point(state, layout, point) {
                copy_model_warning_location(state, index);
                return Ok(ScenePointerAction::RenderOnly);
            }
        }

        if state.scene_kind == SceneWindowKind::Logs
            && !state.diagnostics_visible
            && let Some(control) = windows_scene::logs_control_at_point(layout, point)
        {
            state.pressed_target = Some(ScenePressedTarget::LogsControl(control));
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::RenderOnly);
        }

        if state.scene_kind == SceneWindowKind::TimelineTranscriptionSettings {
            if let Some(target) =
                timeline_transcription_settings_target_at_point(state, layout, point)
            {
                if let Some(settings) = state.timeline_transcription_settings.as_mut() {
                    settings.selected_column =
                        timeline_transcription_settings_target_column(target);
                    settings.dragging_target = Some(target);
                    settings.drag_position = Some(point);
                }
                state.pressed_target = Some(
                    ScenePressedTarget::TimelineTranscriptionSettingsTarget(target),
                );
                hwnd.capture_mouse();
                return Ok(ScenePointerAction::RenderOnly);
            }

            if let Some(target) = timeline_transcription_settings_socket_at_point(layout, point)
                && let Some(settings) = state.timeline_transcription_settings.as_mut()
            {
                settings.selected_column = timeline_transcription_settings_target_column(target);
                match target {
                    windows_scene::TimelineTranscriptionSettingsTarget::Model => {
                        if settings.model_target_docked {
                            if let Some(target_point) =
                                timeline_transcription_settings_target_cursor_jump_point(
                                    state, layout, target,
                                )
                            {
                                move_pointer_to_client_point(hwnd, target_point)?;
                                state.pointer_position = Some(target_point);
                            }
                            return Ok(ScenePointerAction::RenderOnly);
                        }
                    }
                    windows_scene::TimelineTranscriptionSettingsTarget::InputAudioTrack => {
                        if settings.input_target_docked {
                            if let Some(target_point) =
                                timeline_transcription_settings_target_cursor_jump_point(
                                    state, layout, target,
                                )
                            {
                                move_pointer_to_client_point(hwnd, target_point)?;
                                state.pointer_position = Some(target_point);
                            }
                            return Ok(ScenePointerAction::RenderOnly);
                        }
                    }
                    windows_scene::TimelineTranscriptionSettingsTarget::OutputTextTrack => {
                        if settings.output_target_docked {
                            if let Some(target_point) =
                                timeline_transcription_settings_target_cursor_jump_point(
                                    state, layout, target,
                                )
                            {
                                move_pointer_to_client_point(hwnd, target_point)?;
                                state.pointer_position = Some(target_point);
                            }
                            return Ok(ScenePointerAction::RenderOnly);
                        }
                    }
                }
            }

            if timeline_transcription_settings_add_text_track_button_at_point(layout, point) {
                append_text_track_from_timeline_transcription_settings(state);
                return Ok(ScenePointerAction::RenderOnly);
            }

            if timeline_transcription_settings_add_audio_track_button_at_point(layout, point) {
                open_audio_track_picker_from_timeline_transcription_settings(state);
                return Ok(ScenePointerAction::RenderOnly);
            }

            if windows_scene::timeline_transcription_settings_advance_boundaries_toggle_rect(layout)
                .contains(point)
            {
                toggle_timeline_transcription_settings_advance_boundaries(state);
                return Ok(ScenePointerAction::RenderOnly);
            }

            if windows_scene::timeline_transcription_settings_submit_chunks_toggle_rect(layout)
                .contains(point)
            {
                toggle_timeline_transcription_settings_submit_chunks(state);
                return Ok(ScenePointerAction::RenderOnly);
            }

            if windows_scene::timeline_transcription_settings_manual_flush_button_rect(layout)
                .contains(point)
            {
                request_timeline_transcription_manual_flush(state);
                return Ok(ScenePointerAction::RenderOnly);
            }

            if let Some(model_index) =
                timeline_transcription_settings_model_row_at_point(layout, point)
            {
                select_timeline_transcription_settings_model(state, model_index, true);
                return Ok(ScenePointerAction::RenderOnly);
            }

            if let Some(target_index) =
                timeline_transcription_settings_input_row_at_point(state, layout, point)
            {
                select_timeline_transcription_settings_input_audio_track(state, target_index, true);
                return Ok(ScenePointerAction::RenderOnly);
            }

            if let Some(target_index) =
                timeline_transcription_settings_output_row_at_point(state, layout, point)
            {
                select_timeline_transcription_settings_output_text_track(state, target_index, true);
                return Ok(ScenePointerAction::RenderOnly);
            }

            if scene_drag_handle_contains(layout, point) {
                state.diagnostic_selection = None;
                state.pending_diagnostic_selection = None;
                begin_system_window_drag(hwnd, point)?;
                return Ok(ScenePointerAction::Handled);
            }

            return Ok(ScenePointerAction::NotHandled);
        }

        if timeline_add_track_button_at_point(state, layout, point) {
            let action = SceneAction::OpenTimelineTrackMenu;
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::Action(action));
            hwnd.capture_mouse();
            state.last_clicked_action = Some(ClickState {
                action,
                clicked_at: Instant::now(),
            });
            return Ok(ScenePointerAction::Invoke(action));
        }

        if let Some(action) = timeline_viewport_control_at_point(state, layout, point) {
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::Action(action));
            hwnd.capture_mouse();
            state.last_clicked_action = Some(ClickState {
                action,
                clicked_at: Instant::now(),
            });
            return Ok(ScenePointerAction::Invoke(action));
        }

        if timeline_transport_play_button_at_point(state, layout, point) {
            // timeline[impl transport.spacebar-playback]
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::TimelineTransportPlayPause);
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::ToggleAudioInputPlayback);
        }

        if let Some(track_index) = timeline_track_playback_button_at_point(state, layout, point) {
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::TimelineTrackPlayback(track_index));
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::ToggleAudioInputPlayback);
        }

        if let Some(track_index) = timeline_track_loopback_button_at_point(state, layout, point) {
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::TimelineTrackLoopback(track_index));
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::ToggleAudioInputLoopback);
        }

        if let Some(track_index) = timeline_track_reorder_handle_at_point(state, layout, point) {
            state.pending_diagnostic_selection = None;
            state.pressed_target =
                Some(ScenePressedTarget::TimelineTrackReorderHandle(track_index));
            state.pending_timeline_track_reorder =
                timeline_document_snapshot(state).map(|document| PendingTimelineTrackReorder {
                    source_index: track_index,
                    current_index: track_index,
                    original_track_order: document
                        .tracks()
                        .iter()
                        .map(crate::timeline::TimelineTrack::id)
                        .collect(),
                });
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::RenderOnly);
        }

        if let Some(track_index) =
            timeline_track_transcription_toggle_button_at_point(state, layout, point)
        {
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::TimelineTrackTranscriptionToggle(
                track_index,
            ));
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::ToggleAudioInputTranscription);
        }

        if let Some(track_index) =
            timeline_track_transcription_settings_button_at_point(state, layout, point)
        {
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::TimelineTrackTranscriptionSettings(
                track_index,
            ));
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::OpenTimelineTranscriptionSettings(
                track_index,
            ));
        }

        if let Some(track_index) = timeline_track_record_button_at_point(state, layout, point) {
            // timeline[impl recording.append-live]
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::TimelineTrackRecord(track_index));
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::ToggleAudioInputRecording);
        }

        if let Some(head) = timeline_audio_head_at_point(state, layout, point) {
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::TimelineAudioHead(head));
            let seconds = timeline_seconds_from_client_point(state, layout, point);
            if let Some(device_window) = state.audio_input_device_window.as_mut() {
                device_window.begin_head_interaction(head, seconds);
            }
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::RenderOnly);
        }

        if state.timeline_tool == TimelineInteractionTool::Brush
            && let Some((track_index, track_id)) =
                timeline_text_track_at_point(state, layout, point)
        {
            state.pending_diagnostic_selection = None;
            state.diagnostic_selection = None;
            state.timeline_selection = Some(windows_scene::TimelineRectSelection::new(
                TimelineTimeRangeNs::new(
                    timeline_time_from_client_point(state, layout, point),
                    timeline_time_from_client_point(state, layout, point),
                ),
                windows_scene::timeline_track_vertical_range(track_index),
            ));
            state.pending_timeline_text_block = Some(PendingTimelineTextBlock {
                origin: point,
                anchor_time: timeline_time_from_client_point(state, layout, point),
                track_index,
                track_id,
            });
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::RenderOnly);
        }

        if timeline_selection_surface_at_point(state, layout, point) {
            // timeline[impl selection.rectangle]
            // timeline[impl selection.ruler-all-tracks]
            state.pending_diagnostic_selection = None;
            state.diagnostic_selection = None;
            state.timeline_selection = None;
            state.pending_timeline_selection = Some(PendingTimelineSelection {
                origin: point,
                anchor_time: timeline_time_from_client_point(state, layout, point),
                anchor_track_y: timeline_track_world_y_from_client_point(state, layout, point),
                include_all_tracks: timeline_layout(layout).time_axis_rect.contains(point),
            });
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::RenderOnly);
        }

        if !state.diagnostics_visible
            && let Some(cell) =
                scene_pretty_text_cell_from_client_point(state, layout, point, false)
        {
            // windowing[impl scene.pretty-text.selection]
            state.diagnostic_selection = None;
            state.pending_diagnostic_selection = Some(PendingTerminalSelection {
                origin: point,
                anchor: cell,
                mode: selection_mode,
            });
            state.diagnostic_selection_drag_point = Some(point);
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::RenderOnly);
        }

        if let Some(action) = scene_action_at_point(state.scene_kind, layout, point) {
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::Action(action));
            hwnd.capture_mouse();
            state.last_clicked_action = Some(ClickState {
                action,
                clicked_at: Instant::now(),
            });
            return Ok(ScenePointerAction::Invoke(action));
        }

        if let Some(cell) = cursor_gallery_cell_at_point(state, layout, point) {
            // windowing[impl cursor-gallery.virtual-navigation]
            state.pending_diagnostic_selection = None;
            state.scene_action_selected_index = cell.index;
            state.scene_virtual_cursor = Some(rect_center(cell.hit_rect()));
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::RenderOnly);
        }

        if demo_mode_button_at_point(state, layout, point) {
            // windowing[impl demo-mode.window]
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::DemoModeButton);
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::RenderOnly);
        }

        if demo_mode_scramble_toggle_at_point(state, layout, point) {
            // windowing[impl demo-mode.input-device-identifier-scramble]
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::DemoModeScrambleToggle);
            toggle_demo_mode_scramble_input_device_identifiers(state)?;
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::RenderOnly);
        }

        if legacy_recording_devices_button_at_point(state, layout, point) {
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::LegacyRecordingDevices);
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::OpenLegacyRecordingDevices);
        }

        if audio_input_device_detail_legacy_recording_button_at_point(state, layout, point) {
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::LegacyRecordingDevices);
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::OpenLegacyRecordingDevices);
        }

        if let Some(index) = audio_input_device_at_point(state, layout, point) {
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::AudioInputDevice(index));
            state.audio_input_picker.select_index(index);
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::ChooseAudioInputDevice(index));
        }

        if let Some(index) = audio_daemon_model_at_point(state, layout, point) {
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::AudioDaemonModel(index));
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::SelectAudioDaemonModel(index));
        }

        if audio_daemon_cuda_check_button_at_point(state, layout, point) {
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::AudioDaemonCudaCheck);
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::RunAudioDaemonCudaCheck);
        }

        if audio_input_device_arm_button_at_point(state, layout, point) {
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::AudioInputDeviceArm);
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::ToggleAudioInputRecording);
        }

        if audio_input_device_transcription_button_at_point(state, layout, point) {
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::AudioInputTranscription);
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::ToggleAudioInputTranscription);
        }

        if audio_input_device_transcription_flush_button_at_point(state, layout, point) {
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::AudioInputTranscriptionFlush);
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::FlushAudioInputTranscription);
        }

        if audio_input_device_play_pause_button_at_point(state, layout, point) {
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::AudioInputPlayback);
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::ToggleAudioInputPlayback);
        }

        if audio_input_device_loopback_button_at_point(state, layout, point) {
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::AudioInputDeviceLoopback);
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::ToggleAudioInputLoopback);
        }

        if let Some(head) = audio_input_timeline_head_at_point(state, layout, point) {
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::AudioInputTimelineHead(head));
            if let Some(device_window) = state.audio_input_device_window.as_mut() {
                let body_rect = layout.terminal_panel_rect().inset(24);
                let detail_layout = windows_scene::audio_input_device_detail_layout(body_rect);
                let duration_seconds = device_window.runtime.duration_seconds();
                let seconds = windows_scene::audio_input_timeline_seconds_from_point(
                    detail_layout.waveform_rect,
                    duration_seconds,
                    point,
                );
                device_window.begin_head_interaction(head, seconds);
            }
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::RenderOnly);
        }

        if audio_input_timeline_at_point(state, layout, point).is_some() {
            state.pending_diagnostic_selection = None;
            state.pressed_target = Some(ScenePressedTarget::AudioInputTimeline);
            if let Some(device_window) = state.audio_input_device_window.as_mut() {
                let body_rect = layout.terminal_panel_rect().inset(24);
                let detail_layout = windows_scene::audio_input_device_detail_layout(body_rect);
                let duration_seconds = device_window.runtime.duration_seconds();
                let seconds = windows_scene::audio_input_timeline_seconds_from_point(
                    detail_layout.waveform_rect,
                    duration_seconds,
                    point,
                );
                let point_x = point.to_win32_point().map_or(0, |point| point.x);
                device_window.begin_timeline_interaction(seconds, point_x);
            }
            hwnd.capture_mouse();
            return Ok(ScenePointerAction::BeginAudioInputTimeline);
        }

        if scene_drag_handle_contains(layout, point) {
            state.diagnostic_selection = None;
            state.pending_diagnostic_selection = None;
            begin_system_window_drag(hwnd, point)?;
            return Ok(ScenePointerAction::Handled);
        }

        state.diagnostic_selection = None;
        Ok(ScenePointerAction::NotHandled)
    })?;

    match action {
        ScenePointerAction::NotHandled => Ok(false),
        ScenePointerAction::Handled => Ok(true),
        ScenePointerAction::RenderOnly => {
            with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
            Ok(true)
        }
        ScenePointerAction::WindowChrome(button) => {
            execute_window_chrome_button(hwnd, button);
            Ok(true)
        }
        ScenePointerAction::OpenTimelineTranscriptionSettings(track_index) => {
            if let Err(error) = open_timeline_transcription_settings_window_from_scene(track_index)
            {
                error!(
                    ?error,
                    "failed to launch timeline transcription settings window"
                );
            }
            with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
            Ok(true)
        }
        ScenePointerAction::Invoke(action) => {
            let disposition = invoke_scene_action(hwnd, action)?;
            if disposition == SceneActionDisposition::CloseWindow {
                hwnd.post_close();
            }
            Ok(true)
        }
        ScenePointerAction::ChooseAudioInputDevice(index) => {
            let device = with_scene_app_state(|state| {
                Ok(state.audio_input_picker.devices.get(index).cloned())
            })?;
            commit_audio_input_picker_selection_from_scene(hwnd, device);
            with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
            Ok(true)
        }
        ScenePointerAction::ToggleAudioInputRecording => {
            with_scene_app_state(|state| {
                if let Some(device_window) = state.audio_input_device_window.as_mut() {
                    device_window.toggle_recording()?;
                }
                render_scene_window_frame(state, hwnd, None, false)
            })?;
            Ok(true)
        }
        ScenePointerAction::ToggleAudioInputTranscription => {
            with_scene_app_state(|state| {
                toggle_audio_input_transcription_from_scene_state(state);
                render_scene_window_frame(state, hwnd, None, false)
            })?;
            Ok(true)
        }
        ScenePointerAction::FlushAudioInputTranscription => {
            with_scene_app_state(|state| {
                if let Some(device_window) = state.audio_input_device_window.as_mut() {
                    device_window.flush_transcription_chunk();
                }
                render_scene_window_frame(state, hwnd, None, false)
            })?;
            Ok(true)
        }
        ScenePointerAction::SelectAudioDaemonModel(index) => {
            super::audio_transcription_select_model_index(index);
            with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
            Ok(true)
        }
        ScenePointerAction::RunAudioDaemonCudaCheck => {
            show_audio_daemon_cuda_check_result();
            with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
            Ok(true)
        }
        ScenePointerAction::ToggleAudioInputPlayback => {
            with_scene_app_state(|state| {
                if let Some(device_window) = state.audio_input_device_window.as_mut() {
                    device_window.toggle_playback()?;
                }
                render_scene_window_frame(state, hwnd, None, false)
            })?;
            Ok(true)
        }
        ScenePointerAction::ToggleAudioInputLoopback => {
            with_scene_app_state(|state| {
                if let Some(device_window) = state.audio_input_device_window.as_mut() {
                    device_window.toggle_loopback()?;
                }
                render_scene_window_frame(state, hwnd, None, false)
            })?;
            Ok(true)
        }
        ScenePointerAction::BeginAudioInputTimeline => {
            with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
            Ok(true)
        }
        ScenePointerAction::OpenLegacyRecordingDevices => {
            open_legacy_recording_devices_from_scene(hwnd);
            with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
            Ok(true)
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "pointer release routing for the scene window stays easier to follow in one handler"
)]
fn handle_scene_left_button_up(hwnd: WindowHandle, lparam: LPARAM) -> eyre::Result<bool> {
    let point = ClientPoint::from_lparam(lparam);
    let should_release_capture = with_scene_app_state(|state| {
        Ok(state.pressed_target.is_some()
            || state.pending_diagnostic_selection.is_some()
            || state.pending_timeline_selection.is_some())
    })?;
    if should_release_capture {
        hwnd.release_mouse_capture();
    }

    let action = with_scene_app_state(|state| {
        state.pointer_position = Some(point);
        let pressed_target = state.pressed_target.take();

        if matches!(
            pressed_target,
            Some(ScenePressedTarget::ModelWarningPrepare)
        ) {
            state.model_warning_prepare_started_at = None;
            return Ok(ScenePointerAction::RenderOnly);
        }

        if let Some(ScenePressedTarget::TimelineTranscriptionSettingsTarget(target)) =
            pressed_target
        {
            clear_timeline_transcription_settings_drag(state);
            let layout = scene_client_layout(hwnd, state)?;
            match target {
                windows_scene::TimelineTranscriptionSettingsTarget::Model => {
                    if let Some(model_index) =
                        timeline_transcription_settings_model_row_at_point(layout, point)
                    {
                        select_timeline_transcription_settings_model(state, model_index, true);
                    } else if timeline_transcription_settings_socket_at_point(layout, point)
                        == Some(windows_scene::TimelineTranscriptionSettingsTarget::Model)
                        && let Some(settings) = state.timeline_transcription_settings.as_mut()
                    {
                        settings.selected_column =
                            windows_scene::TimelineTranscriptionSettingsColumn::Model;
                        settings.model_target_docked = false;
                    }
                }
                windows_scene::TimelineTranscriptionSettingsTarget::InputAudioTrack => {
                    if let Some(target_index) =
                        timeline_transcription_settings_input_row_at_point(state, layout, point)
                    {
                        select_timeline_transcription_settings_input_audio_track(
                            state,
                            target_index,
                            true,
                        );
                    } else if timeline_transcription_settings_socket_at_point(layout, point)
                        == Some(windows_scene::TimelineTranscriptionSettingsTarget::InputAudioTrack)
                        && let Some(settings) = state.timeline_transcription_settings.as_mut()
                    {
                        settings.selected_column =
                            windows_scene::TimelineTranscriptionSettingsColumn::InputAudioTrack;
                        settings.input_target_docked = false;
                    }
                }
                windows_scene::TimelineTranscriptionSettingsTarget::OutputTextTrack => {
                    if let Some(target_index) =
                        timeline_transcription_settings_output_row_at_point(state, layout, point)
                    {
                        select_timeline_transcription_settings_output_text_track(
                            state,
                            target_index,
                            true,
                        );
                    } else if timeline_transcription_settings_socket_at_point(layout, point)
                        == Some(windows_scene::TimelineTranscriptionSettingsTarget::OutputTextTrack)
                        && let Some(settings) = state.timeline_transcription_settings.as_mut()
                    {
                        settings.selected_column =
                            windows_scene::TimelineTranscriptionSettingsColumn::OutputTextTrack;
                        settings.output_target_docked = false;
                    }
                }
            }
            return Ok(ScenePointerAction::RenderOnly);
        }

        if let Some(pending_selection) = state.pending_diagnostic_selection.take() {
            state.diagnostic_selection_drag_point = None;
            let layout = scene_client_layout(hwnd, state)?;
            let cell = if state.diagnostics_visible {
                scene_diagnostic_cell_from_client_point(
                    layout,
                    point,
                    state.diagnostic_cell_width,
                    state.diagnostic_cell_height,
                    true,
                )
            } else {
                scene_pretty_text_cell_from_client_point(state, layout, point, true)
            };
            if let Some(selection) =
                complete_pending_terminal_selection(pending_selection, point, cell)
            {
                state.diagnostic_selection = Some(selection);
            }
            return Ok(ScenePointerAction::RenderOnly);
        }

        if state.pending_timeline_track_reorder.take().is_some() {
            state.pressed_target = None;
            return Ok(ScenePointerAction::RenderOnly);
        }

        if let Some(pending_text_block) = state.pending_timeline_text_block.take() {
            let layout = scene_client_layout(hwnd, state)?;
            state.timeline_selection = None;
            let end_time = timeline_time_from_client_point(state, layout, point);
            if point != pending_text_block.origin
                && let Some(document) = timeline_document_handle_mut(state)
            {
                let _ = document.append_empty_text_block(
                    pending_text_block.track_id,
                    TimelineTimeRangeNs::from_unordered(pending_text_block.anchor_time, end_time),
                );
            }
            return Ok(ScenePointerAction::RenderOnly);
        }

        if let Some(pending_selection) = state.pending_timeline_selection.take() {
            let layout = scene_client_layout(hwnd, state)?;
            state.timeline_selection =
                timeline_selection_from_pending(state, layout, pending_selection, point);
            return Ok(ScenePointerAction::RenderOnly);
        }

        if pressed_target == Some(ScenePressedTarget::AudioInputTimeline) {
            let layout = scene_client_layout(hwnd, state)?;
            if let Some(device_window) = state.audio_input_device_window.as_mut() {
                let body_rect = layout.terminal_panel_rect().inset(24);
                let detail_layout = windows_scene::audio_input_device_detail_layout(body_rect);
                let duration_seconds = device_window.runtime.duration_seconds();
                let seconds = windows_scene::audio_input_timeline_seconds_from_point(
                    detail_layout.waveform_rect,
                    duration_seconds,
                    point,
                );
                let point_x = point.to_win32_point().map_or(0, |point| point.x);
                device_window.complete_timeline_interaction(seconds, point_x)?;
            }
            return Ok(ScenePointerAction::RenderOnly);
        }

        if matches!(
            pressed_target,
            Some(
                ScenePressedTarget::AudioInputTimelineHead(_)
                    | ScenePressedTarget::TimelineAudioHead(_)
            )
        ) {
            let layout = scene_client_layout(hwnd, state)?;
            let timeline_head_seconds = matches!(
                pressed_target,
                Some(ScenePressedTarget::TimelineAudioHead(_))
            )
            .then(|| timeline_seconds_from_client_point(state, layout, point));
            if let Some(device_window) = state.audio_input_device_window.as_mut() {
                let seconds = if matches!(
                    pressed_target,
                    Some(ScenePressedTarget::TimelineAudioHead(_))
                ) {
                    timeline_head_seconds.unwrap_or_default()
                } else {
                    let body_rect = layout.terminal_panel_rect().inset(24);
                    let detail_layout = windows_scene::audio_input_device_detail_layout(body_rect);
                    let duration_seconds = device_window.runtime.duration_seconds();
                    windows_scene::audio_input_timeline_seconds_from_point(
                        detail_layout.waveform_rect,
                        duration_seconds,
                        point,
                    )
                };
                let point_x = point.to_win32_point().map_or(0, |point| point.x);
                device_window.complete_timeline_interaction(seconds, point_x)?;
            }
            return Ok(ScenePointerAction::RenderOnly);
        }

        if let Some(ScenePressedTarget::LogsControl(control)) = pressed_target {
            let layout = scene_client_layout(hwnd, state)?;
            if state.scene_kind == SceneWindowKind::Logs
                && !state.diagnostics_visible
                && windows_scene::logs_control_at_point(layout, point) == Some(control)
            {
                apply_logs_control(state, hwnd, layout, control);
            }
            return Ok(ScenePointerAction::RenderOnly);
        }

        if pressed_target.is_some() {
            return Ok(ScenePointerAction::RenderOnly);
        }

        Ok(ScenePointerAction::NotHandled)
    })?;

    match action {
        ScenePointerAction::NotHandled => Ok(false),
        ScenePointerAction::Handled => Ok(true),
        ScenePointerAction::RenderOnly => {
            with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
            Ok(true)
        }
        ScenePointerAction::WindowChrome(button) => {
            execute_window_chrome_button(hwnd, button);
            Ok(true)
        }
        ScenePointerAction::Invoke(action) => {
            let (app_home, vt_engine) =
                with_scene_app_state(|state| Ok((state.app_home.clone(), state.vt_engine)))?;
            let disposition = perform_scene_action(&app_home, vt_engine, action)?;
            with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
            if disposition == SceneActionDisposition::CloseWindow {
                hwnd.post_close();
            }
            Ok(true)
        }
        ScenePointerAction::OpenTimelineTranscriptionSettings(track_index) => {
            if let Err(error) = open_timeline_transcription_settings_window_from_scene(track_index)
            {
                error!(
                    ?error,
                    "failed to launch timeline transcription settings window"
                );
            }
            with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
            Ok(true)
        }
        ScenePointerAction::ChooseAudioInputDevice(index) => {
            let device = with_scene_app_state(|state| {
                Ok(state.audio_input_picker.devices.get(index).cloned())
            })?;
            commit_audio_input_picker_selection_from_scene(hwnd, device);
            with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
            Ok(true)
        }
        ScenePointerAction::ToggleAudioInputRecording => {
            with_scene_app_state(|state| {
                if let Some(device_window) = state.audio_input_device_window.as_mut() {
                    device_window.toggle_recording()?;
                }
                render_scene_window_frame(state, hwnd, None, false)
            })?;
            Ok(true)
        }
        ScenePointerAction::ToggleAudioInputTranscription => {
            with_scene_app_state(|state| {
                toggle_audio_input_transcription_from_scene_state(state);
                render_scene_window_frame(state, hwnd, None, false)
            })?;
            Ok(true)
        }
        ScenePointerAction::FlushAudioInputTranscription => {
            with_scene_app_state(|state| {
                if let Some(device_window) = state.audio_input_device_window.as_mut() {
                    device_window.flush_transcription_chunk();
                }
                render_scene_window_frame(state, hwnd, None, false)
            })?;
            Ok(true)
        }
        ScenePointerAction::SelectAudioDaemonModel(index) => {
            super::audio_transcription_select_model_index(index);
            with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
            Ok(true)
        }
        ScenePointerAction::RunAudioDaemonCudaCheck => {
            show_audio_daemon_cuda_check_result();
            with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
            Ok(true)
        }
        ScenePointerAction::ToggleAudioInputPlayback => {
            with_scene_app_state(|state| {
                if let Some(device_window) = state.audio_input_device_window.as_mut() {
                    device_window.toggle_playback()?;
                }
                render_scene_window_frame(state, hwnd, None, false)
            })?;
            Ok(true)
        }
        ScenePointerAction::ToggleAudioInputLoopback => {
            with_scene_app_state(|state| {
                if let Some(device_window) = state.audio_input_device_window.as_mut() {
                    device_window.toggle_loopback()?;
                }
                render_scene_window_frame(state, hwnd, None, false)
            })?;
            Ok(true)
        }
        ScenePointerAction::BeginAudioInputTimeline => {
            with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
            Ok(true)
        }
        ScenePointerAction::OpenLegacyRecordingDevices => {
            open_legacy_recording_devices_from_scene(hwnd);
            with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
            Ok(true)
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "scene pointer move handling keeps the interaction state machines together for local reasoning"
)]
fn handle_scene_mouse_move(
    hwnd: WindowHandle,
    wparam: WPARAM,
    lparam: LPARAM,
) -> eyre::Result<bool> {
    let point = ClientPoint::from_lparam(lparam);
    let previous_pointer = with_scene_app_state(|state| {
        let previous = state.pointer_position;
        state.pointer_position = Some(point);
        update_scene_chrome_tooltip(state, hwnd, point)?;
        Ok(previous)
    })?;

    let transcription_target_dragging = with_scene_app_state(|state| {
        let dragging_target = state
            .timeline_transcription_settings
            .and_then(|settings| settings.dragging_target);
        if dragging_target.is_none() {
            return Ok(false);
        }

        if (wparam.0 & 0x0001) == 0 {
            clear_timeline_transcription_settings_drag(state);
            state.pressed_target = None;
            return Ok(true);
        }

        if let Some(settings) = state.timeline_transcription_settings.as_mut() {
            settings.drag_position = Some(point);
        }
        Ok(true)
    })?;
    if transcription_target_dragging {
        with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
        return Ok(true);
    }

    let timeline_playground_hover_changed = with_scene_app_state(|state| {
        if state.scene_kind != SceneWindowKind::TimelinePlayground || state.diagnostics_visible {
            return Ok(false);
        }
        let layout = scene_client_layout(hwnd, state)?;
        let target = timeline_playground_target_at_point(state, layout, point)?;
        let Some(playground) = state.timeline_playground.as_mut() else {
            return Ok(false);
        };
        if playground.hovered_item == target {
            return Ok(false);
        }
        playground.hovered_item = target;
        if let Some(target) = target {
            update_timeline_playground_hover_detail_from_target(state, hwnd, target)?;
        }
        Ok(true)
    })?;
    if timeline_playground_hover_changed {
        with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
        return Ok(true);
    }

    let timeline_playground_pan_dragging = with_scene_app_state(|state| {
        let Some(pan_drag) = state.timeline_playground_pan_drag else {
            return Ok(false);
        };
        let layout = scene_client_layout(hwnd, state)?;
        let vertical_scroll_offset = state
            .timeline_playground
            .as_ref()
            .map_or(0, |playground| playground.vertical_scroll_offset);
        let playground_layout = windows_scene::timeline_playground_layout(
            layout.terminal_panel_rect().inset(24),
            vertical_scroll_offset,
        );
        let origin = pan_drag.origin.to_win32_point()?;
        let current = point.to_win32_point()?;
        let delta_x = current.x - origin.x;
        let delta_y = current.y - origin.y;
        let Some(playground) = state.timeline_playground.as_mut() else {
            state.timeline_playground_pan_drag = None;
            return Ok(true);
        };
        let render_plan =
            playground.dataset.render_plan(&playground.query(
                u32::try_from(playground_layout.content_rect.width().max(1)).unwrap_or(1),
            )?);
        apply_timeline_playground_pan_drag(
            playground,
            pan_drag,
            delta_x,
            delta_y,
            playground_layout.content_rect.width(),
            playground_layout,
            render_plan.rows().len(),
        );
        Ok(true)
    })?;
    if timeline_playground_pan_dragging {
        with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
        return Ok(true);
    }

    let timeline_pan_dragging = with_scene_app_state(|state| {
        let Some(pan_drag) = state.timeline_pan_drag else {
            return Ok(false);
        };
        let Some(document) = timeline_document_handle_mut(state) else {
            state.timeline_pan_drag = None;
            return Ok(true);
        };

        let origin = pan_drag.origin.to_win32_point()?;
        let current = point.to_win32_point()?;
        let delta_x = current.x - origin.x;
        let delta_y = current.y - origin.y;
        document.set_viewport(pan_drag.origin_viewport.pan_pixels(-delta_x));

        let layout = scene_client_layout(hwnd, state)?;
        state.timeline_vertical_scroll_offset =
            windows_scene::timeline_clamp_vertical_scroll_offset(
                timeline_layout(layout),
                timeline_track_count(state),
                pan_drag.origin_vertical_scroll_offset - delta_y,
            );
        Ok(true)
    })?;
    if timeline_pan_dragging {
        with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
        return Ok(true);
    }

    let timeline_track_reordering = with_scene_app_state(|state| {
        if state.pending_timeline_track_reorder.is_none() {
            return Ok(false);
        }

        if (wparam.0 & 0x0001) == 0 {
            state.pending_timeline_track_reorder = None;
            state.pressed_target = None;
            return Ok(true);
        }

        let layout = scene_client_layout(hwnd, state)?;
        let Some(next_index) = timeline_track_reorder_index_at_point(state, layout, point) else {
            return Ok(true);
        };
        let has_document = state.timeline_document.is_some();
        let Some(pending_reorder) = state.pending_timeline_track_reorder.as_mut() else {
            return Ok(false);
        };
        if next_index != pending_reorder.current_index {
            if !has_document {
                state.pending_timeline_track_reorder = None;
                state.pressed_target = None;
                return Ok(true);
            }
            let moved = state.timeline_document.as_mut().is_some_and(|document| {
                document.move_track(pending_reorder.current_index, next_index)
            });
            pending_reorder.current_index = next_index;
            if moved {
                broadcast_timeline_document_changed();
            }
        }
        Ok(true)
    })?;
    if timeline_track_reordering {
        with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
        return Ok(true);
    }

    let timeline_selection_dragging = with_scene_app_state(|state| {
        let Some(pending_selection) = state.pending_timeline_selection else {
            return Ok(false);
        };

        if (wparam.0 & 0x0001) == 0 {
            state.pending_timeline_selection = None;
            state.timeline_selection = None;
            return Ok(true);
        }

        // timeline[impl selection.rectangle]
        let layout = scene_client_layout(hwnd, state)?;
        state.timeline_selection =
            timeline_selection_from_pending(state, layout, pending_selection, point);
        Ok(true)
    })?;
    if timeline_selection_dragging {
        with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
        return Ok(true);
    }

    let timeline_text_block_dragging = with_scene_app_state(|state| {
        let Some(pending_text_block) = state.pending_timeline_text_block else {
            return Ok(false);
        };

        if (wparam.0 & 0x0001) == 0 {
            state.pending_timeline_text_block = None;
            state.timeline_selection = None;
            return Ok(true);
        }

        let layout = scene_client_layout(hwnd, state)?;
        state.timeline_selection = Some(windows_scene::TimelineRectSelection::new(
            TimelineTimeRangeNs::from_unordered(
                pending_text_block.anchor_time,
                timeline_time_from_client_point(state, layout, point),
            ),
            windows_scene::timeline_track_vertical_range(pending_text_block.track_index),
        ));
        Ok(true)
    })?;
    if timeline_text_block_dragging {
        with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
        return Ok(true);
    }

    let timeline_dragging = with_scene_app_state(|state| {
        if !matches!(
            state.pressed_target,
            Some(
                ScenePressedTarget::AudioInputTimeline
                    | ScenePressedTarget::AudioInputTimelineHead(_)
                    | ScenePressedTarget::TimelineAudioHead(_)
            )
        ) {
            return Ok(false);
        }
        let layout = scene_client_layout(hwnd, state)?;
        let timeline_head_seconds = matches!(
            state.pressed_target,
            Some(ScenePressedTarget::TimelineAudioHead(_))
        )
        .then(|| timeline_seconds_from_client_point(state, layout, point));
        if let Some(device_window) = state.audio_input_device_window.as_mut() {
            let seconds = if matches!(
                state.pressed_target,
                Some(ScenePressedTarget::TimelineAudioHead(_))
            ) {
                timeline_head_seconds.unwrap_or_default()
            } else {
                let body_rect = layout.terminal_panel_rect().inset(24);
                let detail_layout = windows_scene::audio_input_device_detail_layout(body_rect);
                let duration_seconds = device_window.runtime.duration_seconds();
                windows_scene::audio_input_timeline_seconds_from_point(
                    detail_layout.waveform_rect,
                    duration_seconds,
                    point,
                )
            };
            device_window.update_timeline_interaction(seconds);
        }
        Ok(true)
    })?;
    if timeline_dragging {
        with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
        return Ok(true);
    }

    let diagnostic_selection_result = with_scene_app_state(|state| {
        let Some(pending_selection) = state.pending_diagnostic_selection else {
            return Ok(None);
        };

        state.diagnostic_selection_drag_point = Some(point);

        let action = if (wparam.0 & 0x0001) == 0 {
            update_pending_terminal_selection_action(pending_selection, point, false, None)
        } else if point == pending_selection.origin {
            update_pending_terminal_selection_action(pending_selection, point, true, None)
        } else {
            let layout = scene_client_layout(hwnd, state)?;
            let cell = if state.diagnostics_visible {
                scene_diagnostic_cell_from_client_point(
                    layout,
                    point,
                    state.diagnostic_cell_width,
                    state.diagnostic_cell_height,
                    true,
                )
            } else {
                scene_pretty_text_cell_from_client_point(state, layout, point, true)
            };
            update_pending_terminal_selection_action(pending_selection, point, true, cell)
        };

        match action {
            PendingTerminalSelectionAction::KeepPending => Ok(Some(true)),
            PendingTerminalSelectionAction::ClearPending => {
                state.pending_diagnostic_selection = None;
                state.diagnostic_selection_drag_point = None;
                Ok(Some(state.diagnostic_selection.is_some()))
            }
            PendingTerminalSelectionAction::Update(selection) => {
                state.diagnostic_selection = Some(selection);
                Ok(Some(true))
            }
        }
    })?;

    if let Some(consumed) = diagnostic_selection_result {
        if consumed {
            with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
        }
        return Ok(consumed);
    }

    let should_render = with_scene_app_state(|state| {
        let layout = scene_client_layout(hwnd, state)?;
        Ok(
            scene_interactive_region_contains(state, layout, previous_pointer)
                || scene_interactive_region_contains(state, layout, Some(point)),
        )
    })?;

    if should_render {
        with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
        return Ok(true);
    }

    Ok(false)
}

fn handle_scene_right_button_up(hwnd: WindowHandle, lparam: LPARAM) -> eyre::Result<bool> {
    let point = ClientPoint::from_lparam(lparam);

    let released_timeline_pan = with_scene_app_state(|state| {
        state.pointer_position = Some(point);
        let released = state.timeline_pan_drag.take().is_some()
            || state.timeline_playground_pan_drag.take().is_some();
        if released {
            trace!(?point, "timeline right-button pan released");
        }
        Ok(released)
    })?;
    if released_timeline_pan {
        hwnd.release_mouse_capture();
        with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
        return Ok(true);
    }

    // windowing[impl diagnostics.text.selection-and-copy]

    let copy_text = with_scene_app_state(|state| {
        let layout = scene_client_layout(hwnd, state)?;
        let text = if state.diagnostics_visible {
            if !scene_diagnostic_text_rect(layout).contains(point) {
                return Ok(None);
            }

            let diagnostic_text = build_scene_diagnostic_text(state);
            if let Some(selection) = state.diagnostic_selection.take() {
                cell_grid::extract_selected_text(
                    scene_diagnostic_text_rect(layout),
                    &diagnostic_text,
                    state.diagnostic_cell_width,
                    state.diagnostic_cell_height,
                    selection,
                )
            } else {
                diagnostic_text
            }
        } else {
            let Some(target) = scene_pretty_text_target(state, layout) else {
                return Ok(None);
            };
            if !target.rect.contains(point) {
                return Ok(None);
            }

            if let Some(selection) = state.diagnostic_selection.take() {
                cell_grid::extract_selected_text(
                    target.rect,
                    &target.text,
                    target.cell_width,
                    target.cell_height,
                    selection,
                )
            } else {
                target.text
            }
        };
        Ok(Some(text))
    })?;

    let Some(copy_text) = copy_text else {
        return Ok(false);
    };

    if !copy_text.is_empty()
        && let Err(error) = write_clipboard(&copy_text)
    {
        error!(
            ?error,
            "failed to copy scene diagnostics text to the clipboard"
        );
    }
    with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
    Ok(true)
}

// timeline[impl viewport.mouse-pan]
fn handle_scene_right_button_down(hwnd: WindowHandle, lparam: LPARAM) -> eyre::Result<bool> {
    let point = ClientPoint::from_lparam(lparam);

    let should_begin_pan = with_scene_app_state(|state| {
        state.pointer_position = Some(point);
        state.chrome_tooltip.hide(hwnd);
        let layout = scene_client_layout(hwnd, state)?;
        if state.scene_kind == SceneWindowKind::TimelinePlayground {
            if !timeline_playground_pan_interaction_at_point(state, layout, point) {
                trace!(
                    ?point,
                    "timeline playground right-button pan ignored outside pan surface"
                );
                return Ok(false);
            }
            let Some(playground) = state.timeline_playground.as_mut() else {
                return Ok(false);
            };
            playground.cancel_zoom_animation_at_current_range();
            state.timeline_playground_pan_drag = Some(TimelinePlaygroundPanDrag {
                origin: point,
                origin_visible_start_ns: playground.visible_start_ns,
                origin_visible_end_ns: playground.visible_end_ns,
                origin_vertical_scroll_offset: playground.vertical_scroll_offset,
            });
            playground.hovered_item = None;
            trace!(?point, "timeline playground right-button pan started");
            return Ok(true);
        }

        if !timeline_pan_interaction_at_point(state, layout, point) {
            trace!(
                ?point,
                "timeline right-button pan ignored outside timeline pan surface"
            );
            return Ok(false);
        }

        if state.timeline_document.is_none() {
            return Ok(false);
        }
        let origin_viewport = cancel_timeline_zoom_animation_at_current_viewport(state);

        state.pending_timeline_selection = None;
        state.timeline_pan_drag = Some(TimelinePanDrag {
            origin: point,
            origin_viewport,
            origin_vertical_scroll_offset: timeline_current_vertical_scroll_offset(state, layout),
        });
        trace!(?point, "timeline right-button pan started");
        Ok(true)
    })?;

    if !should_begin_pan {
        return Ok(false);
    }

    hwnd.capture_mouse();
    with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
    Ok(true)
}

// timeline[impl viewport.mouse-zoom-anchor]
// timeline[impl viewport.mouse-zoom-animation]
#[expect(
    clippy::too_many_lines,
    reason = "scene mouse-wheel dispatch keeps logs, playground, and timeline zoom routing together"
)]
fn handle_scene_mouse_wheel(
    hwnd: WindowHandle,
    wparam: WPARAM,
    lparam: LPARAM,
) -> eyre::Result<bool> {
    let point = screen_to_client_point(hwnd, lparam)?;

    with_scene_app_state(|state| {
        let layout = scene_client_layout(hwnd, state)?;
        if state.scene_kind == SceneWindowKind::Logs && !state.diagnostics_visible {
            let wheel_delta = high_word_i16(wparam.0);
            scroll_logs_by_wheel(state, layout, wheel_delta);
            render_scene_window_frame(state, hwnd, None, false)?;
            return Ok(true);
        }

        if state.scene_kind == SceneWindowKind::TimelinePlayground && !state.diagnostics_visible {
            let vertical_scroll_offset = state
                .timeline_playground
                .as_ref()
                .map_or(0, |playground| playground.vertical_scroll_offset);
            let playground_layout = windows_scene::timeline_playground_layout(
                layout.terminal_panel_rect().inset(24),
                vertical_scroll_offset,
            );
            if !playground_layout.content_rect.contains(point) {
                return Ok(false);
            }
            let wheel_delta = high_word_i16(wparam.0);
            if wheel_delta == 0 {
                return Ok(true);
            }
            if let Some(playground) = state.timeline_playground.as_mut() {
                let anchor_x_pixels = point
                    .to_win32_point()
                    .map_or(0, |point| point.x - playground_layout.content_rect.left());
                // timeline[impl playground.viewport-controls]
                // timeline[impl playground.mouse-zoom-anchor]
                if wheel_delta > 0 {
                    playground.zoom_about_pixels(
                        anchor_x_pixels,
                        playground_layout.content_rect.width(),
                        1,
                        2,
                    );
                } else {
                    playground.zoom_about_pixels(
                        anchor_x_pixels,
                        playground_layout.content_rect.width(),
                        2,
                        1,
                    );
                }
            }
            render_scene_window_frame(state, hwnd, None, false)?;
            return Ok(true);
        }

        if !timeline_selection_surface_at_point(state, layout, point) {
            return Ok(false);
        }

        let wheel_delta = high_word_i16(wparam.0);
        if wheel_delta == 0 {
            return Ok(true);
        }

        let current_viewport = current_timeline_zoom_viewport(state);
        let base_viewport = state
            .timeline_zoom_animation
            .map_or(current_viewport, |animation| animation.target_viewport);
        if state.timeline_document.is_none() {
            return Ok(false);
        }

        let steps = if wheel_delta.abs() < MOUSE_WHEEL_DELTA {
            i32::from(wheel_delta.signum())
        } else {
            i32::from(wheel_delta / MOUSE_WHEEL_DELTA)
        };
        let factor = if steps > 0 {
            0.5_f64.powi(steps)
        } else {
            2.0_f64.powi(-steps)
        };
        let anchor = windows_scene::timeline_viewport_point_from_client_point(
            timeline_layout(layout),
            point,
        );
        let target_viewport = base_viewport.scaled_about(anchor, factor);
        let current_vertical_scroll_offset = timeline_current_vertical_scroll_offset(state, layout);
        if let Some(pan_drag) = state.timeline_pan_drag.as_mut() {
            trace!(
                wheel_delta,
                steps,
                ?point,
                "timeline wheel zoom while right-button pan is active"
            );
            state.timeline_zoom_animation = None;
            rebase_timeline_pan_drag_after_zoom(
                pan_drag,
                point,
                target_viewport,
                current_vertical_scroll_offset,
            );
            if let Some(document) = timeline_document_handle_mut(state) {
                document.set_viewport(target_viewport);
            }
        } else {
            trace!(wheel_delta, steps, ?point, "timeline wheel zoom");
            if let Some(document) = timeline_document_handle_mut(state) {
                document.set_viewport(current_viewport);
            }
            state.timeline_zoom_animation = Some(TimelineZoomAnimation {
                start_viewport: current_viewport,
                target_viewport,
                started_at: Instant::now(),
            });
        }
        render_scene_window_frame(state, hwnd, None, false)?;
        Ok(true)
    })
}

#[cfg(test)]
fn scene_mouse_down_action(action_at_point: Option<SceneAction>) -> ScenePointerAction {
    match action_at_point {
        Some(action) => ScenePointerAction::Invoke(action),
        None => ScenePointerAction::NotHandled,
    }
}

#[cfg(test)]
fn window_chrome_mouse_down_action(
    button_at_point: Option<WindowChromeButton>,
) -> WindowChromePointerAction {
    match button_at_point {
        Some(WindowChromeButton::Pin) => WindowChromePointerAction::RenderOnly,
        Some(WindowChromeButton::Diagnostics) => WindowChromePointerAction::RenderOnly,
        Some(button) => WindowChromePointerAction::Execute(button),
        None => WindowChromePointerAction::NotHandled,
    }
}

fn handle_scene_set_cursor(hwnd: WindowHandle, lparam: LPARAM) -> eyre::Result<bool> {
    let hit_test_code = u32::from(low_word_u16(lparam.0));
    if hit_test_code != HTCAPTION && hit_test_code != HTCLIENT {
        return Ok(false);
    }

    let point = cursor_client_point(hwnd)?;
    let cursor = with_scene_app_state(|state| {
        let layout = scene_client_layout(hwnd, state)?;
        Ok(scene_cursor_for_point(state, layout, point))
    })?;

    if let Some(cursor) = cursor {
        set_system_cursor(cursor);
        return Ok(true);
    }

    Ok(false)
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
        let layout = terminal_client_layout(hwnd, state)?;
        if state.in_move_size_loop
            && should_defer_terminal_resize_during_move_size(state.terminal_layout, layout)
        {
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

fn handle_dpi_changed(hwnd: WindowHandle, lparam: LPARAM) -> LRESULT {
    let result = with_app_state(|state| apply_app_dpi(state, window_dpi(hwnd)))
        .and_then(|()| apply_suggested_dpi_rect(hwnd, lparam));

    match result {
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
            state.chrome_tooltip.hide(hwnd);
            render_current_frame_with_options(state, hwnd, None, true)?;
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

    if control_key_is_down() && matches!(code_unit, 43 | 45 | 61 | 95) {
        return LRESULT(0);
    }

    match with_app_state(|state| {
        let consumed = state.terminal.handle_char(code_unit, lparam.0)?;
        if consumed && state.terminal.take_repaint_requested() {
            render_current_frame(state, hwnd, None)?;
        }
        Ok(consumed)
    }) {
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

    if let Some(action) = current_window_shortcut_action(virtual_key) {
        match execute_window_shortcut(hwnd, action) {
            Ok(true) => return LRESULT(0),
            Ok(false) => {}
            Err(error) => return fail_and_close(hwnd, &error),
        }
    }

    match with_app_state(|state| {
        let consumed = state.terminal.handle_key_event(
            virtual_key,
            lparam.0,
            was_down,
            false,
            keyboard_mods(virtual_key, lparam.0, false),
        )?;
        if consumed && state.terminal.take_repaint_requested() {
            render_current_frame(state, hwnd, None)?;
        }
        Ok(consumed)
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
        let consumed = state.terminal.handle_key_event(
            virtual_key,
            lparam.0,
            false,
            true,
            keyboard_mods(virtual_key, lparam.0, true),
        )?;
        if consumed && state.terminal.take_repaint_requested() {
            render_current_frame(state, hwnd, None)?;
        }
        Ok(consumed)
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
        let mut state = state.borrow_mut();
        if let Some(app_state) = state.as_mut()
            && let Err(error) = app_state.taskbar_progress.clear(hwnd)
        {
            error!(
                ?error,
                "failed to clear taskbar progress during window shutdown"
            );
        }
        let _ = state.take();
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
    sync_window_chrome(state, hwnd)?;

    if let Some((width, height)) = resize
        && let Some(renderer) = state.renderer.as_mut()
    {
        renderer.resize(width, height)?;
    }

    let layout = {
        #[cfg(feature = "tracy")]
        let _span = debug_span!("compute_client_layout").entered();
        terminal_client_layout(hwnd, state)?
    };
    let window_chrome_buttons_state = terminal_window_chrome_buttons_state(state, hwnd, layout);
    let diagnostic_text = {
        #[cfg(feature = "tracy")]
        let _span = debug_span!("build_diagnostic_panel_text").entered();
        build_diagnostic_panel_text(state, layout)?
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
            title: resolved_visible_title(state.launch_title.as_deref(), &state.terminal_chrome)
                .map(ToOwned::to_owned),
            diagnostic_text,
            diagnostic_selection: state
                .diagnostic_panel_visible
                .then_some(state.diagnostic_selection)
                .flatten(),
            window_chrome_buttons_state,
            diagnostic_cell_width: state.diagnostic_cell_width,
            diagnostic_cell_height: state.diagnostic_cell_height,
            scene: None,
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

fn terminal_window_chrome_buttons_state(
    state: &AppState,
    hwnd: WindowHandle,
    layout: TerminalLayout,
) -> WindowChromeButtonsState {
    WindowChromeButtonsState {
        pin: window_chrome_button_visual_state(
            layout.pin_button_rect(),
            state.pointer_position,
            state.pressed_chrome_button == Some(WindowChromeButton::Pin),
            state.pin_button_last_clicked_at,
            state.pinned_topmost,
        ),
        diagnostics: window_chrome_button_visual_state(
            layout.diagnostics_button_rect(),
            state.pointer_position,
            state.pressed_chrome_button == Some(WindowChromeButton::Diagnostics),
            state.diagnostics_button_last_clicked_at,
            state.diagnostic_panel_visible,
        ),
        minimize: window_chrome_button_visual_state(
            layout.minimize_button_rect(),
            state.pointer_position,
            state.pressed_chrome_button == Some(WindowChromeButton::Minimize),
            None,
            false,
        ),
        maximize_restore: window_chrome_button_visual_state(
            layout.maximize_restore_button_rect(),
            state.pointer_position,
            state.pressed_chrome_button == Some(WindowChromeButton::MaximizeRestore),
            None,
            hwnd.is_zoomed(),
        ),
        close: window_chrome_button_visual_state(
            layout.close_button_rect(),
            state.pointer_position,
            state.pressed_chrome_button == Some(WindowChromeButton::Close),
            None,
            false,
        ),
        pinned: state.pinned_topmost,
        maximized: hwnd.is_zoomed(),
        focused: state.window_focused,
    }
}

fn scene_window_chrome_buttons_state(
    state: &SceneAppState,
    hwnd: WindowHandle,
    layout: TerminalLayout,
) -> WindowChromeButtonsState {
    WindowChromeButtonsState {
        pin: window_chrome_button_visual_state(
            layout.pin_button_rect(),
            state.pointer_position,
            state.pressed_target == Some(ScenePressedTarget::ChromeButton(WindowChromeButton::Pin)),
            state.pin_button_last_clicked_at,
            state.pinned_topmost,
        ),
        diagnostics: window_chrome_button_visual_state(
            layout.diagnostics_button_rect(),
            state.pointer_position,
            state.pressed_target
                == Some(ScenePressedTarget::ChromeButton(
                    WindowChromeButton::Diagnostics,
                )),
            state.diagnostics_button_last_clicked_at,
            state.diagnostics_visible,
        ),
        minimize: window_chrome_button_visual_state(
            layout.minimize_button_rect(),
            state.pointer_position,
            state.pressed_target
                == Some(ScenePressedTarget::ChromeButton(
                    WindowChromeButton::Minimize,
                )),
            None,
            false,
        ),
        maximize_restore: window_chrome_button_visual_state(
            layout.maximize_restore_button_rect(),
            state.pointer_position,
            state.pressed_target
                == Some(ScenePressedTarget::ChromeButton(
                    WindowChromeButton::MaximizeRestore,
                )),
            None,
            hwnd.is_zoomed(),
        ),
        close: window_chrome_button_visual_state(
            layout.close_button_rect(),
            state.pointer_position,
            state.pressed_target
                == Some(ScenePressedTarget::ChromeButton(WindowChromeButton::Close)),
            None,
            false,
        ),
        pinned: state.pinned_topmost,
        maximized: hwnd.is_zoomed(),
        focused: state.window_focused,
    }
}

fn window_chrome_button_visual_state(
    rect: ClientRect,
    pointer_position: Option<ClientPoint>,
    pressed: bool,
    last_clicked_at: Option<Instant>,
    active: bool,
) -> ButtonVisualState {
    windows_scene::compute_button_visual_state(
        rect,
        pointer_position,
        pressed,
        last_clicked_at,
        active,
        Instant::now(),
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "scene window rendering dispatch is intentionally centralized across scene kinds"
)]
fn render_scene_window_frame(
    state: &mut SceneAppState,
    hwnd: WindowHandle,
    resize: Option<(u32, u32)>,
    force_redraw: bool,
) -> eyre::Result<()> {
    sync_demo_mode_state(state);
    apply_timeline_zoom_animation(state);
    apply_timeline_playground_zoom_animation(state);
    sync_timeline_audio_runtime_from_document(state);
    let timeline_transcription_completion_target = (state.scene_kind == SceneWindowKind::Timeline)
        .then(|| state.hwnd.map(|hwnd| hwnd.raw().0 as isize))
        .flatten();
    if let Some(device_window) = state.audio_input_device_window.as_mut() {
        if let Some(target) = timeline_transcription_completion_target {
            device_window.set_transcription_completion_notification_target(
                target,
                TIMELINE_TRANSCRIPTION_WORKER_COMPLETED_MESSAGE,
            );
        }
        // timeline[impl recording.append-live]
        device_window.sync_transport();
    }
    sync_timeline_transcription_text_blocks(state);
    if super::has_job_snapshots() && super::jobs::mark_jobs_window_auto_opened() {
        open_jobs_window_from_scene_state(state);
    }

    if let Some((width, height)) = resize
        && let Some(renderer) = state.renderer.as_mut()
    {
        renderer.resize(width, height)?;
    }

    let layout = scene_client_layout(hwnd, state)?;
    sync_scene_window_title(state, hwnd)?;
    if state.scene_kind == SceneWindowKind::Logs && !state.diagnostics_visible {
        sync_logs_scroll_offset(state, layout);
    }
    let window_chrome_buttons_state = scene_window_chrome_buttons_state(state, hwnd, layout);
    let scramble_input_device_identifiers = state
        .demo_mode_scramble_input_device_identifiers
        .is_enabled();
    let timeline_document = timeline_document_snapshot(state);
    let scene = if state.diagnostics_visible && state.scene_kind == SceneWindowKind::Launcher {
        windows_scene::build_launcher_diagnostic_render_scene(
            layout,
            window_chrome_buttons_state,
            state.scene_action_selected_index,
            state.scene_virtual_cursor,
            state.diagnostic_selection,
            state.diagnostic_cell_width,
            state.diagnostic_cell_height,
        )
    } else if state.diagnostics_visible
        && state.scene_kind == SceneWindowKind::AudioInputDevicePicker
    {
        windows_scene::build_audio_input_device_diagnostic_render_scene(
            layout,
            window_chrome_buttons_state,
            &state.audio_input_picker.devices,
            state.audio_input_picker.selected_index,
            state.diagnostic_selection,
            state.diagnostic_cell_width,
            state.diagnostic_cell_height,
            scramble_input_device_identifiers,
        )
    } else if state.diagnostics_visible
        && state.scene_kind == SceneWindowKind::AudioInputDeviceDetails
    {
        windows_scene::build_audio_input_device_detail_diagnostic_render_scene(
            layout,
            window_chrome_buttons_state,
            state.audio_input_device_window.as_ref(),
            state.diagnostic_selection,
            state.diagnostic_cell_width,
            state.diagnostic_cell_height,
            scramble_input_device_identifiers,
        )
    } else if state.diagnostics_visible && state.scene_kind == SceneWindowKind::AudioDaemon {
        let daemon_status = super::audio_transcription_daemon_status(&crate::paths::CACHE_DIR);
        windows_scene::build_audio_daemon_diagnostic_render_scene(
            layout,
            window_chrome_buttons_state,
            &daemon_status,
            state.diagnostic_selection,
            state.diagnostic_cell_width,
            state.diagnostic_cell_height,
        )
    } else if state.diagnostics_visible && state.scene_kind == SceneWindowKind::Logs {
        windows_scene::build_scene_diagnostic_render_scene(
            layout,
            state.scene_kind,
            window_chrome_buttons_state,
            &logs_diagnostic_text(),
            state.diagnostic_selection,
            state.diagnostic_cell_width,
            state.diagnostic_cell_height,
        )
    } else if state.diagnostics_visible
        && state.scene_kind == SceneWindowKind::TimelinePlaygroundDetail
    {
        let detail_state = timeline_playground_detail_window_state(state);
        let pretty_text = timeline_playground_detail_pretty_text(&detail_state);
        windows_scene::build_timeline_playground_detail_diagnostic_render_scene(
            layout,
            window_chrome_buttons_state,
            detail_state.detail.as_ref(),
            &pretty_text,
            detail_state.pinned,
            state.diagnostic_selection,
            state.diagnostic_cell_width,
            state.diagnostic_cell_height,
        )
    } else if state.diagnostics_visible {
        windows_scene::build_scene_diagnostic_render_scene(
            layout,
            state.scene_kind,
            window_chrome_buttons_state,
            &build_scene_diagnostic_text(state),
            state.diagnostic_selection,
            state.diagnostic_cell_width,
            state.diagnostic_cell_height,
        )
    } else if state.scene_kind == SceneWindowKind::AudioInputDevicePicker {
        windows_scene::build_audio_input_device_picker_render_scene(
            layout,
            window_chrome_buttons_state,
            &state.audio_input_picker.devices,
            state.audio_input_picker.selected_index,
            scramble_input_device_identifiers,
        )
    } else if state.scene_kind == SceneWindowKind::AudioInputDeviceDetails {
        windows_scene::build_audio_input_device_detail_render_scene(
            layout,
            window_chrome_buttons_state,
            state.audio_input_device_window.as_ref(),
            audio_input_device_detail_visual_state(state, layout),
            scramble_input_device_identifiers,
            state.diagnostic_selection,
        )
    } else if state.scene_kind == SceneWindowKind::AudioDaemon {
        let daemon_status = super::audio_transcription_daemon_status(&crate::paths::CACHE_DIR);
        windows_scene::build_audio_daemon_render_scene(
            layout,
            window_chrome_buttons_state,
            &daemon_status,
            audio_daemon_visual_state(state, layout),
        )
    } else if state.scene_kind == SceneWindowKind::Jobs {
        let jobs = super::job_snapshots();
        windows_scene::build_jobs_render_scene(layout, window_chrome_buttons_state, &jobs)
    } else if state.scene_kind == SceneWindowKind::Logs {
        windows_scene::build_logs_render_scene(
            layout,
            window_chrome_buttons_state,
            &logs_window_text(state, layout),
            &logs_window_rows(state, layout),
            logs_window_visual_state(state, layout),
            state.diagnostic_selection,
            state.diagnostic_cell_width,
            state.diagnostic_cell_height,
        )
    } else if state.scene_kind == SceneWindowKind::ModelWarning {
        let progress = (state.scene_opened_at.elapsed().as_secs_f32() / 0.28).clamp(0.0, 1.0);
        let prepare_hold_progress = state
            .model_warning_prepare_started_at
            .map_or(0.0, |started| {
                (started.elapsed().as_secs_f32()
                    / MODEL_WARNING_PREPARE_HOLD_DURATION.as_secs_f32())
                .clamp(0.0, 1.0)
            });
        windows_scene::build_model_warning_render_scene(
            layout,
            window_chrome_buttons_state,
            state.model_warning.as_ref(),
            progress,
            prepare_hold_progress,
        )
    } else if state.scene_kind == SceneWindowKind::CursorGallery {
        windows_scene::build_cursor_gallery_render_scene(
            layout,
            window_chrome_buttons_state,
            state.scene_action_selected_index,
            state.scene_virtual_cursor,
            state.pointer_position,
        )
    } else if state.scene_kind == SceneWindowKind::DemoMode {
        windows_scene::build_demo_mode_render_scene(
            layout,
            window_chrome_buttons_state,
            scramble_input_device_identifiers,
            demo_mode_visual_state(state, layout),
        )
    } else if state.scene_kind == SceneWindowKind::TimelinePlayground {
        let pointer_position = state.pointer_position;
        let button_visual_states = scene_button_visual_states(state, layout);
        let playground = state
            .timeline_playground
            .as_mut()
            .expect("timeline playground scene has playground state");
        playground.sync_live_tracing_events();
        let playground_layout = windows_scene::timeline_playground_layout(
            layout.terminal_panel_rect().inset(24),
            playground.vertical_scroll_offset,
        );
        let query = playground
            .query(u32::try_from(playground_layout.content_rect.width().max(1)).unwrap_or(1))
            .expect("timeline playground query must be valid");
        let render_plan = playground.dataset.render_plan(&query);
        playground.clamp_vertical_scroll_offset(playground_layout, render_plan.rows().len());
        playground.update_row_position_animation(&render_plan);
        let row_visual_positions = playground.row_visual_positions(&render_plan);
        playground.apply_row_position_animation();
        windows_scene::build_timeline_playground_render_scene(
            layout,
            window_chrome_buttons_state,
            &playground.dataset,
            &render_plan,
            playground.view_state(pointer_position),
            &row_visual_positions,
            &button_visual_states,
        )
    } else if state.scene_kind == SceneWindowKind::TimelinePlaygroundDetail {
        let detail_state = timeline_playground_detail_window_state(state);
        let pretty_text = timeline_playground_detail_pretty_text(&detail_state);
        windows_scene::build_timeline_playground_detail_render_scene(
            layout,
            window_chrome_buttons_state,
            &pretty_text,
            detail_state.pinned,
            state.diagnostic_selection,
        )
    } else if state.scene_kind == SceneWindowKind::TimelineTranscriptionSettings {
        windows_scene::build_timeline_transcription_settings_render_scene(
            layout,
            window_chrome_buttons_state,
            timeline_document.as_ref(),
            timeline_transcription_settings_view_state(state, layout),
        )
    } else if state.scene_kind == SceneWindowKind::Timeline {
        windows_scene::build_blank_timeline_render_scene(
            layout,
            window_chrome_buttons_state,
            timeline_document.as_ref(),
            state.audio_input_device_window.as_ref(),
            timeline_document_visual_state(state, layout),
        )
    } else {
        windows_scene::build_scene_render_scene(
            layout,
            state.scene_kind,
            window_chrome_buttons_state,
            scaled_scene_button_size(state.dpi),
            &scene_button_visual_states(state, layout),
            state.scene_virtual_cursor,
        )
    };

    let Some(renderer) = state.renderer.as_mut() else {
        return Ok(());
    };

    let frame = RenderFrameModel {
        layout,
        title: Some(state.scene_kind.title().to_owned()),
        diagnostic_text: String::new(),
        diagnostic_selection: None,
        window_chrome_buttons_state,
        diagnostic_cell_width: state.diagnostic_cell_width,
        diagnostic_cell_height: state.diagnostic_cell_height,
        scene: Some(scene),
        terminal_cell_width: state.terminal_cell_width,
        terminal_cell_height: state.terminal_cell_height,
        terminal_display: Arc::new(TerminalDisplayState::default()),
        terminal_visual_state: RendererTerminalVisualState::default(),
    };

    if force_redraw {
        renderer.render_frame_model_force_redraw(frame)?;
    } else {
        renderer.render_frame_model(frame)?;
    }

    Ok(())
}

fn sync_timeline_audio_runtime_from_document(state: &mut SceneAppState) {
    if state.scene_kind != SceneWindowKind::Timeline || state.audio_input_device_window.is_some() {
        return;
    }
    let Some(document) = timeline_document_handle(state) else {
        return;
    };
    let microphone = document.tracks().iter().find_map(|track| {
        let crate::timeline::TimelineTrackProjection::Audio(projection) = track.projection() else {
            return None;
        };
        let device_id = projection.source_device_id()?;
        Some((device_id.to_owned(), projection.source_label().to_owned()))
    });
    let Some((device_id, fallback_name)) = microphone else {
        return;
    };
    let device = list_active_audio_input_devices()
        .unwrap_or_default()
        .into_iter()
        .find(|device| device.id == device_id)
        .unwrap_or(AudioInputDeviceSummary {
            id: device_id,
            name: fallback_name,
            is_default: false,
            state: "unknown".to_owned(),
            icon: String::new(),
            sample_rate_hz: None,
        });
    info!(device = %device.name, "attached timeline microphone runtime from audio track");
    state.audio_input_device_window = Some(AudioInputDeviceWindowState::new(device));
}

fn sync_timeline_transcription_text_blocks(state: &mut SceneAppState) {
    let Some(document) = timeline_document_handle(state) else {
        return;
    };
    let target_text_track_id = document.tracks().iter().find_map(|track| {
        let crate::timeline::TimelineTrackProjection::Transcription(projection) =
            track.projection()
        else {
            return None;
        };
        projection.target_text_track_id()
    });
    let Some(target_text_track_id) = target_text_track_id else {
        if state
            .audio_input_device_window
            .as_ref()
            .is_some_and(|device_window| {
                !device_window.runtime.transcription.staged_text.is_empty()
            })
        {
            info!("staged transcription text is waiting for an output text track target");
        }
        return;
    };
    let Some((text, start_seconds, end_seconds)) = state
        .audio_input_device_window
        .as_mut()
        .and_then(|device_window| {
            let text = device_window
                .runtime
                .transcription
                .take_staged_transcript_text()?;
            let end_seconds = device_window.runtime.transcription_head_seconds;
            let start_seconds = (end_seconds - device_window.runtime.transcription.chunk_seconds)
                .max(0.0)
                .min(end_seconds);
            Some((text, start_seconds, end_seconds))
        })
    else {
        return;
    };
    let Some(document) = timeline_document_handle_mut(state) else {
        return;
    };
    let appended = document.append_text_block(
        target_text_track_id,
        TimelineTimeRangeNs::new(
            TimelineTimeNs::from_duration(Time::new::<second>(start_seconds)),
            TimelineTimeNs::from_duration(Time::new::<second>(end_seconds)),
        ),
        text,
    );
    if appended {
        info!(
            start_seconds,
            end_seconds, "committed transcription result into timeline text track"
        );
        broadcast_timeline_document_changed();
    }
}

fn demo_mode_visual_state(
    state: &SceneAppState,
    layout: TerminalLayout,
) -> windows_scene::DemoModeVisualState {
    let demo_layout = windows_scene::demo_mode_layout(layout.terminal_panel_rect().inset(30));
    windows_scene::DemoModeVisualState {
        demo_button: windows_scene::compute_button_visual_state(
            demo_layout.demo_button_bounds,
            demo_mode_hover_point(state, demo_layout),
            state.pressed_target == Some(ScenePressedTarget::DemoModeButton),
            None,
            false,
            Instant::now(),
        ),
        scramble_toggle: windows_scene::compute_button_visual_state(
            demo_layout.scramble_toggle_bounds,
            demo_mode_hover_point(state, demo_layout),
            state.pressed_target == Some(ScenePressedTarget::DemoModeScrambleToggle),
            state.demo_mode_scramble_toggle_last_changed_at,
            state
                .demo_mode_scramble_input_device_identifiers
                .is_enabled(),
            Instant::now(),
        ),
    }
}

fn demo_mode_hover_point(
    state: &SceneAppState,
    layout: windows_scene::DemoModeLayout,
) -> Option<ClientPoint> {
    state
        .pointer_position
        .filter(|point| {
            layout.demo_button_bounds.contains(*point)
                || layout.scramble_toggle_bounds.contains(*point)
        })
        .or(state.scene_virtual_cursor)
}

fn scene_button_visual_states(
    state: &SceneAppState,
    layout: TerminalLayout,
) -> Vec<(SceneAction, ButtonVisualState)> {
    let now = Instant::now();
    let specs = windows_scene::scene_button_specs(state.scene_kind);
    let button_layouts = windows_scene::layout_scene_buttons(
        layout.terminal_panel_rect(),
        specs.len(),
        scaled_scene_button_size(state.dpi),
    );

    specs
        .iter()
        .zip(button_layouts)
        .enumerate()
        .map(|(index, (spec, button_layout))| {
            let pressed = state.pressed_target == Some(ScenePressedTarget::Action(spec.action));
            let selected = matches!(
                state.scene_kind,
                SceneWindowKind::Launcher
                    | SceneWindowKind::TimelineStart
                    | SceneWindowKind::TimelineAddTrack
            ) && index == state.scene_action_selected_index;
            let last_clicked = state
                .last_clicked_action
                .filter(|click| click.action == spec.action)
                .map(|click| click.clicked_at);
            let active = scene_action_active(spec.action)
                || selected
                || timeline_playground_action_active(state, spec.action);
            (
                spec.action,
                windows_scene::compute_button_visual_state(
                    button_layout.hit_rect(),
                    state.pointer_position,
                    pressed,
                    last_clicked,
                    active,
                    now,
                ),
            )
        })
        .collect()
}

fn timeline_playground_action_active(state: &SceneAppState, action: SceneAction) -> bool {
    let Some(playground) = state.timeline_playground.as_ref() else {
        return false;
    };
    if action == SceneAction::ToggleTimelinePlaygroundLiveEvents {
        return playground.source_mode == TimelinePlaygroundSourceMode::LiveTracingEvents;
    }
    matches!(
        (action, playground.grouping_mode),
        (
            SceneAction::TimelinePlaygroundGroupingGroupKey,
            TimelineGroupingMode::GroupKey
        ) | (
            SceneAction::TimelinePlaygroundGroupingSourceKey,
            TimelineGroupingMode::SourceKey
        ) | (
            SceneAction::TimelinePlaygroundGroupingLabel,
            TimelineGroupingMode::Label
        ) | (
            SceneAction::TimelinePlaygroundGroupingAll,
            TimelineGroupingMode::All
        )
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "timeline visual-state assembly keeps the row-control hover and pressed wiring localized to the owning timeline scene state"
)]
fn timeline_document_visual_state(
    state: &SceneAppState,
    layout: TerminalLayout,
) -> windows_scene::TimelineDocumentVisualState {
    let point = state.pointer_position;
    let timeline_layout = timeline_layout(layout);
    let live_audio_track_index = timeline_live_audio_track_index(state);
    let live_transcription_track_index = timeline_live_transcription_track_index(state);
    let vertical_scroll_offset = windows_scene::timeline_clamp_vertical_scroll_offset(
        timeline_layout,
        timeline_track_count(state),
        state.timeline_vertical_scroll_offset,
    );
    let add_track_last_clicked = state
        .last_clicked_action
        .filter(|click| click.action == SceneAction::OpenTimelineTrackMenu)
        .map(|click| click.clicked_at);

    let control_visual_state = |action| {
        let Some(rect) = windows_scene::timeline_viewport_control_rect(timeline_layout, action)
        else {
            return ButtonVisualState::default();
        };
        let last_clicked = state
            .last_clicked_action
            .filter(|click| click.action == action)
            .map(|click| click.clicked_at);
        windows_scene::compute_button_visual_state(
            rect,
            point,
            state.pressed_target == Some(ScenePressedTarget::Action(action)),
            last_clicked,
            false,
            Instant::now(),
        )
    };

    windows_scene::TimelineDocumentVisualState {
        add_track_button: windows_scene::compute_button_visual_state(
            timeline_layout.add_track_rect,
            point,
            state.pressed_target
                == Some(ScenePressedTarget::Action(
                    SceneAction::OpenTimelineTrackMenu,
                )),
            add_track_last_clicked,
            false,
            Instant::now(),
        ),
        transport_play_button: windows_scene::compute_button_visual_state(
            windows_scene::timeline_transport_play_button_rect(timeline_layout),
            point,
            state.pressed_target == Some(ScenePressedTarget::TimelineTransportPlayPause),
            None,
            false,
            Instant::now(),
        ),
        microphone_record_button: live_audio_track_index
            .and_then(|row_index| {
                windows_scene::timeline_track_record_button_rect(timeline_layout, row_index)
            })
            .map_or_else(ButtonVisualState::default, |rect| {
                windows_scene::compute_button_visual_state(
                    rect,
                    point,
                    state.pressed_target
                        == live_audio_track_index.map(ScenePressedTarget::TimelineTrackRecord),
                    None,
                    false,
                    Instant::now(),
                )
            }),
        microphone_playback_hovered: live_audio_track_index
            .and_then(|row_index| {
                windows_scene::timeline_track_playback_button_rect(timeline_layout, row_index)
            })
            .is_some_and(|rect| point.is_some_and(|point| rect.contains(point))),
        microphone_playback_pressed: state.pressed_target
            == live_audio_track_index.map(ScenePressedTarget::TimelineTrackPlayback),
        microphone_loopback_hovered: live_audio_track_index
            .and_then(|row_index| {
                windows_scene::timeline_track_loopback_button_rect(timeline_layout, row_index)
            })
            .is_some_and(|rect| point.is_some_and(|point| rect.contains(point))),
        microphone_loopback_pressed: state.pressed_target
            == live_audio_track_index.map(ScenePressedTarget::TimelineTrackLoopback),
        transcription_toggle_hovered: live_transcription_track_index
            .and_then(|row_index| {
                windows_scene::timeline_track_transcription_toggle_button_rect(
                    timeline_layout,
                    row_index,
                )
            })
            .is_some_and(|rect| point.is_some_and(|point| rect.contains(point))),
        transcription_toggle_pressed: state.pressed_target
            == live_transcription_track_index
                .map(ScenePressedTarget::TimelineTrackTranscriptionToggle),
        transcription_settings_hovered: live_transcription_track_index
            .and_then(|row_index| {
                windows_scene::timeline_track_transcription_settings_button_rect(
                    timeline_layout,
                    row_index,
                )
            })
            .is_some_and(|rect| point.is_some_and(|point| rect.contains(point))),
        transcription_settings_pressed: state.pressed_target
            == live_transcription_track_index
                .map(ScenePressedTarget::TimelineTrackTranscriptionSettings),
        pan_left_button: control_visual_state(SceneAction::PanTimelineLeft),
        pan_right_button: control_visual_state(SceneAction::PanTimelineRight),
        zoom_in_button: control_visual_state(SceneAction::ZoomTimelineIn),
        zoom_out_button: control_visual_state(SceneAction::ZoomTimelineOut),
        select_tool_button: windows_scene::compute_button_visual_state(
            windows_scene::timeline_viewport_control_rect(
                timeline_layout,
                SceneAction::SelectTimelineTool,
            )
            .unwrap_or(timeline_layout.viewport_controls_rect),
            point,
            state.pressed_target
                == Some(ScenePressedTarget::Action(SceneAction::SelectTimelineTool)),
            state
                .last_clicked_action
                .filter(|click| click.action == SceneAction::SelectTimelineTool)
                .map(|click| click.clicked_at),
            state.timeline_tool == TimelineInteractionTool::Select,
            Instant::now(),
        ),
        brush_tool_button: windows_scene::compute_button_visual_state(
            windows_scene::timeline_viewport_control_rect(
                timeline_layout,
                SceneAction::SelectTimelineBrush,
            )
            .unwrap_or(timeline_layout.viewport_controls_rect),
            point,
            state.pressed_target
                == Some(ScenePressedTarget::Action(SceneAction::SelectTimelineBrush)),
            state
                .last_clicked_action
                .filter(|click| click.action == SceneAction::SelectTimelineBrush)
                .map(|click| click.clicked_at),
            state.timeline_tool == TimelineInteractionTool::Brush,
            Instant::now(),
        ),
        hovered_head: point.and_then(|point| timeline_audio_head_at_point(state, layout, point)),
        grabbed_head: match state.pressed_target {
            Some(ScenePressedTarget::TimelineAudioHead(kind)) => Some(kind),
            _ => None,
        },
        vertical_scroll_offset,
        selection: state.timeline_selection,
    }
}

fn audio_input_device_detail_visual_state(
    state: &SceneAppState,
    layout: TerminalLayout,
) -> windows_scene::AudioInputDeviceDetailVisualState {
    let Some(point) = state.pointer_position else {
        return windows_scene::AudioInputDeviceDetailVisualState::default();
    };
    windows_scene::AudioInputDeviceDetailVisualState {
        transcription_hovered: audio_input_device_transcription_button_at_point(
            state, layout, point,
        ),
        transcription_pressed: state.pressed_target
            == Some(ScenePressedTarget::AudioInputTranscription),
        transcription_flush_hovered: audio_input_device_transcription_flush_button_at_point(
            state, layout, point,
        ),
        transcription_flush_pressed: state.pressed_target
            == Some(ScenePressedTarget::AudioInputTranscriptionFlush),
        playback_hovered: audio_input_device_play_pause_button_at_point(state, layout, point),
        playback_pressed: state.pressed_target == Some(ScenePressedTarget::AudioInputPlayback),
        loopback_hovered: audio_input_device_loopback_button_at_point(state, layout, point),
        loopback_pressed: state.pressed_target
            == Some(ScenePressedTarget::AudioInputDeviceLoopback),
        hovered_head: audio_input_timeline_head_at_point(state, layout, point),
        grabbed_head: match state.pressed_target {
            Some(ScenePressedTarget::AudioInputTimelineHead(head)) => Some(head),
            _ => None,
        },
    }
}

fn audio_daemon_visual_state(
    state: &SceneAppState,
    layout: TerminalLayout,
) -> windows_scene::AudioDaemonVisualState {
    let Some(point) = state.pointer_position else {
        return windows_scene::AudioDaemonVisualState::default();
    };
    windows_scene::AudioDaemonVisualState {
        cuda_check_hovered: audio_daemon_cuda_check_button_at_point(state, layout, point),
        cuda_check_pressed: state.pressed_target == Some(ScenePressedTarget::AudioDaemonCudaCheck),
        hovered_model_index: audio_daemon_model_at_point(state, layout, point),
        pressed_model_index: match state.pressed_target {
            Some(ScenePressedTarget::AudioDaemonModel(index)) => Some(index),
            _ => None,
        },
    }
}

fn scene_selected_text_for_copy(state: &SceneAppState, layout: TerminalLayout) -> Option<String> {
    let selection = state.diagnostic_selection?;

    // observability[impl logs.copy]
    if state.diagnostics_visible {
        return Some(cell_grid::extract_selected_text(
            scene_diagnostic_text_rect(layout),
            &build_scene_diagnostic_text(state),
            state.diagnostic_cell_width,
            state.diagnostic_cell_height,
            selection,
        ));
    }

    let target = scene_pretty_text_target(state, layout)?;

    Some(cell_grid::extract_selected_text(
        target.rect,
        &target.text,
        target.cell_width,
        target.cell_height,
        selection,
    ))
}

fn logs_window_text(state: &SceneAppState, layout: TerminalLayout) -> String {
    let snapshots = logs::log_snapshots();
    let visible_rows =
        windows_scene::logs_visible_data_row_count(layout, state.diagnostic_cell_height);
    let start = logs_effective_scroll_offset(state, snapshots.len(), visible_rows);
    let end = start.saturating_add(visible_rows).min(snapshots.len());
    log_table_text_for_records(&snapshots[start..end])
}

fn logs_diagnostic_text() -> String {
    log_table_text_for_records(&logs::log_snapshots())
}

fn logs_window_rows(
    state: &SceneAppState,
    layout: TerminalLayout,
) -> Vec<windows_scene::LogRowView> {
    let snapshots = logs::log_snapshots();
    let visible_rows =
        windows_scene::logs_visible_data_row_count(layout, state.diagnostic_cell_height);
    let start = logs_effective_scroll_offset(state, snapshots.len(), visible_rows);
    let end = start.saturating_add(visible_rows).min(snapshots.len());
    snapshots[start..end].iter().map(log_row_view).collect()
}

fn log_row_view(record: &LogRecordSnapshot) -> windows_scene::LogRowView {
    windows_scene::LogRowView {
        time: record.time_text(),
        level: record.level,
        target: fit_log_column(&record.target, 34),
        message: log_single_line(&record.message),
    }
}

fn log_table_text_for_records(records: &[LogRecordSnapshot]) -> String {
    let mut lines = vec![format!(
        "{:<12} {:<5} {:<34} {}",
        "TIME", "LEVEL", "TARGET", "MESSAGE"
    )];
    if records.is_empty() {
        lines.push("no logs captured".to_owned());
    } else {
        lines.extend(records.iter().map(|record| {
            format!(
                "{:<12} {:<5} {:<34} {}",
                record.time_text(),
                record.level.label(),
                fit_log_column(&record.target, 34),
                log_single_line(&record.message)
            )
        }));
    }
    lines.join("\n")
}

fn log_single_line(text: &str) -> String {
    text.chars()
        .map(|character| match character {
            '\r' | '\n' | '\t' => ' ',
            character => character,
        })
        .collect()
}

fn fit_log_column(text: &str, width: usize) -> String {
    let text = log_single_line(text);
    if text.chars().count() <= width {
        return text;
    }
    let mut fitted = text
        .chars()
        .take(width.saturating_sub(1))
        .collect::<String>();
    fitted.push('~');
    fitted
}

fn logs_effective_scroll_offset(
    state: &SceneAppState,
    record_count: usize,
    visible_rows: usize,
) -> usize {
    let max_offset = logs_max_scroll_offset(record_count, visible_rows);
    if state.logs_follow_tail {
        max_offset
    } else {
        state.logs_scroll_offset.min(max_offset)
    }
}

fn logs_max_scroll_offset(record_count: usize, visible_rows: usize) -> usize {
    record_count.saturating_sub(visible_rows.max(1))
}

fn sync_logs_scroll_offset(state: &mut SceneAppState, layout: TerminalLayout) {
    let visible_rows =
        windows_scene::logs_visible_data_row_count(layout, state.diagnostic_cell_height);
    let record_count = logs::log_snapshots().len();
    state.logs_scroll_offset = logs_effective_scroll_offset(state, record_count, visible_rows);
}

fn scroll_logs_by_wheel(state: &mut SceneAppState, layout: TerminalLayout, wheel_delta: i16) {
    if wheel_delta == 0 {
        return;
    }
    let visible_rows =
        windows_scene::logs_visible_data_row_count(layout, state.diagnostic_cell_height);
    let record_count = logs::log_snapshots().len();
    let max_offset = logs_max_scroll_offset(record_count, visible_rows);
    let steps = if wheel_delta.unsigned_abs() < MOUSE_WHEEL_DELTA.unsigned_abs() {
        isize::from(wheel_delta.signum())
    } else {
        isize::from(wheel_delta / MOUSE_WHEEL_DELTA)
    };
    let base = logs_effective_scroll_offset(state, record_count, visible_rows);
    let next = base.saturating_add_signed(-steps * 3).min(max_offset);
    state.logs_scroll_offset = next;
    state.logs_follow_tail = next == max_offset;
}

fn logs_window_visual_state(
    state: &SceneAppState,
    layout: TerminalLayout,
) -> windows_scene::LogsWindowVisualState {
    let now = Instant::now();
    let button_state = |control| {
        windows_scene::compute_button_visual_state(
            windows_scene::logs_control_rect(layout, control),
            state.pointer_position,
            state.pressed_target == Some(ScenePressedTarget::LogsControl(control)),
            None,
            false,
            now,
        )
    };
    windows_scene::LogsWindowVisualState {
        to_bottom: button_state(windows_scene::LogsWindowControl::ToBottom),
        clear: button_state(windows_scene::LogsWindowControl::Clear),
        settings: button_state(windows_scene::LogsWindowControl::Settings),
    }
}

fn apply_logs_control(
    state: &mut SceneAppState,
    hwnd: WindowHandle,
    layout: TerminalLayout,
    control: windows_scene::LogsWindowControl,
) {
    match control {
        windows_scene::LogsWindowControl::ToBottom => {
            state.logs_follow_tail = true;
            sync_logs_scroll_offset(state, layout);
        }
        windows_scene::LogsWindowControl::Clear => {
            logs::clear_logs();
            state.logs_scroll_offset = 0;
            state.logs_follow_tail = true;
            state.diagnostic_selection = None;
        }
        windows_scene::LogsWindowControl::Settings => {
            info!(
                source_hwnd = hwnd.raw().0 as isize,
                "Log settings are not implemented yet"
            );
        }
    }
}

fn build_scene_diagnostic_text(state: &SceneAppState) -> String {
    let scramble_input_device_identifiers = state
        .demo_mode_scramble_input_device_identifiers
        .is_enabled();
    let mut lines = vec![
        format!("window\t{}", state.scene_kind.title()),
        format!("bell-source\t{}", current_bell_source_label()),
    ];

    if let BellSource::File(path) = current_bell_source() {
        lines.push(format!("bell-file\t{}", path.display()));
    }

    if state.scene_kind == SceneWindowKind::AudioInputDevicePicker {
        lines.push(format!(
            "audio-input-selected-index\t{}",
            state.audio_input_picker.selected_index
        ));
        lines.push("audio-input-devices".to_owned());
        for (index, device) in state.audio_input_picker.devices.iter().enumerate() {
            let status = if index == state.audio_input_picker.selected_index {
                "selected"
            } else {
                "available"
            };
            lines.push(format!(
                "- {}\t{}\t{}",
                device.name,
                status,
                windows_scene::input_device_identifier_display_text(
                    &device.id,
                    scramble_input_device_identifiers,
                )
            ));
        }
    } else if let Some(device_window) = &state.audio_input_device_window {
        push_audio_input_device_window_diagnostic_text(
            &mut lines,
            device_window,
            scramble_input_device_identifiers,
        );
    } else if state.scene_kind == SceneWindowKind::AudioDaemon {
        push_audio_daemon_diagnostic_text(&mut lines);
    } else if state.scene_kind == SceneWindowKind::Logs {
        lines.push(String::new());
        lines.push(logs_diagnostic_text());
    } else if state.scene_kind == SceneWindowKind::TimelinePlaygroundDetail {
        let detail_state = timeline_playground_detail_window_state(state);
        lines.push(String::new());
        lines.push(timeline_playground_detail_pretty_text(&detail_state));
    }

    lines.push(String::new());
    lines.push("actions".to_owned());
    for spec in windows_scene::scene_button_specs(state.scene_kind) {
        let status = if scene_action_active(spec.action) {
            "active"
        } else {
            "available"
        };
        lines.push(format!("- {}\t{}", spec.label, status));
    }

    lines.join("\n")
}

fn push_audio_input_device_window_diagnostic_text(
    lines: &mut Vec<String>,
    device_window: &AudioInputDeviceWindowState,
    scramble_input_device_identifiers: bool,
) {
    lines.push(format!(
        "audio-input-selected-device\t{}",
        device_window.device.name
    ));
    lines.push(format!(
        "audio-input-endpoint-id\t{}",
        windows_scene::input_device_identifier_display_text(
            &device_window.device.id,
            scramble_input_device_identifiers,
        )
    ));
    lines.push(format!(
        "audio-input-sample-rate\t{}",
        device_window
            .device
            .sample_rate_hz
            .map_or_else(|| "unknown".to_owned(), |rate| rate.to_string())
    ));
    lines.push(format!(
        "audio-input-armed-for-record\t{}",
        device_window.armed_for_record
    ));
    lines.push(format!(
        "audio-input-recording\t{}",
        device_window.is_recording()
    ));
    lines.push(format!(
        "audio-input-playing\t{}",
        device_window.is_playing()
    ));
    lines.push(format!(
        "audio-input-buffer-duration\t{:.3}",
        device_window.runtime.duration_seconds()
    ));
    lines.push(format!(
        "audio-input-recording-head\t{:.3}",
        device_window.runtime.recording_head_seconds
    ));
    lines.push(format!(
        "audio-input-playback-head\t{:.3}",
        device_window.runtime.playback.head_seconds
    ));
    lines.push(format!(
        "audio-input-transcription-head\t{:.3}",
        device_window.runtime.transcription_head_seconds
    ));
    if let Some(selection) = device_window.runtime.selection {
        lines.push(format!(
            "audio-input-selection\t{:.3}\t{:.3}",
            selection.begin_seconds, selection.end_seconds
        ));
    }
    if let Some(error) = device_window.runtime.last_error() {
        lines.push(format!("audio-input-error\t{error}"));
    }
}

fn push_audio_daemon_diagnostic_text(lines: &mut Vec<String>) {
    let daemon_status = super::audio_transcription_daemon_status(&crate::paths::CACHE_DIR);
    lines.push(format!(
        "audio-daemon-entrypoint\t{}",
        daemon_status.python_entrypoint
    ));
    lines.push(format!(
        "audio-daemon-control-transport\t{}",
        daemon_status.control_transport
    ));
    lines.push(format!(
        "audio-daemon-payload-transport\t{}",
        daemon_status.payload_transport
    ));
    lines.push(format!(
        "audio-daemon-selected-model\t{}",
        daemon_status.selected_model
    ));
    lines.push(format!(
        "audio-daemon-tensor\t{}x{}\t{}\t{} bytes",
        daemon_status.tensor_mel_bins,
        daemon_status.tensor_frames,
        daemon_status.tensor_dtype,
        daemon_status.tensor_bytes
    ));
    lines.push(format!(
        "audio-daemon-shared-memory-pool\t{} slots\t{} bytes",
        daemon_status.shared_memory_minimum_slots, daemon_status.shared_memory_total_bytes
    ));
    lines.push(format!(
        "audio-daemon-queue\t{} requests\t{} ms oldest\t{} ms python lag",
        daemon_status.queued_request_count,
        daemon_status.oldest_queued_age_ms,
        daemon_status.python_lag_ms
    ));
    lines.push(format!(
        "audio-daemon-source\t{}",
        daemon_status.daemon_source_dir
    ));
    lines.push(format!(
        "audio-daemon-model-cache\t{}",
        daemon_status.model_cache_dir
    ));
}

fn scene_action_active(action: SceneAction) -> bool {
    matches!(
        (action, current_bell_source()),
        (SceneAction::SelectWindowsBell, BellSource::Windows)
            | (SceneAction::SelectFileBell, BellSource::File(_))
    )
}

fn scene_action_at_point(
    scene_kind: SceneWindowKind,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<SceneAction> {
    let specs = windows_scene::scene_button_specs(scene_kind);
    let (button_rect, max_button_size) = if scene_kind == SceneWindowKind::TimelinePlayground {
        (
            windows_scene::timeline_playground_layout(layout.terminal_panel_rect().inset(24), 0)
                .controls_rect,
            76,
        )
    } else {
        (
            layout.terminal_panel_rect(),
            scaled_scene_button_size(system_dpi()),
        )
    };
    let button_layouts =
        windows_scene::layout_scene_buttons(button_rect, specs.len(), max_button_size);
    specs
        .iter()
        .zip(button_layouts)
        .find_map(|(spec, button_layout)| {
            button_layout
                .hit_rect()
                .contains(point)
                .then_some(spec.action)
        })
}

fn timeline_add_track_button_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> bool {
    state.scene_kind == SceneWindowKind::Timeline && !state.diagnostics_visible && {
        let timeline_layout =
            windows_scene::timeline_document_layout(layout.terminal_panel_rect().inset(24));
        timeline_layout.add_track_rect.contains(point)
            || windows_scene::timeline_add_track_list_item_rect(
                timeline_layout,
                timeline_track_count(state),
                timeline_current_vertical_scroll_offset(state, layout),
            )
            .is_some_and(|rect| rect.contains(point))
    }
}

fn timeline_viewport_control_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<SceneAction> {
    if state.scene_kind != SceneWindowKind::Timeline || state.diagnostics_visible {
        return None;
    }

    let timeline_layout =
        windows_scene::timeline_document_layout(layout.terminal_panel_rect().inset(24));
    [
        SceneAction::PanTimelineLeft,
        SceneAction::PanTimelineRight,
        SceneAction::ZoomTimelineIn,
        SceneAction::ZoomTimelineOut,
        SceneAction::SelectTimelineTool,
        SceneAction::SelectTimelineBrush,
    ]
    .into_iter()
    .find(|action| {
        windows_scene::timeline_viewport_control_rect(timeline_layout, *action)
            .is_some_and(|rect| rect.contains(point))
    })
}

fn timeline_live_audio_track_index(state: &SceneAppState) -> Option<usize> {
    let document = timeline_document_handle(state)?;
    document
        .tracks()
        .iter()
        .position(|track| track.kind() == crate::timeline::TimelineTrackKind::Audio)
}

fn timeline_live_transcription_track_index(state: &SceneAppState) -> Option<usize> {
    let document = timeline_document_handle(state)?;
    document
        .tracks()
        .iter()
        .position(|track| track.kind() == crate::timeline::TimelineTrackKind::Transcription)
}

fn timeline_text_track_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<(usize, TimelineTrackId)> {
    let vertical_scroll_offset = timeline_current_vertical_scroll_offset(state, layout);
    let document = timeline_document_snapshot(state)?;
    let timeline_layout = timeline_layout(layout);
    let row_index = windows_scene::timeline_track_index_at_point(
        timeline_layout,
        vertical_scroll_offset,
        document.tracks().len(),
        point,
    )?;
    let track = document.tracks().get(row_index)?;
    (track.kind() == crate::timeline::TimelineTrackKind::Text).then_some((row_index, track.id()))
}

fn timeline_text_block_tooltip_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<(String, ClientRect)> {
    let document = timeline_document_snapshot(state)?;
    let timeline_layout = timeline_layout(layout);
    let vertical_scroll_offset = timeline_current_vertical_scroll_offset(state, layout);
    let viewport = current_timeline_zoom_viewport(state);

    for (row_index, track) in document.tracks().iter().enumerate() {
        if track.kind() != crate::timeline::TimelineTrackKind::Text {
            continue;
        }
        let Some(row_rect) = windows_scene::timeline_track_row_rect(
            timeline_layout.scrollport_rect,
            row_index,
            vertical_scroll_offset,
        ) else {
            continue;
        };

        for block in document
            .text_blocks()
            .iter()
            .filter(|block| block.track_id() == track.id())
        {
            let Some(rect) = windows_scene::timeline_time_range_clip_rect(
                row_rect,
                timeline_layout.scrollport_rect,
                viewport,
                block.time_range(),
                28,
            ) else {
                continue;
            };
            if rect.contains(point) {
                let text = if block.text().is_empty() {
                    "Empty text block".to_owned()
                } else {
                    block.text().to_owned()
                };
                return Some((text, rect));
            }
        }
    }

    None
}

#[expect(
    clippy::too_many_lines,
    reason = "transcription settings hover and shader state are centralized for matching controls"
)]
fn timeline_transcription_settings_view_state(
    state: &SceneAppState,
    layout: TerminalLayout,
) -> Option<windows_scene::TimelineTranscriptionSettingsViewState> {
    let mut settings = state.timeline_transcription_settings?;
    let point = state.pointer_position;
    settings.add_text_track_button_visual_state = windows_scene::compute_button_visual_state(
        windows_scene::timeline_transcription_settings_add_text_track_button_rect(layout),
        point,
        false,
        None,
        false,
        Instant::now(),
    );
    settings.add_audio_track_button_visual_state = windows_scene::compute_button_visual_state(
        windows_scene::timeline_transcription_settings_add_audio_track_button_rect(layout),
        point,
        false,
        None,
        false,
        Instant::now(),
    );
    settings.advance_boundaries_toggle_visual_state = windows_scene::compute_button_visual_state(
        windows_scene::timeline_transcription_settings_advance_boundaries_toggle_rect(layout),
        point,
        false,
        None,
        false,
        Instant::now(),
    );
    settings.submit_chunks_toggle_visual_state = windows_scene::compute_button_visual_state(
        windows_scene::timeline_transcription_settings_submit_chunks_toggle_rect(layout),
        point,
        false,
        None,
        false,
        Instant::now(),
    );
    settings.manual_flush_button_visual_state = windows_scene::compute_button_visual_state(
        windows_scene::timeline_transcription_settings_manual_flush_button_rect(layout),
        point,
        false,
        None,
        false,
        Instant::now(),
    );
    settings.hovered_model_index =
        point.and_then(|point| timeline_transcription_settings_model_row_at_point(layout, point));
    settings.hovered_input_audio_track_index = point
        .and_then(|point| timeline_transcription_settings_input_row_at_point(state, layout, point));
    settings.hovered_output_text_track_index = point.and_then(|point| {
        timeline_transcription_settings_output_row_at_point(state, layout, point)
    });
    settings.hovered_model_socket = point.is_some_and(|point| {
        windows_scene::timeline_transcription_settings_model_socket_rect(layout).contains(point)
    });
    settings.hovered_input_socket = point.is_some_and(|point| {
        windows_scene::timeline_transcription_settings_input_socket_rect(layout).contains(point)
    });
    settings.hovered_output_socket = point.is_some_and(|point| {
        windows_scene::timeline_transcription_settings_output_socket_rect(layout).contains(point)
    });
    settings.hovered_model_target = point.is_some_and(|point| {
        windows_scene::timeline_transcription_settings_target_puck_rect(
            layout,
            settings,
            windows_scene::TimelineTranscriptionSettingsTarget::Model,
        )
        .is_some_and(|rect| rect.contains(point))
    });
    settings.hovered_input_target = point.is_some_and(|point| {
        windows_scene::timeline_transcription_settings_target_puck_rect(
            layout,
            settings,
            windows_scene::TimelineTranscriptionSettingsTarget::InputAudioTrack,
        )
        .is_some_and(|rect| rect.contains(point))
    });
    settings.hovered_output_target = point.is_some_and(|point| {
        windows_scene::timeline_transcription_settings_target_puck_rect(
            layout,
            settings,
            windows_scene::TimelineTranscriptionSettingsTarget::OutputTextTrack,
        )
        .is_some_and(|rect| rect.contains(point))
    });
    settings.hovered_add_text_track_button = point.is_some_and(|point| {
        windows_scene::timeline_transcription_settings_add_text_track_button_rect(layout)
            .contains(point)
    });
    settings.hovered_add_audio_track_button = point.is_some_and(|point| {
        windows_scene::timeline_transcription_settings_add_audio_track_button_rect(layout)
            .contains(point)
    });
    settings.hovered_advance_boundaries_toggle = point.is_some_and(|point| {
        windows_scene::timeline_transcription_settings_advance_boundaries_toggle_rect(layout)
            .contains(point)
    });
    settings.hovered_submit_chunks_toggle = point.is_some_and(|point| {
        windows_scene::timeline_transcription_settings_submit_chunks_toggle_rect(layout)
            .contains(point)
    });
    settings.hovered_manual_flush_button = point.is_some_and(|point| {
        windows_scene::timeline_transcription_settings_manual_flush_button_rect(layout)
            .contains(point)
    });
    Some(settings)
}

fn timeline_transcription_settings_target_column(
    target: windows_scene::TimelineTranscriptionSettingsTarget,
) -> windows_scene::TimelineTranscriptionSettingsColumn {
    match target {
        windows_scene::TimelineTranscriptionSettingsTarget::Model => {
            windows_scene::TimelineTranscriptionSettingsColumn::Model
        }
        windows_scene::TimelineTranscriptionSettingsTarget::InputAudioTrack => {
            windows_scene::TimelineTranscriptionSettingsColumn::InputAudioTrack
        }
        windows_scene::TimelineTranscriptionSettingsTarget::OutputTextTrack => {
            windows_scene::TimelineTranscriptionSettingsColumn::OutputTextTrack
        }
    }
}

fn clear_timeline_transcription_settings_drag(state: &mut SceneAppState) {
    if let Some(settings) = state.timeline_transcription_settings.as_mut() {
        settings.dragging_target = None;
        settings.drag_position = None;
    }
}

fn select_timeline_transcription_settings_model(
    state: &mut SceneAppState,
    model_index: usize,
    dock_target: bool,
) {
    if let Some(settings) = state.timeline_transcription_settings.as_mut() {
        settings.selected_column = windows_scene::TimelineTranscriptionSettingsColumn::Model;
        settings.selected_model_index = model_index;
        if dock_target {
            settings.model_target_docked = true;
        }
    }
    apply_timeline_transcription_settings_selection(state);
}

fn select_timeline_transcription_settings_input_audio_track(
    state: &mut SceneAppState,
    target_index: usize,
    dock_target: bool,
) {
    if let Some(settings) = state.timeline_transcription_settings.as_mut() {
        settings.selected_column =
            windows_scene::TimelineTranscriptionSettingsColumn::InputAudioTrack;
        settings.selected_input_audio_track_index = target_index;
        if dock_target {
            settings.input_target_docked = true;
        }
    }
    apply_timeline_transcription_settings_selection(state);
}

fn select_timeline_transcription_settings_output_text_track(
    state: &mut SceneAppState,
    target_index: usize,
    dock_target: bool,
) {
    if let Some(settings) = state.timeline_transcription_settings.as_mut() {
        settings.selected_column =
            windows_scene::TimelineTranscriptionSettingsColumn::OutputTextTrack;
        settings.selected_output_text_track_index = target_index;
        if dock_target {
            settings.output_target_docked = true;
        }
    }
    apply_timeline_transcription_settings_selection(state);
}

fn append_text_track_from_timeline_transcription_settings(state: &mut SceneAppState) {
    let Some(settings) = state.timeline_transcription_settings else {
        return;
    };
    let Some(document) = timeline_document_handle_mut(state) else {
        return;
    };
    let selected_target_index = {
        let track_id = document.append_text_track();
        document
            .tracks()
            .iter()
            .filter(|track| track.kind() == crate::timeline::TimelineTrackKind::Text)
            .map(crate::timeline::TimelineTrack::id)
            .position(|candidate| candidate == track_id)
            .map_or(0, |index| index + 1)
    };
    let _ = document.set_transcription_track_target_text_track(
        settings.track_id,
        document
            .tracks()
            .iter()
            .filter(|track| track.kind() == crate::timeline::TimelineTrackKind::Text)
            .map(crate::timeline::TimelineTrack::id)
            .nth(selected_target_index.saturating_sub(1)),
    );
    if let Some(settings) = state.timeline_transcription_settings.as_mut() {
        settings.selected_column =
            windows_scene::TimelineTranscriptionSettingsColumn::OutputTextTrack;
        settings.selected_output_text_track_index = selected_target_index;
        settings.output_target_docked = true;
    }
    apply_timeline_transcription_settings_selection(state);
    let _ = send_timeline_document_command(
        state,
        TimelineDocumentCommand::AppendTextTrackForTranscriptionSettings {
            track_id: settings.track_id,
        },
    );
}

fn open_audio_track_picker_from_timeline_transcription_settings(state: &mut SceneAppState) {
    let mut picker =
        AudioInputPickerState::new(list_active_audio_input_devices().unwrap_or_default());
    if let Some(default_index) = picker.devices.iter().position(|device| device.is_default) {
        picker.select_index(default_index);
    }
    state.audio_input_picker = picker;
    state.audio_input_picker_completion =
        AudioInputPickerCompletion::TimelineTranscriptionInputTrack;
    state.scene_kind = SceneWindowKind::AudioInputDevicePicker;
    state.scene_action_selected_index = 0;
    state.scene_virtual_cursor = None;
}

fn toggle_timeline_transcription_settings_advance_boundaries(state: &mut SceneAppState) {
    update_timeline_transcription_settings_automation(state, |advance, submit| (!advance, submit));
}

fn toggle_timeline_transcription_settings_submit_chunks(state: &mut SceneAppState) {
    update_timeline_transcription_settings_automation(state, |advance, submit| (advance, !submit));
}

fn update_timeline_transcription_settings_automation(
    state: &mut SceneAppState,
    update: impl FnOnce(bool, bool) -> (bool, bool),
) {
    let Some(settings) = state.timeline_transcription_settings else {
        return;
    };
    let Some(document) = timeline_document_handle(state) else {
        return;
    };
    let Some(track) = document
        .tracks()
        .iter()
        .find(|track| track.id() == settings.track_id)
    else {
        return;
    };
    let crate::timeline::TimelineTrackProjection::Transcription(projection) = track.projection()
    else {
        return;
    };
    let (advance, submit) = update(
        projection.automatically_advance_chunk_boundaries(),
        projection.automatically_submit_chunks(),
    );
    let changed = timeline_document_handle_mut(state).is_some_and(|document| {
        document.set_transcription_track_automation(settings.track_id, advance, submit)
    });
    if changed {
        let _ = send_timeline_document_command(
            state,
            TimelineDocumentCommand::SetTranscriptionAutomation {
                track_id: settings.track_id,
                advance_boundaries: advance,
                submit_chunks: submit,
            },
        );
    }
}

fn request_timeline_transcription_manual_flush(state: &mut SceneAppState) {
    let Some(model_name) = selected_timeline_transcription_model_name(state) else {
        super::jobs::record_failed_job(
            "Timeline transcription manual flush",
            "No transcription track is attached. Select a transcription track before manually flushing a chunk.",
        );
        return;
    };
    if let Some(device_window) = state.audio_input_device_window.as_mut() {
        device_window.set_transcription_model_name(model_name);
        device_window.flush_transcription_chunk();
        return;
    }

    super::jobs::record_failed_job(
        "Timeline transcription manual flush",
        "No live input audio track is attached. Target an audio track before manually flushing a transcription chunk.",
    );
}

fn model_warning_open_button_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<usize> {
    let warning = state.model_warning.as_ref()?;
    (0..warning.status.locations.len()).find(|index| {
        windows_scene::model_warning_location_open_rect(layout, *index)
            .is_some_and(|rect| rect.contains(point))
    })
}

fn model_warning_copy_button_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<usize> {
    let warning = state.model_warning.as_ref()?;
    (0..warning.status.locations.len()).find(|index| {
        windows_scene::model_warning_location_copy_rect(layout, *index)
            .is_some_and(|rect| rect.contains(point))
    })
}

fn open_model_warning_location(state: &SceneAppState, index: usize) {
    let Some(location) = state
        .model_warning
        .as_ref()
        .and_then(|warning| warning.status.locations.get(index))
    else {
        return;
    };
    let path = if location.path.is_dir() {
        location.path.clone()
    } else {
        location
            .path
            .parent()
            .map_or_else(|| location.path.clone(), Path::to_path_buf)
    };
    if let Err(error) = std::process::Command::new("explorer.exe")
        .arg(&path)
        .spawn()
    {
        error!(?error, path = %path.display(), "failed to open model warning location");
    }
}

fn copy_model_warning_location(state: &SceneAppState, index: usize) {
    let Some(location) = state
        .model_warning
        .as_ref()
        .and_then(|warning| warning.status.locations.get(index))
    else {
        return;
    };
    if let Err(error) = write_clipboard(location.path.display().to_string()) {
        error!(?error, path = %location.path.display(), "failed to copy model warning location");
    }
}

fn timeline_transcription_settings_target_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<windows_scene::TimelineTranscriptionSettingsTarget> {
    let settings = state.timeline_transcription_settings?;
    [
        windows_scene::TimelineTranscriptionSettingsTarget::Model,
        windows_scene::TimelineTranscriptionSettingsTarget::InputAudioTrack,
        windows_scene::TimelineTranscriptionSettingsTarget::OutputTextTrack,
    ]
    .into_iter()
    .find(|target| {
        windows_scene::timeline_transcription_settings_target_puck_rect(layout, settings, *target)
            .is_some_and(|rect| rect.contains(point))
    })
}

fn timeline_transcription_settings_socket_at_point(
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<windows_scene::TimelineTranscriptionSettingsTarget> {
    if windows_scene::timeline_transcription_settings_model_socket_rect(layout).contains(point) {
        return Some(windows_scene::TimelineTranscriptionSettingsTarget::Model);
    }
    if windows_scene::timeline_transcription_settings_input_socket_rect(layout).contains(point) {
        return Some(windows_scene::TimelineTranscriptionSettingsTarget::InputAudioTrack);
    }
    if windows_scene::timeline_transcription_settings_output_socket_rect(layout).contains(point) {
        return Some(windows_scene::TimelineTranscriptionSettingsTarget::OutputTextTrack);
    }
    None
}

fn timeline_transcription_settings_target_cursor_jump_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    target: windows_scene::TimelineTranscriptionSettingsTarget,
) -> Option<ClientPoint> {
    let settings = state.timeline_transcription_settings?;
    let puck_rect =
        windows_scene::timeline_transcription_settings_target_puck_rect(layout, settings, target)?;
    Some(ClientPoint::new(
        puck_rect.left() + (puck_rect.width() / 2),
        puck_rect.top() + (puck_rect.height() / 2),
    ))
}

fn timeline_transcription_settings_add_text_track_button_at_point(
    layout: TerminalLayout,
    point: ClientPoint,
) -> bool {
    windows_scene::timeline_transcription_settings_add_text_track_button_rect(layout)
        .contains(point)
}

fn timeline_transcription_settings_add_audio_track_button_at_point(
    layout: TerminalLayout,
    point: ClientPoint,
) -> bool {
    windows_scene::timeline_transcription_settings_add_audio_track_button_rect(layout)
        .contains(point)
}

fn timeline_transcription_settings_add_text_track_tooltip(
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<(&'static str, ClientRect)> {
    let rect = windows_scene::timeline_transcription_settings_add_text_track_button_rect(layout);
    rect.contains(point)
        .then_some(("Add a new text track for transcription output", rect))
}

fn timeline_transcription_settings_model_row_at_point(
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<usize> {
    (0..KNOWN_WHISPER_MODELS.len()).find(|index| {
        windows_scene::timeline_transcription_settings_model_row_rect(layout, *index)
            .is_some_and(|rect| rect.contains(point))
    })
}

fn timeline_transcription_settings_input_row_count(state: &SceneAppState) -> usize {
    timeline_document_handle(state).map_or(1, |document| {
        document
            .tracks()
            .iter()
            .filter(|track| track.kind() == crate::timeline::TimelineTrackKind::Audio)
            .count()
            + 1
    })
}

fn timeline_transcription_settings_output_row_count(state: &SceneAppState) -> usize {
    timeline_document_handle(state).map_or(1, |document| {
        document
            .tracks()
            .iter()
            .filter(|track| track.kind() == crate::timeline::TimelineTrackKind::Text)
            .count()
            + 1
    })
}

fn timeline_transcription_settings_input_row_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<usize> {
    (0..timeline_transcription_settings_input_row_count(state)).find(|index| {
        windows_scene::timeline_transcription_settings_input_row_rect(layout, *index)
            .is_some_and(|rect| rect.contains(point))
    })
}

fn timeline_transcription_settings_output_row_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<usize> {
    (0..timeline_transcription_settings_output_row_count(state)).find(|index| {
        windows_scene::timeline_transcription_settings_output_row_rect(layout, *index)
            .is_some_and(|rect| rect.contains(point))
    })
}

fn timeline_transcription_settings_state_for_document(
    document: &TimelineDocument,
    track_index: usize,
) -> Option<windows_scene::TimelineTranscriptionSettingsViewState> {
    let track = document.tracks().get(track_index)?;
    let crate::timeline::TimelineTrackProjection::Transcription(projection) = track.projection()
    else {
        return None;
    };
    let text_track_ids = document
        .tracks()
        .iter()
        .filter(|track| track.kind() == crate::timeline::TimelineTrackKind::Text)
        .map(crate::timeline::TimelineTrack::id)
        .collect::<Vec<_>>();
    let audio_track_ids = document
        .tracks()
        .iter()
        .filter(|track| track.kind() == crate::timeline::TimelineTrackKind::Audio)
        .map(crate::timeline::TimelineTrack::id)
        .collect::<Vec<_>>();
    let selected_model_index = KNOWN_WHISPER_MODELS
        .iter()
        .position(|model| model.name == projection.model_name())
        .unwrap_or_default();
    let selected_input_audio_track_index = projection
        .target_audio_track_id()
        .and_then(|track_id| {
            audio_track_ids
                .iter()
                .position(|candidate| *candidate == track_id)
        })
        .map_or(0, |index| index + 1);
    let selected_output_text_track_index = projection
        .target_text_track_id()
        .and_then(|track_id| {
            text_track_ids
                .iter()
                .position(|candidate| *candidate == track_id)
        })
        .map_or(0, |index| index + 1);

    Some(windows_scene::TimelineTranscriptionSettingsViewState {
        track_id: track.id(),
        selected_column: windows_scene::TimelineTranscriptionSettingsColumn::Model,
        selected_model_index,
        selected_input_audio_track_index,
        selected_output_text_track_index,
        add_text_track_button_visual_state:
            crate::app::windows_d3d12_renderer::ButtonVisualState::default(),
        add_audio_track_button_visual_state:
            crate::app::windows_d3d12_renderer::ButtonVisualState::default(),
        advance_boundaries_toggle_visual_state:
            crate::app::windows_d3d12_renderer::ButtonVisualState::default(),
        submit_chunks_toggle_visual_state:
            crate::app::windows_d3d12_renderer::ButtonVisualState::default(),
        manual_flush_button_visual_state:
            crate::app::windows_d3d12_renderer::ButtonVisualState::default(),
        model_target_docked: true,
        input_target_docked: projection.target_audio_track_id().is_some(),
        output_target_docked: projection.target_text_track_id().is_some(),
        dragging_target: None,
        drag_position: None,
        hovered_model_index: None,
        hovered_input_audio_track_index: None,
        hovered_output_text_track_index: None,
        hovered_model_socket: false,
        hovered_input_socket: false,
        hovered_output_socket: false,
        hovered_model_target: false,
        hovered_input_target: false,
        hovered_output_target: false,
        hovered_add_text_track_button: false,
        hovered_add_audio_track_button: false,
        hovered_advance_boundaries_toggle: false,
        hovered_submit_chunks_toggle: false,
        hovered_manual_flush_button: false,
    })
}

fn open_timeline_transcription_settings_window_from_scene(track_index: usize) -> eyre::Result<()> {
    let Some((
        app_home,
        vt_engine,
        timeline_document,
        timeline_document_command_sender,
        timeline_document_command_target,
        timeline_transcription_settings,
    )) = with_scene_app_state(|state| {
        let Some(timeline_document) = timeline_document_handle(state) else {
            return Ok(None);
        };
        Ok(
            timeline_transcription_settings_state_for_document(timeline_document, track_index).map(
                |timeline_transcription_settings| {
                    (
                        state.app_home.clone(),
                        state.vt_engine,
                        timeline_document.clone(),
                        state.timeline_document_command_sender.clone(),
                        state.timeline_document_command_target,
                        timeline_transcription_settings,
                    )
                },
            ),
        )
    })?
    else {
        return Ok(());
    };

    thread::Builder::new()
        .name("teamy-studio-timeline-transcription-settings".to_owned())
        .spawn_with_current_span(move || {
            if let Err(error) = run_scene_window(
                &app_home,
                SceneWindowKind::TimelineTranscriptionSettings,
                vt_engine,
                SceneWindowInitialization {
                    timeline_document: Some(timeline_document),
                    timeline_document_command_sender,
                    timeline_document_command_target,
                    timeline_transcription_settings: Some(timeline_transcription_settings),
                    ..Default::default()
                },
            ) {
                error!(
                    ?error,
                    "failed to open timeline transcription settings window"
                );
            }
        })
        .wrap_err("failed to spawn Teamy Studio timeline transcription settings window thread")?;
    Ok(())
}

fn open_jobs_window_from_scene_state(state: &SceneAppState) {
    let app_home = state.app_home.clone();
    let vt_engine = state.vt_engine;
    let spawn_result = thread::Builder::new()
        .name("teamy-studio-jobs".to_owned())
        .spawn_with_current_span(move || {
            if let Err(error) = run_scene_window(
                &app_home,
                SceneWindowKind::Jobs,
                vt_engine,
                SceneWindowInitialization::default(),
            ) {
                error!(?error, "failed to open jobs window");
            }
        });
    if let Err(error) = spawn_result {
        error!(?error, "failed to spawn Teamy Studio jobs thread");
    }
}

fn open_model_warning_window_from_scene_state(
    state: &SceneAppState,
    warning: windows_scene::ModelWarningViewState,
) {
    let app_home = state.app_home.clone();
    let vt_engine = state.vt_engine;
    open_model_warning_window(&app_home, vt_engine, warning);
}

fn open_model_warning_window(
    app_home: &AppHome,
    vt_engine: VtEngineChoice,
    warning: windows_scene::ModelWarningViewState,
) {
    ring_terminal_bell();
    let app_home = app_home.clone();
    let spawn_result = thread::Builder::new()
        .name("teamy-studio-model-warning".to_owned())
        .spawn_with_current_span(move || {
            if let Err(error) = run_scene_window(
                &app_home,
                SceneWindowKind::ModelWarning,
                vt_engine,
                SceneWindowInitialization {
                    model_warning: Some(warning),
                    ..Default::default()
                },
            ) {
                error!(?error, "failed to open model warning window");
            }
        });
    if let Err(error) = spawn_result {
        error!(?error, "failed to spawn Teamy Studio model warning thread");
    }
}

fn transcription_model_warning_for_selected_settings(
    state: &SceneAppState,
) -> Option<windows_scene::ModelWarningViewState> {
    let settings = state.timeline_transcription_settings?;
    let model_name = KNOWN_WHISPER_MODELS
        .get(settings.selected_model_index)
        .map_or(KNOWN_WHISPER_MODELS[0].name, |model| model.name);
    let status = crate::model::inspect_whisper_model_preparation(
        &state.app_home,
        &crate::paths::CACHE_DIR,
        model_name,
    );
    (!status.is_compatible()).then_some(windows_scene::ModelWarningViewState { status })
}

fn toggle_audio_input_transcription_from_scene_state(state: &mut SceneAppState) {
    let selected_model_name = selected_timeline_transcription_model_name(state);
    if state
        .audio_input_device_window
        .as_ref()
        .is_some_and(|device_window| !device_window.runtime.transcription.enabled)
        && let Some(warning) = transcription_model_warning_for_timeline(state)
    {
        if let Some(device_window) = state.audio_input_device_window.as_mut() {
            device_window.runtime.transcription.enabled = false;
        }
        open_model_warning_window_from_scene_state(state, warning);
        return;
    }

    if let Some(device_window) = state.audio_input_device_window.as_mut() {
        if let Some(model_name) = selected_model_name {
            device_window.set_transcription_model_name(model_name);
        }
        device_window.toggle_transcription();
        return;
    }

    tracing::warn!(
        scene_kind = ?state.scene_kind,
        "Timeline transcription toggle ignored because no live audio device is attached"
    );
    super::jobs::record_failed_job(
        "Timeline transcription",
        "No live microphone track is attached. Add a Microphone track and select an input device before enabling transcription.",
    );
}

fn selected_timeline_transcription_model_name(state: &SceneAppState) -> Option<String> {
    let document = timeline_document_handle(state)?;
    document.tracks().iter().find_map(|track| {
        let crate::timeline::TimelineTrackProjection::Transcription(projection) =
            track.projection()
        else {
            return None;
        };
        Some(projection.model_name().to_owned())
    })
}

fn transcription_model_warning_for_timeline(
    state: &SceneAppState,
) -> Option<windows_scene::ModelWarningViewState> {
    let model_name = selected_timeline_transcription_model_name(state)?;
    let status = crate::model::inspect_whisper_model_preparation(
        &state.app_home,
        &crate::paths::CACHE_DIR,
        &model_name,
    );
    (!status.is_compatible()).then_some(windows_scene::ModelWarningViewState { status })
}

fn apply_timeline_transcription_settings_selection(state: &mut SceneAppState) {
    let Some(settings) = state.timeline_transcription_settings else {
        return;
    };
    let changed = apply_timeline_transcription_settings_to_document(state, settings);
    if changed {
        let _ = send_timeline_document_command(
            state,
            TimelineDocumentCommand::ApplyTranscriptionSettings(settings),
        );
    }
}

fn apply_timeline_transcription_settings_to_document(
    state: &mut SceneAppState,
    settings: windows_scene::TimelineTranscriptionSettingsViewState,
) -> bool {
    let Some(document) = timeline_document_handle_mut(state) else {
        return false;
    };
    let model_name = KNOWN_WHISPER_MODELS
        .get(settings.selected_model_index)
        .map_or(KNOWN_WHISPER_MODELS[0].name, |model| model.name);
    let text_track_ids = document
        .tracks()
        .iter()
        .filter(|track| track.kind() == crate::timeline::TimelineTrackKind::Text)
        .map(crate::timeline::TimelineTrack::id)
        .collect::<Vec<_>>();
    let audio_track_ids = document
        .tracks()
        .iter()
        .filter(|track| track.kind() == crate::timeline::TimelineTrackKind::Audio)
        .map(crate::timeline::TimelineTrack::id)
        .collect::<Vec<_>>();
    let target_audio_track_id = settings
        .selected_input_audio_track_index
        .checked_sub(1)
        .and_then(|index| audio_track_ids.get(index).copied());
    let target_track_id = settings
        .selected_output_text_track_index
        .checked_sub(1)
        .and_then(|index| text_track_ids.get(index).copied());
    let model_changed = document.set_transcription_track_model_name(settings.track_id, model_name);
    let input_changed = document
        .set_transcription_track_target_audio_track(settings.track_id, target_audio_track_id);
    let target_changed =
        document.set_transcription_track_target_text_track(settings.track_id, target_track_id);
    model_changed || input_changed || target_changed
}

fn timeline_track_reorder_handle_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<usize> {
    let document = timeline_document_handle(state)?;
    let timeline_layout = timeline_layout(layout);
    (0..document.tracks().len()).find(|index| {
        windows_scene::timeline_track_reorder_handle_rect(timeline_layout, *index)
            .is_some_and(|rect| rect.contains(point))
    })
}

fn timeline_track_reorder_index_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<usize> {
    let document = timeline_document_handle(state)?;
    if document.tracks().is_empty() {
        return None;
    }
    let timeline_layout = timeline_layout(layout);
    let mut closest_index = 0;
    let mut closest_distance = i32::MAX;
    let point_y = point.to_win32_point().map_or(0, |point| point.y);
    for index in 0..document.tracks().len() {
        let Some(row_rect) = windows_scene::timeline_track_list_row_rect(timeline_layout, index)
        else {
            continue;
        };
        let distance = (point_y - (row_rect.top() + (row_rect.height() / 2))).abs();
        if distance < closest_distance {
            closest_distance = distance;
            closest_index = index;
        }
    }
    Some(closest_index)
}

fn timeline_transport_play_button_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> bool {
    state.scene_kind == SceneWindowKind::Timeline
        && !state.diagnostics_visible
        && state.audio_input_device_window.is_some()
        && windows_scene::timeline_transport_play_button_rect(timeline_layout(layout))
            .contains(point)
}

fn timeline_track_record_button_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<usize> {
    let row_index = timeline_live_audio_track_index(state)?;
    windows_scene::timeline_track_record_button_rect(timeline_layout(layout), row_index)
        .filter(|rect| rect.contains(point))
        .map(|_| row_index)
}

fn timeline_track_playback_button_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<usize> {
    let row_index = timeline_live_audio_track_index(state)?;
    windows_scene::timeline_track_playback_button_rect(timeline_layout(layout), row_index)
        .filter(|rect| rect.contains(point))
        .map(|_| row_index)
}

fn timeline_track_loopback_button_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<usize> {
    let row_index = timeline_live_audio_track_index(state)?;
    windows_scene::timeline_track_loopback_button_rect(timeline_layout(layout), row_index)
        .filter(|rect| rect.contains(point))
        .map(|_| row_index)
}

fn timeline_track_transcription_toggle_button_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<usize> {
    let row_index = timeline_live_transcription_track_index(state)?;
    windows_scene::timeline_track_transcription_toggle_button_rect(
        timeline_layout(layout),
        row_index,
    )
    .filter(|rect| rect.contains(point))
    .map(|_| row_index)
}

fn timeline_track_transcription_settings_button_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<usize> {
    let row_index = timeline_live_transcription_track_index(state)?;
    windows_scene::timeline_track_transcription_settings_button_rect(
        timeline_layout(layout),
        row_index,
    )
    .filter(|rect| rect.contains(point))
    .map(|_| row_index)
}

fn timeline_audio_head_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<AudioInputTimelineHeadKind> {
    let audio_input_state = state.audio_input_device_window.as_ref()?;
    windows_scene::timeline_audio_head_grabbers(
        timeline_layout(layout),
        current_timeline_zoom_viewport(state),
        audio_input_state,
    )
    .into_iter()
    .find_map(|grabber| grabber.rect.contains(point).then_some(grabber.kind))
}

fn timeline_track_count(state: &SceneAppState) -> usize {
    timeline_document_handle(state).map_or(0, |document| document.tracks().len())
}

fn timeline_layout(layout: TerminalLayout) -> windows_scene::TimelineDocumentLayout {
    windows_scene::timeline_document_layout(layout.terminal_panel_rect().inset(24))
}

fn timeline_selection_surface_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> bool {
    state.scene_kind == SceneWindowKind::Timeline
        && !state.diagnostics_visible
        && windows_scene::timeline_selection_surface_contains(timeline_layout(layout), point)
}

fn timeline_scroll_interaction_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> bool {
    state.scene_kind == SceneWindowKind::Timeline
        && !state.diagnostics_visible
        && windows_scene::timeline_scroll_interaction_contains(timeline_layout(layout), point)
}

fn timeline_pan_interaction_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> bool {
    state.scene_kind == SceneWindowKind::Timeline
        && !state.diagnostics_visible
        && timeline_document_handle(state).is_some()
        && windows_scene::timeline_selection_surface_contains(timeline_layout(layout), point)
}

fn timeline_playground_pan_interaction_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> bool {
    if state.scene_kind != SceneWindowKind::TimelinePlayground || state.diagnostics_visible {
        return false;
    }
    let vertical_scroll_offset = state
        .timeline_playground
        .as_ref()
        .map_or(0, |playground| playground.vertical_scroll_offset);
    let layout = windows_scene::timeline_playground_layout(
        layout.terminal_panel_rect().inset(24),
        vertical_scroll_offset,
    );
    layout.ruler_rect.contains(point) || layout.content_rect.contains(point)
}

fn apply_timeline_playground_pan_drag(
    playground: &mut TimelinePlaygroundState,
    pan_drag: TimelinePlaygroundPanDrag,
    delta_x_pixels: i32,
    vertical_delta_pixels: i32,
    viewport_width_pixels: i32,
    layout: windows_scene::TimelinePlaygroundLayout,
    row_count: usize,
) {
    // timeline[impl playground.live-tracing-pan]
    playground.live_tracing_follow_tail = false;
    let width = i128::from(viewport_width_pixels.max(1));
    let duration = i128::from(visible_duration_ns_i64(
        pan_drag.origin_visible_start_ns,
        pan_drag.origin_visible_end_ns,
    ));
    let delta_ns =
        i64::try_from(i128::from(delta_x_pixels) * duration / width).unwrap_or_else(|_| {
            if delta_x_pixels.is_negative() {
                i64::MIN
            } else {
                i64::MAX
            }
        });
    playground.visible_start_ns = pan_drag.origin_visible_start_ns.saturating_sub(delta_ns);
    playground.visible_end_ns = playground
        .visible_start_ns
        .saturating_add(i64::try_from(duration).unwrap_or(i64::MAX))
        .max(playground.visible_start_ns.saturating_add(1));
    playground.vertical_scroll_offset =
        windows_scene::timeline_playground_clamp_vertical_scroll_offset(
            layout,
            row_count,
            pan_drag
                .origin_vertical_scroll_offset
                .saturating_sub(vertical_delta_pixels),
        );
    playground.zoom_animation = None;
}

fn visible_duration_ns_i64(start_ns: i64, end_ns: i64) -> i64 {
    i64::try_from((i128::from(end_ns) - i128::from(start_ns)).max(1)).unwrap_or(i64::MAX)
}

fn timeline_playground_row_positions(
    render_plan: &TimelineRenderPlan,
) -> Vec<(TimelineRenderRowKey, i32)> {
    render_plan
        .rows()
        .iter()
        .map(|row| {
            (
                row.key(),
                windows_scene::timeline_playground_row_world_top(row.id().as_u32()),
            )
        })
        .collect()
}

fn interpolate_i32(start: i32, end: i32, progress: f64) -> i32 {
    let value = f64::from(start) + (f64::from(end) - f64::from(start)) * progress;
    f64_to_i32_saturating(value)
}

fn rebase_timeline_pan_drag_after_zoom(
    pan_drag: &mut TimelinePanDrag,
    point: ClientPoint,
    target_viewport: TimelineViewport,
    current_vertical_scroll_offset: i32,
) {
    pan_drag.origin = point;
    pan_drag.origin_viewport = target_viewport;
    pan_drag.origin_vertical_scroll_offset = current_vertical_scroll_offset;
}

fn timeline_current_vertical_scroll_offset(state: &SceneAppState, layout: TerminalLayout) -> i32 {
    windows_scene::timeline_clamp_vertical_scroll_offset(
        timeline_layout(layout),
        timeline_track_count(state),
        state.timeline_vertical_scroll_offset,
    )
}

fn timeline_time_from_client_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> TimelineTimeNs {
    current_timeline_zoom_viewport(state).x_to_time(
        windows_scene::timeline_viewport_point_from_client_point(timeline_layout(layout), point),
    )
}

#[expect(
    clippy::cast_precision_loss,
    reason = "timeline interaction maps integer nanoseconds into floating-point transport seconds for the shared audio runtime"
)]
fn timeline_seconds_from_client_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> f64 {
    timeline_time_from_client_point(state, layout, point).as_i64() as f64 / 1_000_000_000.0
}

fn timeline_track_world_y_from_client_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> i32 {
    let total_track_height =
        windows_scene::timeline_track_world_height(timeline_track_count(state));
    windows_scene::timeline_track_world_y_from_client_point(
        timeline_layout(layout),
        timeline_current_vertical_scroll_offset(state, layout),
        point,
    )
    .clamp(0, total_track_height)
}

fn timeline_selection_from_pending(
    state: &SceneAppState,
    layout: TerminalLayout,
    pending_selection: PendingTimelineSelection,
    point: ClientPoint,
) -> Option<windows_scene::TimelineRectSelection> {
    if point == pending_selection.origin {
        return None;
    }

    let time_range = TimelineTimeRangeNs::from_unordered(
        pending_selection.anchor_time,
        timeline_time_from_client_point(state, layout, point),
    );
    let total_track_height =
        windows_scene::timeline_track_world_height(timeline_track_count(state));
    let track_y_range = if pending_selection.include_all_tracks {
        windows_scene::TimelineTrackVerticalRange::new(0, total_track_height)
    } else {
        windows_scene::TimelineTrackVerticalRange::new(
            pending_selection.anchor_track_y,
            timeline_track_world_y_from_client_point(state, layout, point),
        )
    };
    Some(windows_scene::TimelineRectSelection::new(
        time_range,
        track_y_range,
    ))
}

fn current_timeline_zoom_viewport(state: &SceneAppState) -> TimelineViewport {
    let base_viewport = state
        .timeline_document
        .as_ref()
        .map_or_else(TimelineViewport::default, TimelineDocument::viewport);
    state
        .timeline_zoom_animation
        .map_or(base_viewport, |animation| {
            let progress = (animation.started_at.elapsed().as_secs_f64()
                / TIMELINE_ZOOM_ANIMATION_DURATION.as_secs_f64())
            .clamp(0.0, 1.0);
            interpolate_timeline_viewport(
                animation.start_viewport,
                animation.target_viewport,
                ease_in_out(progress),
            )
        })
}

fn cancel_timeline_zoom_animation_at_current_viewport(
    state: &mut SceneAppState,
) -> TimelineViewport {
    let viewport = current_timeline_zoom_viewport(state);
    if let Some(document) = timeline_document_handle_mut(state) {
        document.set_viewport(viewport);
    }
    state.timeline_zoom_animation = None;
    viewport
}

fn apply_timeline_zoom_animation(state: &mut SceneAppState) {
    let Some(animation) = state.timeline_zoom_animation else {
        return;
    };
    let Some(document) = timeline_document_handle_mut(state) else {
        state.timeline_zoom_animation = None;
        return;
    };

    let progress = (animation.started_at.elapsed().as_secs_f64()
        / TIMELINE_ZOOM_ANIMATION_DURATION.as_secs_f64())
    .clamp(0.0, 1.0);
    if progress >= 1.0 {
        document.set_viewport(animation.target_viewport);
        state.timeline_zoom_animation = None;
        return;
    }

    document.set_viewport(interpolate_timeline_viewport(
        animation.start_viewport,
        animation.target_viewport,
        ease_in_out(progress),
    ));
}

fn apply_timeline_playground_zoom_animation(state: &mut SceneAppState) {
    if let Some(playground) = state.timeline_playground.as_mut() {
        // timeline[impl playground.viewport-transition]
        playground.apply_zoom_animation();
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "timeline zoom animation interpolates integer nanosecond origins through display-scale f64 progress"
)]
fn interpolate_timeline_viewport(
    start: TimelineViewport,
    target: TimelineViewport,
    progress: f64,
) -> TimelineViewport {
    let origin = lerp_f64(
        start.origin().as_i64() as f64,
        target.origin().as_i64() as f64,
        progress,
    );
    let duration_per_pixel = lerp_f64(
        start.duration_per_pixel().get::<nanosecond>(),
        target.duration_per_pixel().get::<nanosecond>(),
        progress,
    );
    TimelineViewport::new(
        TimelineTimeNs::new(f64_to_i64_saturating(origin)),
        Time::new::<nanosecond>(duration_per_pixel),
    )
}

fn ease_in_out(progress: f64) -> f64 {
    let progress = progress.clamp(0.0, 1.0);
    if progress < 0.5 {
        4.0 * progress * progress * progress
    } else {
        1.0 - (-2.0 * progress + 2.0).powi(3) / 2.0
    }
}

fn lerp_f64(start: f64, target: f64, progress: f64) -> f64 {
    start + ((target - start) * progress)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "timeline playground zoom animation interpolates nanosecond ranges through display-scale f64 progress"
)]
fn interpolate_timeline_playground_range(
    animation: TimelinePlaygroundZoomAnimation,
    progress: f64,
) -> (i64, i64) {
    let visible_start_ns = lerp_f64(
        animation.start_visible_start_ns as f64,
        animation.target_visible_start_ns as f64,
        progress,
    );
    let visible_end_ns = lerp_f64(
        animation.start_visible_end_ns as f64,
        animation.target_visible_end_ns as f64,
        progress,
    );
    (
        f64_to_i64_saturating(visible_start_ns),
        f64_to_i64_saturating(visible_end_ns)
            .max(f64_to_i64_saturating(visible_start_ns).saturating_add(1)),
    )
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "timeline zoom animation rounds the interpolated viewport origin back to integer nanoseconds before storing it"
)]
fn f64_to_i64_saturating(value: f64) -> i64 {
    const I64_MIN_F64: f64 = -9_223_372_036_854_775_808.0;
    const I64_MAX_F64: f64 = 9_223_372_036_854_775_807.0;

    if !value.is_finite() {
        return 0;
    }
    value.round().clamp(I64_MIN_F64, I64_MAX_F64) as i64
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "row position animation rounds interpolated pixel offsets back to integer client pixels"
)]
fn f64_to_i32_saturating(value: f64) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    value
        .round()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

fn window_chrome_button_rect(layout: TerminalLayout, button: WindowChromeButton) -> ClientRect {
    match button {
        WindowChromeButton::Pin => layout.pin_button_rect(),
        WindowChromeButton::Diagnostics => layout.diagnostics_button_rect(),
        WindowChromeButton::Minimize => layout.minimize_button_rect(),
        WindowChromeButton::MaximizeRestore => layout.maximize_restore_button_rect(),
        WindowChromeButton::Close => layout.close_button_rect(),
    }
}

fn window_chrome_button_at_point(
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<WindowChromeButton> {
    [
        WindowChromeButton::Pin,
        WindowChromeButton::Diagnostics,
        WindowChromeButton::Minimize,
        WindowChromeButton::MaximizeRestore,
        WindowChromeButton::Close,
    ]
    .into_iter()
    .find(|button| window_chrome_button_rect(layout, *button).contains(point))
}

fn terminal_toggle_diagnostics_panel(state: &mut AppState, hwnd: WindowHandle) -> eyre::Result<()> {
    state.diagnostic_panel_visible = !state.diagnostic_panel_visible;
    state.diagnostics_button_last_clicked_at = Some(Instant::now());
    state.pending_diagnostic_selection = None;
    state.diagnostic_selection_drag_point = None;
    if !state.diagnostic_panel_visible {
        state.diagnostic_selection = None;
    }
    let layout = terminal_client_layout(hwnd, state)?;
    state.pending_terminal_resize = None;
    let _ = apply_terminal_resize(state, layout)?;
    Ok(())
}

fn scene_toggle_diagnostics_panel(state: &mut SceneAppState) {
    state.diagnostics_visible = !state.diagnostics_visible;
    state.diagnostics_button_last_clicked_at = Some(Instant::now());
    state.pending_diagnostic_selection = None;
    state.diagnostic_selection_drag_point = None;
    if !state.diagnostics_visible {
        state.diagnostic_selection = None;
    }
}

fn terminal_toggle_pin(state: &mut AppState, hwnd: WindowHandle) -> eyre::Result<()> {
    // windowing[impl chrome.pin-button]
    let pinned = !state.pinned_topmost;
    hwnd.set_topmost(pinned)?;
    state.pinned_topmost = pinned;
    state.pin_button_last_clicked_at = Some(Instant::now());
    Ok(())
}

fn scene_toggle_pin(state: &mut SceneAppState, hwnd: WindowHandle) -> eyre::Result<()> {
    // windowing[impl chrome.pin-button]
    let pinned = !state.pinned_topmost;
    hwnd.set_topmost(pinned)?;
    state.pinned_topmost = pinned;
    state.pin_button_last_clicked_at = Some(Instant::now());
    Ok(())
}

fn execute_window_chrome_button(hwnd: WindowHandle, button: WindowChromeButton) {
    match button {
        WindowChromeButton::Pin | WindowChromeButton::Diagnostics => {}
        WindowChromeButton::Minimize => hwnd.minimize(),
        WindowChromeButton::MaximizeRestore => hwnd.toggle_maximize_restore(),
        WindowChromeButton::Close => hwnd.post_close(),
    }
}

fn terminal_drag_handle_contains(layout: TerminalLayout, point: ClientPoint) -> bool {
    layout.drag_handle_rect().contains(point)
        && window_chrome_button_at_point(layout, point).is_none()
}

fn scene_drag_handle_contains(layout: TerminalLayout, point: ClientPoint) -> bool {
    layout.drag_handle_rect().contains(point)
        && window_chrome_button_at_point(layout, point).is_none()
}

fn scene_interactive_region_contains(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: Option<ClientPoint>,
) -> bool {
    point.is_some_and(|point| {
        scene_drag_handle_contains(layout, point)
            || window_chrome_button_at_point(layout, point).is_some()
            || (state.diagnostics_visible && scene_diagnostic_text_rect(layout).contains(point))
            || (!state.diagnostics_visible
                && timeline_add_track_button_at_point(state, layout, point))
            || (!state.diagnostics_visible
                && timeline_viewport_control_at_point(state, layout, point).is_some())
            || (!state.diagnostics_visible
                && timeline_transport_play_button_at_point(state, layout, point))
            || (!state.diagnostics_visible
                && timeline_track_record_button_at_point(state, layout, point).is_some())
            || (!state.diagnostics_visible
                && timeline_audio_head_at_point(state, layout, point).is_some())
            || (!state.diagnostics_visible
                && timeline_selection_surface_at_point(state, layout, point))
            || (!state.diagnostics_visible
                && timeline_scroll_interaction_at_point(state, layout, point))
            || (state.scene_kind == SceneWindowKind::Logs
                && !state.diagnostics_visible
                && windows_scene::logs_control_at_point(layout, point).is_some())
            || (!state.diagnostics_visible
                && scene_pretty_text_target(state, layout)
                    .is_some_and(|target| target.rect.contains(point)))
            || (!state.diagnostics_visible
                && scene_action_at_point(state.scene_kind, layout, point).is_some())
            || cursor_gallery_cell_at_point(state, layout, point).is_some()
            || demo_mode_button_at_point(state, layout, point)
            || demo_mode_scramble_toggle_at_point(state, layout, point)
            || (!state.diagnostics_visible
                && legacy_recording_devices_button_at_point(state, layout, point))
            || (!state.diagnostics_visible
                && audio_input_device_at_point(state, layout, point).is_some())
            || (!state.diagnostics_visible
                && audio_daemon_model_at_point(state, layout, point).is_some())
            || (!state.diagnostics_visible
                && audio_daemon_cuda_check_button_at_point(state, layout, point))
            || (!state.diagnostics_visible
                && audio_input_device_arm_button_at_point(state, layout, point))
            || (!state.diagnostics_visible
                && audio_input_device_transcription_button_at_point(state, layout, point))
            || (!state.diagnostics_visible
                && audio_input_device_transcription_flush_button_at_point(state, layout, point))
            || (!state.diagnostics_visible
                && audio_input_device_play_pause_button_at_point(state, layout, point))
            || (!state.diagnostics_visible
                && audio_input_device_loopback_button_at_point(state, layout, point))
            || (!state.diagnostics_visible
                && audio_input_timeline_head_at_point(state, layout, point).is_some())
            || (!state.diagnostics_visible
                && audio_input_timeline_at_point(state, layout, point).is_some())
            || (!state.diagnostics_visible
                && audio_input_device_detail_legacy_recording_button_at_point(state, layout, point))
            || (state.scene_kind == SceneWindowKind::TimelineTranscriptionSettings
                && (timeline_transcription_settings_model_row_at_point(layout, point).is_some()
                    || timeline_transcription_settings_input_row_at_point(state, layout, point)
                        .is_some()
                    || timeline_transcription_settings_output_row_at_point(state, layout, point)
                        .is_some()
                    || timeline_transcription_settings_target_at_point(state, layout, point)
                        .is_some()
                    || timeline_transcription_settings_socket_at_point(layout, point).is_some()
                    || timeline_transcription_settings_add_text_track_button_at_point(
                        layout, point,
                    )))
    })
}

fn demo_mode_button_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> bool {
    state.scene_kind == SceneWindowKind::DemoMode
        && !state.diagnostics_visible
        && windows_scene::demo_mode_layout(layout.terminal_panel_rect().inset(30))
            .demo_button_bounds
            .contains(point)
}

fn demo_mode_scramble_toggle_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> bool {
    state.scene_kind == SceneWindowKind::DemoMode
        && !state.diagnostics_visible
        && windows_scene::demo_mode_layout(layout.terminal_panel_rect().inset(30))
            .scramble_toggle_bounds
            .contains(point)
}

fn legacy_recording_devices_button_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> bool {
    if state.scene_kind != SceneWindowKind::AudioInputDevicePicker {
        return false;
    }
    let body_rect = layout.terminal_panel_rect().inset(22);
    windows_scene::audio_input_legacy_recording_dialog_button_rect(body_rect).contains(point)
}

fn audio_input_device_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<usize> {
    if state.scene_kind != SceneWindowKind::AudioInputDevicePicker {
        return None;
    }
    let body_rect = layout.terminal_panel_rect().inset(22);
    (0..state.audio_input_picker.devices.len()).find(|index| {
        windows_scene::audio_input_device_row_layout(
            body_rect,
            *index,
            state.audio_input_picker.devices.len(),
        )
        .is_some_and(|row| row.row_rect.contains(point))
    })
}

fn audio_daemon_model_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<usize> {
    if state.scene_kind != SceneWindowKind::AudioDaemon || state.diagnostics_visible {
        return None;
    }
    let body_rect = layout.terminal_panel_rect().inset(24);
    let model_count = super::audio_transcription_available_model_names().len();
    (0..model_count).find(|index| {
        windows_scene::audio_daemon_model_button_rect(body_rect, *index, model_count)
            .contains(point)
    })
}

fn audio_daemon_cuda_check_button_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> bool {
    if state.scene_kind != SceneWindowKind::AudioDaemon || state.diagnostics_visible {
        return false;
    }
    let body_rect = layout.terminal_panel_rect().inset(24);
    windows_scene::audio_daemon_cuda_check_button_rect(body_rect).contains(point)
}

fn audio_input_device_arm_button_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> bool {
    if state.scene_kind != SceneWindowKind::AudioInputDeviceDetails
        || state.audio_input_device_window.is_none()
    {
        return false;
    }
    let body_rect = layout.terminal_panel_rect().inset(24);
    windows_scene::audio_input_device_detail_layout(body_rect)
        .arm_button_rect
        .contains(point)
}

fn audio_input_device_loopback_button_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> bool {
    if state.scene_kind != SceneWindowKind::AudioInputDeviceDetails
        || state.audio_input_device_window.is_none()
    {
        return false;
    }
    let body_rect = layout.terminal_panel_rect().inset(24);
    windows_scene::audio_input_device_detail_layout(body_rect)
        .loopback_button_rect
        .contains(point)
}

fn audio_input_device_transcription_button_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> bool {
    if state.scene_kind != SceneWindowKind::AudioInputDeviceDetails
        || state.audio_input_device_window.is_none()
    {
        return false;
    }
    let body_rect = layout.terminal_panel_rect().inset(24);
    windows_scene::audio_input_device_detail_layout(body_rect)
        .transcription_button_rect
        .contains(point)
}

fn audio_input_device_transcription_flush_button_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> bool {
    if state.scene_kind != SceneWindowKind::AudioInputDeviceDetails
        || state.audio_input_device_window.is_none()
    {
        return false;
    }
    let body_rect = layout.terminal_panel_rect().inset(24);
    windows_scene::audio_input_device_detail_layout(body_rect)
        .transcription_flush_button_rect
        .contains(point)
}

fn audio_input_device_play_pause_button_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> bool {
    if state.scene_kind != SceneWindowKind::AudioInputDeviceDetails
        || state.audio_input_device_window.is_none()
    {
        return false;
    }
    let body_rect = layout.terminal_panel_rect().inset(24);
    windows_scene::audio_input_device_detail_layout(body_rect)
        .play_pause_button_rect
        .contains(point)
}

fn audio_input_device_detail_legacy_recording_button_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> bool {
    if state.scene_kind != SceneWindowKind::AudioInputDeviceDetails
        || state.audio_input_device_window.is_none()
    {
        return false;
    }
    let body_rect = layout.terminal_panel_rect().inset(24);
    windows_scene::audio_input_device_detail_layout(body_rect)
        .legacy_recording_button_rect
        .contains(point)
}

fn audio_input_timeline_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<ClientRect> {
    if state.scene_kind != SceneWindowKind::AudioInputDeviceDetails
        || state.audio_input_device_window.is_none()
    {
        return None;
    }
    let body_rect = layout.terminal_panel_rect().inset(24);
    let waveform_rect = windows_scene::audio_input_device_detail_layout(body_rect).waveform_rect;
    waveform_rect.contains(point).then_some(waveform_rect)
}

fn audio_input_timeline_head_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<AudioInputTimelineHeadKind> {
    if state.scene_kind != SceneWindowKind::AudioInputDeviceDetails {
        return None;
    }
    let body_rect = layout.terminal_panel_rect().inset(24);
    let waveform_rect = windows_scene::audio_input_device_detail_layout(body_rect).waveform_rect;
    let device_window = state.audio_input_device_window.as_ref()?;
    windows_scene::audio_input_timeline_head_grabbers(waveform_rect, device_window)
        .into_iter()
        .find(|grabber| grabber.rect.contains(point))
        .map(|grabber| grabber.kind)
}

fn open_legacy_recording_devices_from_scene(hwnd: WindowHandle) {
    if let Err(error) = open_legacy_recording_devices_dialog() {
        error!(
            ?error,
            "failed to open Windows legacy recording devices dialog"
        );
        hwnd.post_close();
    }
}

fn show_audio_daemon_cuda_check_result() {
    // audio[impl transcription.cuda-check]
    let description = match super::audio_transcription_run_python_cuda_check() {
        Ok(report) => report,
        Err(error) => format!("CUDA check failed to run: {error}"),
    };
    let _ = MessageDialog::new()
        .set_level(MessageLevel::Info)
        .set_title("CUDA Check")
        .set_description(&description)
        .set_buttons(MessageButtons::Ok)
        .show();
}

fn open_audio_input_device_window_from_scene(
    hwnd: WindowHandle,
    device: Option<AudioInputDeviceSummary>,
) {
    let Some(device) = device else {
        return;
    };
    let result = with_scene_app_state(|state| {
        let app_home = state.app_home.clone();
        let vt_engine = state.vt_engine;
        thread::Builder::new()
            .name("teamy-studio-audio-input-device".to_owned())
            .spawn_with_current_span(move || {
                let device_window = AudioInputDeviceWindowState::new(device);
                if let Err(error) = run_scene_window(
                    &app_home,
                    SceneWindowKind::AudioInputDeviceDetails,
                    vt_engine,
                    SceneWindowInitialization {
                        audio_input_device_window: Some(device_window),
                        ..Default::default()
                    },
                ) {
                    error!(?error, "failed to open selected audio input device window");
                }
            })
            .wrap_err("failed to spawn Teamy Studio selected audio input device thread")?;
        Ok(())
    });
    if let Err(error) = result {
        error!(
            ?error,
            "failed to launch selected audio input device window"
        );
        hwnd.post_close();
    }
}

fn commit_audio_input_picker_selection_from_scene(
    hwnd: WindowHandle,
    device: Option<AudioInputDeviceSummary>,
) {
    let Some(device) = device else {
        return;
    };

    let completion = with_scene_app_state(|state| Ok(state.audio_input_picker_completion));
    match completion {
        Ok(AudioInputPickerCompletion::OpenDeviceWindow) => {
            open_audio_input_device_window_from_scene(hwnd, Some(device));
        }
        Ok(AudioInputPickerCompletion::TimelineMicrophoneTrack) => {
            let result = with_scene_app_state(|state| {
                let document = ensure_timeline_document_handle(state);
                let _ = document
                    .append_microphone_track_for_device_id(device.name.clone(), device.id.clone());
                state.audio_input_device_window = Some(AudioInputDeviceWindowState::new(device));
                state.audio_input_picker_completion = AudioInputPickerCompletion::OpenDeviceWindow;
                state.scene_kind = SceneWindowKind::Timeline;
                state.scene_action_selected_index = 0;
                state.scene_virtual_cursor = None;
                render_scene_window_frame(state, hwnd, None, false)
            });
            if let Err(error) = result {
                error!(
                    ?error,
                    "failed to commit selected microphone into the timeline"
                );
                hwnd.post_close();
            } else {
                broadcast_timeline_document_changed();
            }
        }
        Ok(AudioInputPickerCompletion::TimelineTranscriptionInputTrack) => {
            let result = with_scene_app_state(|state| {
                let settings_track_id = state
                    .timeline_transcription_settings
                    .map(|settings| settings.track_id);
                let document = ensure_timeline_document_handle(state);
                let selected_target_index = {
                    let track_id = document.append_microphone_track_for_device_id(
                        device.name.clone(),
                        device.id.clone(),
                    );
                    document
                        .tracks()
                        .iter()
                        .filter(|track| track.kind() == crate::timeline::TimelineTrackKind::Audio)
                        .map(crate::timeline::TimelineTrack::id)
                        .position(|candidate| candidate == track_id)
                        .map_or(0, |index| index + 1)
                };
                if let Some(settings_track_id) = settings_track_id {
                    let audio_track_id = document
                        .tracks()
                        .iter()
                        .filter(|track| track.kind() == crate::timeline::TimelineTrackKind::Audio)
                        .map(crate::timeline::TimelineTrack::id)
                        .nth(selected_target_index.saturating_sub(1));
                    let _ = document.set_transcription_track_target_audio_track(
                        settings_track_id,
                        audio_track_id,
                    );
                    let _ = send_timeline_document_command(
                        state,
                        TimelineDocumentCommand::AppendMicrophoneTrackForTranscriptionSettings {
                            track_id: settings_track_id,
                            device_name: device.name.clone(),
                            device_id: device.id.clone(),
                        },
                    );
                }
                state.audio_input_device_window = Some(AudioInputDeviceWindowState::new(device));
                state.audio_input_picker_completion = AudioInputPickerCompletion::OpenDeviceWindow;
                state.scene_kind = SceneWindowKind::TimelineTranscriptionSettings;
                select_timeline_transcription_settings_input_audio_track(
                    state,
                    selected_target_index,
                    true,
                );
                render_scene_window_frame(state, hwnd, None, false)
            });
            if let Err(error) = result {
                error!(
                    ?error,
                    "failed to commit selected microphone into transcription settings"
                );
                hwnd.post_close();
            } else {
                broadcast_timeline_document_changed();
            }
        }
        Err(error) => {
            error!(?error, "failed to read audio picker completion state");
            hwnd.post_close();
        }
    }
}

fn audio_input_picker_key_from_virtual_key(virtual_key: u32) -> Option<AudioInputPickerKey> {
    if virtual_key == u32::from(VK_UP.0) {
        return Some(AudioInputPickerKey::Up);
    }
    if virtual_key == u32::from(VK_DOWN.0) {
        return Some(AudioInputPickerKey::Down);
    }
    if virtual_key == u32::from(VK_TAB.0) {
        return Some(AudioInputPickerKey::Tab);
    }
    if virtual_key == u32::from(VK_RETURN.0) {
        return Some(AudioInputPickerKey::Enter);
    }
    if virtual_key == u32::from(b'R') {
        return Some(AudioInputPickerKey::LegacyRecordingDevices);
    }
    if virtual_key == u32::from(VK_ESCAPE.0) {
        return Some(AudioInputPickerKey::Escape);
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LauncherMenuNavigation {
    Spatial(SpatialNavigationDirection),
    Sequential(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpatialNavigationDirection {
    Left,
    Right,
    Up,
    Down,
}

fn launcher_menu_navigation_from_virtual_key(
    virtual_key: u32,
    shift_pressed: bool,
) -> Option<LauncherMenuNavigation> {
    if virtual_key == u32::from(VK_LEFT.0) || virtual_key == u32::from(VK_UP.0) {
        return Some(LauncherMenuNavigation::Spatial(
            if virtual_key == u32::from(VK_LEFT.0) {
                SpatialNavigationDirection::Left
            } else {
                SpatialNavigationDirection::Up
            },
        ));
    }
    if virtual_key == u32::from(VK_RIGHT.0) || virtual_key == u32::from(VK_DOWN.0) {
        return Some(LauncherMenuNavigation::Spatial(
            if virtual_key == u32::from(VK_RIGHT.0) {
                SpatialNavigationDirection::Right
            } else {
                SpatialNavigationDirection::Down
            },
        ));
    }
    if virtual_key == u32::from(VK_TAB.0) {
        return Some(LauncherMenuNavigation::Sequential(if shift_pressed {
            -1
        } else {
            1
        }));
    }
    None
}

fn next_scene_action_target(
    rects: &[ClientRect],
    current_index: usize,
    virtual_cursor: Option<ClientPoint>,
    navigation: LauncherMenuNavigation,
) -> (usize, ClientPoint) {
    if rects.is_empty() {
        return (0, ClientPoint::new(0, 0));
    }
    let current_index = current_index.min(rects.len().saturating_sub(1));
    let origin = virtual_cursor.unwrap_or_else(|| rect_center(rects[current_index]));
    let selected_index = match navigation {
        LauncherMenuNavigation::Sequential(direction) => {
            next_sequential_index(rects.len(), current_index, direction)
        }
        LauncherMenuNavigation::Spatial(direction) => {
            next_spatial_index(rects, current_index, origin, direction).unwrap_or(current_index)
        }
    };
    (selected_index, rect_center(rects[selected_index]))
}

fn scene_action_navigation_rects(state: &SceneAppState, layout: TerminalLayout) -> Vec<ClientRect> {
    if state.scene_kind == SceneWindowKind::Launcher && state.diagnostics_visible {
        let rects = windows_scene::launcher_diagnostic_action_hit_rects(
            layout,
            state.diagnostic_cell_width,
            state.diagnostic_cell_height,
        );
        if !rects.is_empty() {
            return rects;
        }
    }

    scene_action_hit_rects(
        state.scene_kind,
        layout,
        scaled_scene_button_size(state.dpi),
    )
}

fn cursor_gallery_navigation_rects(layout: TerminalLayout) -> Vec<ClientRect> {
    windows_scene::cursor_gallery_cell_layouts(layout)
        .into_iter()
        .map(windows_scene::CursorGalleryCellLayout::hit_rect)
        .collect()
}

fn demo_mode_navigation_rects(layout: TerminalLayout) -> Vec<ClientRect> {
    let demo_layout = windows_scene::demo_mode_layout(layout.terminal_panel_rect().inset(30));
    vec![
        demo_layout.demo_button_bounds,
        demo_layout.scramble_toggle_bounds,
    ]
}

fn cursor_gallery_cell_at_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<windows_scene::CursorGalleryCellLayout> {
    if state.scene_kind != SceneWindowKind::CursorGallery || state.diagnostics_visible {
        return None;
    }

    windows_scene::cursor_gallery_cell_layouts(layout)
        .into_iter()
        .find(|cell| cell.hit_rect().contains(point))
}

fn scene_action_hit_rects(
    scene_kind: SceneWindowKind,
    layout: TerminalLayout,
    max_button_size: i32,
) -> Vec<ClientRect> {
    let specs = windows_scene::scene_button_specs(scene_kind);
    windows_scene::layout_scene_buttons(layout.terminal_panel_rect(), specs.len(), max_button_size)
        .into_iter()
        .map(windows_scene::SceneButtonLayout::hit_rect)
        .collect()
}

fn next_sequential_index(action_count: usize, current_index: usize, direction: i32) -> usize {
    if action_count == 0 {
        return 0;
    }
    match direction.cmp(&0) {
        std::cmp::Ordering::Less => current_index
            .checked_sub(1)
            .unwrap_or(action_count.saturating_sub(1)),
        std::cmp::Ordering::Equal => current_index.min(action_count.saturating_sub(1)),
        std::cmp::Ordering::Greater => (current_index + 1) % action_count,
    }
}

fn next_spatial_index(
    rects: &[ClientRect],
    current_index: usize,
    origin: ClientPoint,
    direction: SpatialNavigationDirection,
) -> Option<usize> {
    let origin = origin.to_win32_point().ok()?;
    rects
        .iter()
        .enumerate()
        .filter(|(index, rect)| {
            *index != current_index && rect_is_in_direction(**rect, origin, direction)
        })
        .map(|(index, rect)| (spatial_navigation_score(*rect, origin, direction), index))
        .min_by_key(|(score, index)| (*score, *index))
        .map(|(_, index)| index)
}

fn rect_is_in_direction(
    rect: ClientRect,
    origin: windows::Win32::Foundation::POINT,
    direction: SpatialNavigationDirection,
) -> bool {
    let center = rect_center_point(rect);
    match direction {
        SpatialNavigationDirection::Left => center.x < origin.x,
        SpatialNavigationDirection::Right => center.x > origin.x,
        SpatialNavigationDirection::Up => center.y < origin.y,
        SpatialNavigationDirection::Down => center.y > origin.y,
    }
}

fn spatial_navigation_score(
    rect: ClientRect,
    origin: windows::Win32::Foundation::POINT,
    direction: SpatialNavigationDirection,
) -> i64 {
    let closest = closest_point_on_rect(rect, origin);
    let center = rect_center_point(rect);
    let (primary, cross) = match direction {
        SpatialNavigationDirection::Left | SpatialNavigationDirection::Right => (
            (closest.x - origin.x).abs(),
            distance_to_range(origin.y, rect.top(), rect.bottom()),
        ),
        SpatialNavigationDirection::Up | SpatialNavigationDirection::Down => (
            (closest.y - origin.y).abs(),
            distance_to_range(origin.x, rect.left(), rect.right()),
        ),
    };
    let center_distance = (center.x - origin.x).abs() + (center.y - origin.y).abs();
    let primary = i64::from(primary.max(1));
    let cross = i64::from(cross);
    let center_distance = i64::from(center_distance);
    (primary * primary * 4) + (cross * cross * 9) + center_distance
}

fn closest_point_on_rect(
    rect: ClientRect,
    point: windows::Win32::Foundation::POINT,
) -> windows::Win32::Foundation::POINT {
    windows::Win32::Foundation::POINT {
        x: point.x.clamp(rect.left(), rect.right()),
        y: point.y.clamp(rect.top(), rect.bottom()),
    }
}

fn distance_to_range(value: i32, start: i32, end: i32) -> i32 {
    if value < start {
        start - value
    } else if value > end {
        value - end
    } else {
        0
    }
}

fn rect_center(rect: ClientRect) -> ClientPoint {
    let center = rect_center_point(rect);
    ClientPoint::new(center.x, center.y)
}

fn rect_center_point(rect: ClientRect) -> windows::Win32::Foundation::POINT {
    windows::Win32::Foundation::POINT {
        x: rect.left() + (rect.width() / 2),
        y: rect.top() + (rect.height() / 2),
    }
}

fn selected_scene_action(state: &SceneAppState) -> Option<SceneAction> {
    windows_scene::scene_button_specs(state.scene_kind)
        .get(state.scene_action_selected_index)
        .map(|spec| spec.action)
}

fn invoke_scene_action(
    hwnd: WindowHandle,
    action: SceneAction,
) -> eyre::Result<SceneActionDisposition> {
    if action == SceneAction::CreateBlankTimeline {
        return with_scene_app_state(|state| {
            // timeline[impl start-window.new-blank]
            // timeline[impl document.window-state]
            state.scene_kind = SceneWindowKind::Timeline;
            replace_timeline_document(state, TimelineDocument::blank());
            state.scene_action_selected_index = 0;
            state.scene_virtual_cursor = None;
            render_scene_window_frame(state, hwnd, None, false)?;
            Ok(SceneActionDisposition::KeepOpen)
        });
    }

    let (app_home, vt_engine) =
        with_scene_app_state(|state| Ok((state.app_home.clone(), state.vt_engine)))?;
    let disposition = perform_scene_action(&app_home, vt_engine, action)?;
    with_scene_app_state(|state| render_scene_window_frame(state, hwnd, None, false))?;
    Ok(disposition)
}

#[expect(
    clippy::too_many_lines,
    reason = "scene action dispatch is intentionally centralized for small launcher actions"
)]
fn perform_scene_action(
    app_home: &AppHome,
    vt_engine: VtEngineChoice,
    action: SceneAction,
) -> eyre::Result<SceneActionDisposition> {
    match action {
        SceneAction::OpenTerminal => {
            let app_home = app_home.clone();
            thread::Builder::new()
                .name("teamy-studio-launcher-terminal".to_owned())
                .spawn_with_current_span(move || {
                    if let Err(error) =
                        super::open_terminal_window(&app_home, None, None, None, vt_engine)
                    {
                        error!(?error, "failed to open Teamy Studio terminal window");
                    }
                })
                .wrap_err("failed to spawn Teamy Studio terminal window thread")?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::OpenCursorInfo => {
            let app_home = app_home.clone();
            thread::Builder::new()
                .name("teamy-studio-launcher-cursor-info".to_owned())
                .spawn_with_current_span(move || {
                    let _ = (app_home, vt_engine);
                    let terminal =
                        match HostedTerminalSession::new_cursor_info_virtual(CursorInfoConfig {
                            initial_mode: super::CursorInfoRenderMode::Overlay,
                            scale: 10,
                            pixel_size: super::CursorInfoPixelSize::HalfHeight,
                        }) {
                            Ok(terminal) => terminal,
                            Err(error) => {
                                error!(
                                    ?error,
                                    "failed to create Teamy Studio virtual cursor-info session"
                                );
                                return;
                            }
                        };
                    if let Err(error) =
                        run_with_terminal_session(terminal, 0, None, Some("Cursor Info"))
                    {
                        error!(?error, "failed to open Teamy Studio cursor-info window");
                    }
                })
                .wrap_err("failed to spawn Teamy Studio cursor-info window thread")?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::OpenCursorGallery => {
            let app_home = app_home.clone();
            thread::Builder::new()
                .name("teamy-studio-cursor-gallery".to_owned())
                .spawn_with_current_span(move || {
                    if let Err(error) = run_scene_window(
                        &app_home,
                        SceneWindowKind::CursorGallery,
                        vt_engine,
                        SceneWindowInitialization::default(),
                    ) {
                        error!(?error, "failed to open cursor gallery window");
                    }
                })
                .wrap_err("failed to spawn Teamy Studio cursor gallery thread")?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::OpenDemoMode => {
            let app_home = app_home.clone();
            thread::Builder::new()
                .name("teamy-studio-demo-mode".to_owned())
                .spawn_with_current_span(move || {
                    if let Err(error) = run_scene_window(
                        &app_home,
                        SceneWindowKind::DemoMode,
                        vt_engine,
                        SceneWindowInitialization::default(),
                    ) {
                        error!(?error, "failed to open demo mode window");
                    }
                })
                .wrap_err("failed to spawn Teamy Studio demo mode thread")?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::OpenTimelinePlayground => {
            // timeline[impl playground.launcher-button]
            let app_home = app_home.clone();
            thread::Builder::new()
                .name("teamy-studio-timeline-playground".to_owned())
                .spawn_with_current_span(move || {
                    if let Err(error) = run_scene_window(
                        &app_home,
                        SceneWindowKind::TimelinePlayground,
                        vt_engine,
                        SceneWindowInitialization::default(),
                    ) {
                        error!(?error, "failed to open timeline playground window");
                    }
                })
                .wrap_err("failed to spawn Teamy Studio timeline playground thread")?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::OpenStorage => {
            // windowing[impl launcher.buttons.storage-placeholder]
            let _ = MessageDialog::new()
                .set_level(MessageLevel::Info)
                .set_title("Storage")
                .set_description("Storage is not implemented yet.")
                .set_buttons(MessageButtons::Ok)
                .show();
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::OpenEnvironmentVariables => {
            // windowing[impl launcher.buttons.environment-variables-placeholder]
            let _ = MessageDialog::new()
                .set_level(MessageLevel::Info)
                .set_title("Environment Variables")
                .set_description("The environment-variable inspector is not implemented yet.")
                .set_buttons(MessageButtons::Ok)
                .show();
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::OpenApplicationWindows => {
            // windowing[impl launcher.buttons.application-windows-placeholder]
            let _ = MessageDialog::new()
                .set_level(MessageLevel::Info)
                .set_title("Application Windows")
                .set_description("The application-window inspector is not implemented yet.")
                .set_buttons(MessageButtons::Ok)
                .show();
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::OpenAudioPicker => {
            let app_home = app_home.clone();
            thread::Builder::new()
                .name("teamy-studio-audio-picker".to_owned())
                .spawn_with_current_span(move || {
                    if let Err(error) = run_scene_window(
                        &app_home,
                        SceneWindowKind::AudioPicker,
                        vt_engine,
                        SceneWindowInitialization::default(),
                    ) {
                        error!(?error, "failed to open audio picker window");
                    }
                })
                .wrap_err("failed to spawn Teamy Studio audio picker thread")?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::OpenAudioDaemon => {
            // audio[impl gui.daemon-window]
            let app_home = app_home.clone();
            thread::Builder::new()
                .name("teamy-studio-audio-daemon".to_owned())
                .spawn_with_current_span(move || {
                    if let Err(error) = run_scene_window(
                        &app_home,
                        SceneWindowKind::AudioDaemon,
                        vt_engine,
                        SceneWindowInitialization::default(),
                    ) {
                        error!(?error, "failed to open audio daemon window");
                    }
                })
                .wrap_err("failed to spawn Teamy Studio audio daemon thread")?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::OpenJobs => {
            let app_home = app_home.clone();
            thread::Builder::new()
                .name("teamy-studio-jobs".to_owned())
                .spawn_with_current_span(move || {
                    if let Err(error) = run_scene_window(
                        &app_home,
                        SceneWindowKind::Jobs,
                        vt_engine,
                        SceneWindowInitialization::default(),
                    ) {
                        error!(?error, "failed to open jobs window");
                    }
                })
                .wrap_err("failed to spawn Teamy Studio jobs thread")?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::OpenLogs => {
            // observability[impl logs.launcher-button]
            let app_home = app_home.clone();
            thread::Builder::new()
                .name("teamy-studio-logs".to_owned())
                .spawn_with_current_span(move || {
                    if let Err(error) = run_scene_window(
                        &app_home,
                        SceneWindowKind::Logs,
                        vt_engine,
                        SceneWindowInitialization::default(),
                    ) {
                        error!(?error, "failed to open logs window");
                    }
                })
                .wrap_err("failed to spawn Teamy Studio logs thread")?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::OpenAudioInputDevices => {
            // audio[impl gui.picker-window]
            let app_home = app_home.clone();
            thread::Builder::new()
                .name("teamy-studio-audio-input-devices".to_owned())
                .spawn_with_current_span(move || {
                    if let Err(error) = run_scene_window(
                        &app_home,
                        SceneWindowKind::AudioInputDevicePicker,
                        vt_engine,
                        SceneWindowInitialization::default(),
                    ) {
                        error!(?error, "failed to open audio input device picker window");
                    }
                })
                .wrap_err("failed to spawn Teamy Studio audio input device picker thread")?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::OpenTimeline => {
            // timeline[impl launcher.button]
            let app_home = app_home.clone();
            thread::Builder::new()
                .name("teamy-studio-timeline".to_owned())
                .spawn_with_current_span(move || {
                    if let Err(error) = run_scene_window(
                        &app_home,
                        SceneWindowKind::TimelineStart,
                        vt_engine,
                        SceneWindowInitialization::default(),
                    ) {
                        error!(?error, "failed to open timeline window");
                    }
                })
                .wrap_err("failed to spawn Teamy Studio timeline thread")?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::OpenTimelineTrackMenu => {
            with_scene_app_state(|state| {
                // timeline[impl add-track.workflow]
                state.scene_kind = SceneWindowKind::TimelineAddTrack;
                state.scene_action_selected_index = 0;
                state.scene_virtual_cursor = None;
                Ok(())
            })?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::CreateBlankTimeline => Ok(SceneActionDisposition::KeepOpen),
        SceneAction::PanTimelineLeft => {
            with_scene_app_state(|state| {
                if state.scene_kind == SceneWindowKind::TimelinePlayground {
                    if let Some(playground) = state.timeline_playground.as_mut() {
                        playground.pan(-1);
                    }
                } else if let Some(document) = timeline_document_handle_mut(state) {
                    document.pan_viewport_left();
                }
                Ok(())
            })?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::PanTimelineRight => {
            with_scene_app_state(|state| {
                if state.scene_kind == SceneWindowKind::TimelinePlayground {
                    if let Some(playground) = state.timeline_playground.as_mut() {
                        playground.pan(1);
                    }
                } else if let Some(document) = timeline_document_handle_mut(state) {
                    document.pan_viewport_right();
                }
                Ok(())
            })?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::ZoomTimelineIn => {
            with_scene_app_state(|state| {
                if state.scene_kind == SceneWindowKind::TimelinePlayground {
                    if let Some(playground) = state.timeline_playground.as_mut() {
                        playground.zoom(1, 2);
                    }
                } else if let Some(document) = timeline_document_handle_mut(state) {
                    document.zoom_viewport_in();
                }
                Ok(())
            })?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::ZoomTimelineOut => {
            with_scene_app_state(|state| {
                if state.scene_kind == SceneWindowKind::TimelinePlayground {
                    if let Some(playground) = state.timeline_playground.as_mut() {
                        playground.zoom(2, 1);
                    }
                } else if let Some(document) = timeline_document_handle_mut(state) {
                    document.zoom_viewport_out();
                }
                Ok(())
            })?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::FitTimelinePlaygroundToContent => {
            // timeline[impl playground.fit-content]
            with_scene_app_state(|state| {
                if let Some(playground) = state.timeline_playground.as_mut() {
                    playground.fit_to_content();
                }
                Ok(())
            })?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::RegenerateTimelinePlayground => {
            // timeline[impl playground.query-controls]
            with_scene_app_state(|state| {
                if let Some(playground) = state.timeline_playground.as_mut() {
                    playground.regenerate()?;
                }
                Ok(())
            })?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::ToggleTimelinePlaygroundLiveEvents => {
            // timeline[impl playground.live-tracing-events]
            with_scene_app_state(|state| {
                if let Some(playground) = state.timeline_playground.as_mut() {
                    playground.toggle_live_tracing_events();
                }
                Ok(())
            })?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::TimelinePlaygroundGroupingGroupKey => {
            set_timeline_playground_grouping(TimelineGroupingMode::GroupKey)?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::TimelinePlaygroundGroupingSourceKey => {
            set_timeline_playground_grouping(TimelineGroupingMode::SourceKey)?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::TimelinePlaygroundGroupingLabel => {
            set_timeline_playground_grouping(TimelineGroupingMode::Label)?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::TimelinePlaygroundGroupingAll => {
            set_timeline_playground_grouping(TimelineGroupingMode::All)?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::IncreaseTimelinePlaygroundFolding => {
            // timeline[impl playground.query-controls]
            with_scene_app_state(|state| {
                if let Some(playground) = state.timeline_playground.as_mut() {
                    playground.minimum_visible_pixels =
                        playground.minimum_visible_pixels.saturating_add(1).min(32);
                }
                Ok(())
            })?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::DecreaseTimelinePlaygroundFolding => {
            // timeline[impl playground.query-controls]
            with_scene_app_state(|state| {
                if let Some(playground) = state.timeline_playground.as_mut() {
                    playground.minimum_visible_pixels =
                        playground.minimum_visible_pixels.saturating_sub(1).max(1);
                }
                Ok(())
            })?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::SelectTimelineTool => {
            with_scene_app_state(|state| {
                state.timeline_tool = TimelineInteractionTool::Select;
                Ok(())
            })?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::SelectTimelineBrush => {
            with_scene_app_state(|state| {
                state.timeline_tool = TimelineInteractionTool::Brush;
                Ok(())
            })?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::AppendMicrophoneTrack => {
            with_scene_app_state(|state| {
                // timeline[impl add-track.microphone-live-device]
                let mut picker = AudioInputPickerState::new(
                    list_active_audio_input_devices().unwrap_or_default(),
                );
                if let Some(default_index) =
                    picker.devices.iter().position(|device| device.is_default)
                {
                    picker.select_index(default_index);
                }
                state.audio_input_picker = picker;
                state.audio_input_picker_completion =
                    AudioInputPickerCompletion::TimelineMicrophoneTrack;
                state.scene_kind = SceneWindowKind::AudioInputDevicePicker;
                state.scene_action_selected_index = 0;
                state.scene_virtual_cursor = None;
                Ok(())
            })?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::AppendTranscriptionTrack => {
            let mut new_track_index = None;
            with_scene_app_state(|state| {
                let document = ensure_timeline_document_handle(state);
                let track_id = document.append_transcription_track();
                new_track_index = document
                    .tracks()
                    .iter()
                    .position(|track| track.id() == track_id);
                state.scene_kind = SceneWindowKind::Timeline;
                state.scene_action_selected_index = 0;
                state.scene_virtual_cursor = None;
                Ok(())
            })?;
            broadcast_timeline_document_changed();
            if let Some(track_index) = new_track_index {
                open_timeline_transcription_settings_window_from_scene(track_index)?;
            }
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::AppendTextTrack => {
            with_scene_app_state(|state| {
                let document = ensure_timeline_document_handle(state);
                let _ = document.append_text_track();
                state.scene_kind = SceneWindowKind::Timeline;
                state.scene_action_selected_index = 0;
                state.scene_virtual_cursor = None;
                Ok(())
            })?;
            broadcast_timeline_document_changed();
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::CloseTimelineTrackMenu => {
            with_scene_app_state(|state| {
                // timeline[impl add-track.workflow]
                state.scene_kind = SceneWindowKind::Timeline;
                state.scene_action_selected_index = 0;
                state.scene_virtual_cursor = None;
                Ok(())
            })?;
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::ImportTimeline => {
            // timeline[impl import.tracy.file-picker]
            let Some(path) = FileDialog::new()
                .add_filter("Tracy Capture", &["tracy"])
                .set_title("Import Tracy Capture")
                .pick_file()
            else {
                return Ok(SceneActionDisposition::KeepOpen);
            };

            match TimelineDocument::import_tracy_capture(&path) {
                Ok(document) => {
                    with_scene_app_state(|state| {
                        // timeline[impl add-track.tracy]
                        if state.timeline_document.is_some()
                            && matches!(
                                state.scene_kind,
                                SceneWindowKind::Timeline | SceneWindowKind::TimelineAddTrack
                            )
                        {
                            if let Some(existing_document) = timeline_document_handle_mut(state) {
                                existing_document.append_tracy_capture_track(&path)?;
                            }
                        } else {
                            replace_timeline_document(state, document);
                        }
                        state.scene_kind = SceneWindowKind::Timeline;
                        state.scene_action_selected_index = 0;
                        state.scene_virtual_cursor = None;
                        Ok(())
                    })?;
                    broadcast_timeline_document_changed();
                }
                Err(error) => {
                    error!(?error, path = %path.display(), "failed to import Tracy capture");
                    let _ = MessageDialog::new()
                        .set_level(MessageLevel::Error)
                        .set_title("Import Timeline")
                        .set_description(format!("Failed to import Tracy capture:\n{error}"))
                        .set_buttons(MessageButtons::Ok)
                        .show();
                }
            }
            Ok(SceneActionDisposition::KeepOpen)
        }
        SceneAction::SelectWindowsBell => {
            // windowing[impl audio-picker.selection.persisted]
            // windowing[impl audio-picker.selection.preview]
            set_bell_source(app_home, BellSource::Windows)?;
            ring_terminal_bell();
            Ok(SceneActionDisposition::CloseWindow)
        }
        SceneAction::SelectFileBell => {
            let Some(path) = FileDialog::new()
                .add_filter("Wave Audio", &["wav"])
                .set_title("Pick Bell File")
                .pick_file()
            else {
                return Ok(SceneActionDisposition::KeepOpen);
            };

            // windowing[impl audio-picker.selection.persisted]
            // windowing[impl audio-picker.selection.preview]
            set_bell_source(app_home, BellSource::File(path))?;
            ring_terminal_bell();
            Ok(SceneActionDisposition::CloseWindow)
        }
    }
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
    // behavior[impl window.appearance.terminal.scrollbar.stateful]
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
    // behavior[impl window.appearance.terminal.scrollbar.shader]
    if scrollbar_rect.width() <= 0
        || scrollbar_rect.height() <= 0
        || scrollbar.total == 0
        || scrollbar.visible == 0
    {
        return None;
    }

    let track_height = u64::try_from(scrollbar_rect.height().max(1)).ok()?;
    let track_height_i32 = scrollbar_rect.height().max(1);
    let min_thumb_height = scrollbar_rect.width().max(22).min(track_height_i32);
    let proportional_thumb = (track_height.saturating_mul(scrollbar.visible) / scrollbar.total)
        .max(u64::try_from(min_thumb_height).ok()?);
    let thumb_height = i32::try_from(proportional_thumb.min(track_height)).ok()?;
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
    // behavior[impl window.interaction.scrollback.scrollbar-track-grab]
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
    // behavior[impl window.interaction.scrollback.scrollbar-drag]
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
    // behavior[impl window.appearance.terminal.cursor.legible-block]
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

fn build_diagnostic_panel_text(
    state: &mut AppState,
    layout: TerminalLayout,
) -> eyre::Result<String> {
    let (cols, rows) = effective_terminal_grid_size(layout);
    let viewport = state.terminal.viewport_metrics()?;
    let display = state.terminal.cached_display_state();
    let caret = display.cursor.map_or_else(
        || {
            format!(
                "caret offscreen | viewport {}+{} / {}",
                viewport.offset, viewport.visible, viewport.total
            )
        },
        |cursor| {
            let screen_row =
                i64::from(cursor.cell.row()) + i64::try_from(viewport.offset).unwrap_or(i64::MAX);
            let screen_col = i64::from(cursor.cell.column());
            format!(
                "caret {},{} | screen {},{} | viewport {}+{} / {}",
                cursor.cell.column() + 1,
                cursor.cell.row() + 1,
                screen_col + 1,
                screen_row + 1,
                viewport.offset,
                viewport.visible,
                viewport.total
            )
        },
    );

    let title_line = resolved_visible_title(state.launch_title.as_deref(), &state.terminal_chrome)
        .filter(|title| !title.is_empty())
        .map_or_else(
            || "terminal".to_owned(),
            |title| format!("terminal {title}"),
        );

    Ok(format!("{title_line}\n{cols} cols x {rows} rows\n{caret}"))
}

fn build_terminal_throughput_benchmark_diagnostic_panel_text(
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

fn write_terminal_throughput_results(
    app_home: &AppHome,
    cache_home: &CacheHome,
    scenario_results: &[TerminalThroughputBenchmarkScenarioResult],
) -> eyre::Result<TerminalThroughputBenchmarkResultsReport> {
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
    let report = build_terminal_throughput_results_report(
        app_home,
        &results_path,
        timestamp,
        scenario_results,
    );
    let json = facet_json::to_string_pretty(&report)
        .wrap_err("failed to serialize terminal throughput benchmark results")?;
    std::fs::write(&results_path, json)
        .wrap_err_with(|| format!("failed to write {}", results_path.display()))?;
    Ok(report)
}

fn build_terminal_throughput_results_report(
    app_home: &AppHome,
    results_path: &Path,
    generated_at: chrono::DateTime<Utc>,
    scenario_results: &[TerminalThroughputBenchmarkScenarioResult],
) -> TerminalThroughputBenchmarkResultsReport {
    TerminalThroughputBenchmarkResultsReport {
        results_path: results_path.display().to_string(),
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

fn resize_window_client(
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
        eyre::bail!("failed to resize window")
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
    diagnostic_cell_width: i32,
    diagnostic_cell_height: i32,
    mode: TerminalThroughputBenchmarkMode,
    line_count: usize,
) -> eyre::Result<()> {
    let layout = client_layout(hwnd, terminal_cell_width, terminal_cell_height, true)?;
    let diagnostic_text =
        build_terminal_throughput_benchmark_diagnostic_panel_text(terminal, mode, line_count);
    let terminal_display = terminal.cached_display_state();

    renderer.render_frame_model_blocking(RenderFrameModel {
        layout,
        title: Some("self-test".to_owned()),
        diagnostic_text,
        diagnostic_selection: None,
        window_chrome_buttons_state: WindowChromeButtonsState::default(),
        diagnostic_cell_width,
        diagnostic_cell_height,
        scene: None,
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
    // tool[impl tests.performance.terminal-throughput-pwsh-noprofile]
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

fn system_dpi() -> u32 {
    // Safety: querying the system DPI does not require additional preconditions.
    let dpi = unsafe { GetDpiForSystem() };
    if dpi == 0 {
        USER_DEFAULT_SCREEN_DPI
    } else {
        dpi
    }
}

fn window_dpi(hwnd: WindowHandle) -> u32 {
    // Safety: the window handle is live while processing its window message.
    let dpi = unsafe { GetDpiForWindow(hwnd.raw()) };
    if dpi == 0 { system_dpi() } else { dpi }
}

fn scaled_font_height(base_font_height: i32, dpi: u32) -> i32 {
    scale_for_dpi(base_font_height, dpi).clamp(MAX_FONT_HEIGHT, MIN_FONT_HEIGHT)
}

fn scaled_window_dimension(base_dimension: i32, dpi: u32) -> i32 {
    scale_for_dpi(base_dimension, dpi).max(1)
}

fn scaled_scene_button_size(dpi: u32) -> i32 {
    scaled_window_dimension(windows_scene::DEFAULT_MAX_BUTTON_SIZE, dpi)
}

fn scale_for_dpi(value: i32, dpi: u32) -> i32 {
    if dpi == USER_DEFAULT_SCREEN_DPI {
        return value;
    }

    let sign = value.signum();
    let magnitude = i64::from(value.abs());
    let scaled = ((magnitude * i64::from(dpi)) + i64::from(USER_DEFAULT_SCREEN_DPI / 2))
        / i64::from(USER_DEFAULT_SCREEN_DPI);
    let scaled = i32::try_from(scaled).unwrap_or(i32::MAX);
    sign.saturating_mul(scaled)
}

fn apply_app_dpi(state: &mut AppState, dpi: u32) -> eyre::Result<()> {
    if state.dpi == dpi {
        return Ok(());
    }

    let terminal_font_height = scaled_font_height(TERMINAL_FONT_HEIGHT, dpi);
    let (terminal_cell_width, terminal_cell_height) =
        measure_terminal_cell_size(terminal_font_height)?;
    let diagnostic_font_height = scaled_font_height(DIAGNOSTIC_FONT_HEIGHT, dpi);
    let (diagnostic_cell_width, diagnostic_cell_height) =
        measure_terminal_cell_size(diagnostic_font_height)?;

    state.dpi = dpi;
    state.terminal_font_height = terminal_font_height;
    state.terminal_cell_width = terminal_cell_width;
    state.terminal_cell_height = terminal_cell_height;
    state.diagnostic_font_height = diagnostic_font_height;
    state.diagnostic_cell_width = diagnostic_cell_width;
    state.diagnostic_cell_height = diagnostic_cell_height;
    Ok(())
}

fn apply_scene_dpi(state: &mut SceneAppState, dpi: u32) -> eyre::Result<()> {
    if state.dpi == dpi {
        return Ok(());
    }

    let (terminal_cell_width, terminal_cell_height) =
        measure_terminal_cell_size(scaled_font_height(TERMINAL_FONT_HEIGHT, dpi))?;
    let (diagnostic_cell_width, diagnostic_cell_height) =
        measure_terminal_cell_size(scaled_font_height(DIAGNOSTIC_FONT_HEIGHT, dpi))?;

    state.dpi = dpi;
    state.terminal_cell_width = terminal_cell_width;
    state.terminal_cell_height = terminal_cell_height;
    state.diagnostic_cell_width = diagnostic_cell_width;
    state.diagnostic_cell_height = diagnostic_cell_height;
    Ok(())
}

fn apply_suggested_dpi_rect(hwnd: WindowHandle, lparam: LPARAM) -> eyre::Result<()> {
    let suggested_rect = dpi_changed_suggested_rect(lparam)?;
    let width = suggested_rect.right - suggested_rect.left;
    let height = suggested_rect.bottom - suggested_rect.top;

    // Safety: the system-provided suggested bounds come from WM_DPICHANGED for this live top-level window.
    if unsafe {
        MoveWindow(
            hwnd.raw(),
            suggested_rect.left,
            suggested_rect.top,
            width,
            height,
            true,
        )
    }
    .is_err()
    {
        eyre::bail!("failed to apply WM_DPICHANGED suggested bounds")
    }

    Ok(())
}

fn dpi_changed_suggested_rect(lparam: LPARAM) -> eyre::Result<RECT> {
    if lparam.0 == 0 {
        eyre::bail!("WM_DPICHANGED did not provide a suggested window rectangle")
    }

    // Safety: WM_DPICHANGED guarantees that lParam points to a RECT valid for the duration of message processing.
    Ok(unsafe { *(lparam.0 as *const RECT) })
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
    let font_family = U16CString::from_str(FONT_FAMILY).expect("font family must not contain nul");
    for (slot, value) in font.lfFaceName.iter_mut().zip(font_family.as_slice()) {
        *slot = *value;
    }
    font
}

fn handle_mouse_wheel(hwnd: WindowHandle, wparam: WPARAM, lparam: LPARAM) -> eyre::Result<bool> {
    // behavior[impl window.interaction.zoom.terminal]
    // behavior[impl window.interaction.zoom.output]
    // behavior[impl window.interaction.scrollback.mouse-wheel]
    let ctrl_down = control_key_is_down();
    if !ctrl_down {
        return with_app_state(|state| {
            let layout = terminal_client_layout(hwnd, state)?;
            let point = screen_to_client_point(hwnd, lparam)?;
            if let Some(cell) = terminal_cell_from_client_point(layout, point, true)
                && state.terminal.mouse_reporting_enabled()
                && state
                    .terminal
                    .send_mouse_wheel(cell, high_word_i16(wparam.0) > 0)?
            {
                if state.terminal.take_repaint_requested() {
                    render_current_frame(state, hwnd, None)?;
                }
                return Ok(true);
            }

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
        let layout = terminal_client_layout(hwnd, state)?;
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
            return apply_terminal_zoom_step(state, hwnd, zoom_direction);
        }

        let next_font_height = (state.diagnostic_font_height + (zoom_direction * FONT_ZOOM_STEP))
            .clamp(MAX_FONT_HEIGHT, MIN_FONT_HEIGHT);
        if next_font_height == state.diagnostic_font_height {
            return Ok(true);
        }

        let (cell_width, cell_height) = measure_terminal_cell_size(next_font_height)?;
        debug!(
            font_height = next_font_height,
            const_name = "DIAGNOSTIC_FONT_HEIGHT",
            "diagnostic zoom changed; use this font height for the default constant"
        );
        state.diagnostic_font_height = next_font_height;
        state.diagnostic_cell_width = cell_width;
        state.diagnostic_cell_height = cell_height;
        render_current_frame(state, hwnd, None)?;
        Ok(true)
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowShortcutAction {
    TerminalZoom(ShortcutStep),
    WindowResize(ShortcutStep),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShortcutStep {
    Increase,
    Decrease,
}

fn current_window_shortcut_action(virtual_key: u32) -> Option<WindowShortcutAction> {
    window_shortcut_action(control_key_is_down(), shift_key_is_down(), virtual_key)
}

fn scene_window_shortcut_action(
    control_down: bool,
    virtual_key: u32,
) -> Option<SceneWindowShortcutAction> {
    if !control_down {
        return None;
    }

    if virtual_key == u32::from(b'C') {
        return Some(SceneWindowShortcutAction::CopySelection);
    }

    if virtual_key == u32::from(b'D') {
        return Some(SceneWindowShortcutAction::CloseWindow);
    }

    None
}

fn window_shortcut_action(
    control_down: bool,
    shift_down: bool,
    virtual_key: u32,
) -> Option<WindowShortcutAction> {
    if !control_down {
        return None;
    }

    let step = shortcut_step(virtual_key)?;

    Some(if shift_down {
        WindowShortcutAction::WindowResize(step)
    } else {
        WindowShortcutAction::TerminalZoom(step)
    })
}

fn execute_window_shortcut(hwnd: WindowHandle, action: WindowShortcutAction) -> eyre::Result<bool> {
    match action {
        WindowShortcutAction::TerminalZoom(step) => with_app_state(|state| {
            apply_terminal_zoom_step(state, hwnd, terminal_zoom_direction(step))
        }),
        WindowShortcutAction::WindowResize(step) => {
            let (terminal_cell_width, terminal_cell_height) = with_app_state(|state| {
                Ok((state.terminal_cell_width, state.terminal_cell_height))
            })?;
            resize_window_by_terminal_step(
                hwnd,
                terminal_cell_width,
                terminal_cell_height,
                window_resize_direction(step),
            )?;
            Ok(true)
        }
    }
}

fn shortcut_step(virtual_key: u32) -> Option<ShortcutStep> {
    if virtual_key == u32::from(VK_OEM_PLUS.0) || virtual_key == u32::from(VK_ADD.0) {
        return Some(ShortcutStep::Increase);
    }

    if virtual_key == u32::from(VK_OEM_MINUS.0) || virtual_key == u32::from(VK_SUBTRACT.0) {
        return Some(ShortcutStep::Decrease);
    }

    None
}

fn terminal_zoom_direction(step: ShortcutStep) -> i32 {
    match step {
        ShortcutStep::Increase => -1,
        ShortcutStep::Decrease => 1,
    }
}

fn window_resize_direction(step: ShortcutStep) -> i32 {
    match step {
        ShortcutStep::Increase => 1,
        ShortcutStep::Decrease => -1,
    }
}

fn apply_terminal_zoom_step(
    state: &mut AppState,
    hwnd: WindowHandle,
    zoom_direction: i32,
) -> eyre::Result<bool> {
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

    let layout = terminal_client_layout(hwnd, state)?;
    state.pending_terminal_resize = None;
    apply_terminal_resize(state, layout)?;
    render_current_frame(state, hwnd, None)?;
    Ok(true)
}

fn resize_window_by_terminal_step(
    hwnd: WindowHandle,
    terminal_cell_width: i32,
    terminal_cell_height: i32,
    resize_direction: i32,
) -> eyre::Result<()> {
    let client_rect = hwnd.client_rect()?;
    let width_step = (terminal_cell_width * WINDOW_RESIZE_STEP_COLS).max(1);
    let height_step = (terminal_cell_height * WINDOW_RESIZE_STEP_ROWS).max(1);
    let next_client_width =
        (client_rect.width() + (resize_direction * width_step)).max(MIN_WINDOW_CLIENT_WIDTH);
    let next_client_height =
        (client_rect.height() + (resize_direction * height_step)).max(MIN_WINDOW_CLIENT_HEIGHT);
    resize_window_client(hwnd, next_client_width, next_client_height)
}

fn build_client_layout(
    rect: ClientRect,
    cell_width: i32,
    cell_height: i32,
    diagnostic_panel_visible: bool,
) -> TerminalLayout {
    TerminalLayout {
        client_width: rect.width(),
        client_height: rect.height(),
        cell_width,
        cell_height,
        diagnostic_panel_visible,
    }
}

fn client_layout(
    hwnd: WindowHandle,
    cell_width: i32,
    cell_height: i32,
    diagnostic_panel_visible: bool,
) -> eyre::Result<TerminalLayout> {
    let rect = hwnd.client_rect()?;
    Ok(build_client_layout(
        rect,
        cell_width,
        cell_height,
        diagnostic_panel_visible,
    ))
}

fn terminal_client_layout(hwnd: WindowHandle, state: &AppState) -> eyre::Result<TerminalLayout> {
    client_layout(
        hwnd,
        state.terminal_cell_width,
        state.terminal_cell_height,
        state.diagnostic_panel_visible,
    )
}

fn scene_client_layout(hwnd: WindowHandle, state: &SceneAppState) -> eyre::Result<TerminalLayout> {
    client_layout(
        hwnd,
        state.terminal_cell_width,
        state.terminal_cell_height,
        false,
    )
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

fn should_defer_terminal_resize_during_move_size(
    current_layout: Option<TerminalLayout>,
    next_layout: TerminalLayout,
) -> bool {
    let Some(current_layout) = current_layout else {
        return false;
    };

    layout_has_visible_terminal_cells(current_layout)
        && layout_has_visible_terminal_cells(next_layout)
}

fn layout_has_visible_terminal_cells(layout: TerminalLayout) -> bool {
    let (visible_cols, visible_rows) = layout.visible_grid_size();
    visible_cols > 0 && visible_rows > 0
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

fn with_scene_app_state<T>(
    f: impl FnOnce(&mut SceneAppState) -> eyre::Result<T>,
) -> eyre::Result<T> {
    SCENE_APP_STATE.with(|state| {
        let mut borrowed = state.borrow_mut();
        let app_state = borrowed
            .as_mut()
            .ok_or_else(|| eyre::eyre!("scene application state was not initialized"))?;
        f(app_state)
    })
}

fn handle_left_button_up(hwnd: WindowHandle, lparam: LPARAM) -> eyre::Result<bool> {
    let should_release_capture = with_app_state(|state| {
        Ok(state.pressed_chrome_button.is_some() || state.terminal_scrollbar_drag.is_some())
    })?;
    if should_release_capture {
        hwnd.release_mouse_capture();
    }

    let action = with_app_state(|state| {
        let point = ClientPoint::from_lparam(lparam);
        state.pointer_position = Some(point);
        state.chrome_tooltip.hide(hwnd);

        if state.terminal_scrollbar_drag.take().is_some() {
            let layout = terminal_client_layout(hwnd, state)?;
            state.terminal_scrollbar_hovered_part =
                current_terminal_scrollbar(state)?.and_then(|scrollbar| {
                    terminal_scrollbar_hit_test(
                        layout.terminal_scrollbar_rect().inset(4),
                        scrollbar,
                        point,
                    )
                });
            return Ok(WindowChromePointerAction::RenderOnly);
        }

        if state.pressed_chrome_button.take().is_some() {
            return Ok(WindowChromePointerAction::RenderOnly);
        }

        if state.pending_window_drag.take().is_some() {
            return Ok(WindowChromePointerAction::RenderOnly);
        }

        if let Some(pending_selection) = state.pending_diagnostic_selection.take() {
            state.diagnostic_selection_drag_point = None;
            let layout = terminal_client_layout(hwnd, state)?;
            let cell = diagnostic_panel_cell_from_client_point(
                layout,
                point,
                state.diagnostic_cell_width,
                state.diagnostic_cell_height,
                true,
            );
            if let Some(selection) =
                complete_pending_terminal_selection(pending_selection, point, cell)
            {
                state.diagnostic_selection = Some(selection);
            }
            return Ok(WindowChromePointerAction::RenderOnly);
        }

        if let Some(pending_selection) = state.pending_terminal_selection.take() {
            state.terminal_selection_drag_point = None;
            let layout = terminal_client_layout(hwnd, state)?;
            let point = ClientPoint::from_lparam(lparam);
            let cell = terminal_cell_from_client_point(layout, point, true)
                .map(|cell| state.terminal.viewport_to_screen_cell(cell))
                .transpose()?;
            if let Some(selection) =
                complete_pending_terminal_selection(pending_selection, point, cell)
            {
                state.terminal_selection = Some(selection);
            }
            return Ok(WindowChromePointerAction::RenderOnly);
        }

        Ok(WindowChromePointerAction::NotHandled)
    })?;

    match action {
        WindowChromePointerAction::NotHandled => Ok(false),
        WindowChromePointerAction::Handled => Ok(true),
        WindowChromePointerAction::RenderOnly => {
            with_app_state(|state| render_current_frame(state, hwnd, None))?;
            Ok(true)
        }
        WindowChromePointerAction::Execute(button) => {
            execute_window_chrome_button(hwnd, button);
            Ok(true)
        }
    }
}

/// behavior[impl window.interaction.drag]
/// behavior[impl window.interaction.selection.linear]
/// behavior[impl window.interaction.selection.block-alt-drag]
/// behavior[impl window.interaction.selection.click-dismiss]
#[expect(
    clippy::too_many_lines,
    reason = "the terminal pointer down path coordinates several mutually exclusive interaction modes"
)]
fn handle_left_button_down(hwnd: WindowHandle, lparam: LPARAM) -> eyre::Result<bool> {
    let point = ClientPoint::from_lparam(lparam);
    let in_drag_handle = hit_test_drag_handle_point(hwnd, point)?;
    let selection_mode = if alt_key_is_down() {
        TerminalSelectionMode::Block
    } else {
        TerminalSelectionMode::Linear
    };

    let action = with_app_state(|state| {
        state.pointer_position = Some(point);
        state.pending_window_drag = None;
        state.terminal_selection_drag_point = None;
        state.diagnostic_selection_drag_point = None;
        state.pressed_chrome_button = None;

        let layout = terminal_client_layout(hwnd, state)?;
        if let Some(button) = window_chrome_button_at_point(layout, point) {
            state.pending_terminal_selection = None;
            state.pending_diagnostic_selection = None;
            state.terminal_scrollbar_hovered_part = None;
            if state.terminal_scrollbar_drag.take().is_some() {
                hwnd.release_mouse_capture();
            }
            state.pressed_chrome_button = Some(button);
            hwnd.capture_mouse();
            if button == WindowChromeButton::Diagnostics {
                terminal_toggle_diagnostics_panel(state, hwnd)?;
                return Ok(WindowChromePointerAction::RenderOnly);
            }
            if button == WindowChromeButton::Pin {
                terminal_toggle_pin(state, hwnd)?;
                return Ok(WindowChromePointerAction::RenderOnly);
            }
            return Ok(WindowChromePointerAction::Execute(button));
        }

        state.pending_diagnostic_selection = None;
        if state.diagnostic_panel_visible
            && let Some(cell) = diagnostic_panel_cell_from_client_point(
                layout,
                point,
                state.diagnostic_cell_width,
                state.diagnostic_cell_height,
                false,
            )
        {
            state.terminal_selection = None;
            state.pending_terminal_selection = None;
            state.terminal_selection_drag_point = None;
            state.terminal_scrollbar_hovered_part = None;
            if state.terminal_scrollbar_drag.take().is_some() {
                hwnd.release_mouse_capture();
            }
            state.diagnostic_selection = None;
            state.pending_diagnostic_selection = Some(PendingTerminalSelection {
                origin: point,
                anchor: cell,
                mode: selection_mode,
            });
            state.diagnostic_selection_drag_point = Some(point);
            return Ok(WindowChromePointerAction::RenderOnly);
        }

        if !in_drag_handle {
            state.pending_terminal_selection = None;
            state.diagnostic_selection = None;

            if let Some(scrollbar) = current_terminal_scrollbar(state)? {
                let scrollbar_rect = layout.terminal_scrollbar_rect().inset(4);
                if let Some(part) = terminal_scrollbar_hit_test(scrollbar_rect, scrollbar, point) {
                    let Some(geometry) = terminal_scrollbar_geometry(scrollbar_rect, scrollbar)
                    else {
                        return Ok(WindowChromePointerAction::NotHandled);
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
                    return Ok(WindowChromePointerAction::RenderOnly);
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
                return Ok(WindowChromePointerAction::RenderOnly);
            }

            return Ok(WindowChromePointerAction::NotHandled);
        }

        state.terminal_selection = None;
        state.pending_terminal_selection = None;
        state.terminal_selection_drag_point = None;
        state.diagnostic_selection = None;
        state.pending_diagnostic_selection = None;
        state.diagnostic_selection_drag_point = None;
        state.terminal_scrollbar_hovered_part = None;
        state.terminal_scrollbar_drag = None;
        begin_system_window_drag(hwnd, point)?;
        Ok(WindowChromePointerAction::Handled)
    })?;

    match action {
        WindowChromePointerAction::NotHandled => Ok(false),
        WindowChromePointerAction::Handled => Ok(true),
        WindowChromePointerAction::RenderOnly => {
            with_app_state(|state| render_current_frame(state, hwnd, None))?;
            Ok(true)
        }
        WindowChromePointerAction::Execute(button) => {
            execute_window_chrome_button(hwnd, button);
            Ok(true)
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "pointer move handling merges selection, scrollbar, drag, and hover state updates"
)]
fn handle_mouse_move(hwnd: WindowHandle, wparam: WPARAM, lparam: LPARAM) -> eyre::Result<bool> {
    let point = ClientPoint::from_lparam(lparam);

    let previous_pointer = with_app_state(|state| {
        let previous = state.pointer_position;
        state.pointer_position = Some(point);
        update_terminal_chrome_tooltip(state, hwnd, point)?;
        Ok(previous)
    })?;

    let diagnostic_selection_result = with_app_state(|state| {
        let Some(pending_selection) = state.pending_diagnostic_selection else {
            return Ok(None);
        };

        state.diagnostic_selection_drag_point = Some(point);

        let action = if (wparam.0 & 0x0001) == 0 {
            update_pending_terminal_selection_action(pending_selection, point, false, None)
        } else if point == pending_selection.origin {
            update_pending_terminal_selection_action(pending_selection, point, true, None)
        } else {
            let layout = terminal_client_layout(hwnd, state)?;
            let cell = diagnostic_panel_cell_from_client_point(
                layout,
                point,
                state.diagnostic_cell_width,
                state.diagnostic_cell_height,
                true,
            );
            update_pending_terminal_selection_action(pending_selection, point, true, cell)
        };

        match action {
            PendingTerminalSelectionAction::KeepPending => Ok(Some(true)),
            PendingTerminalSelectionAction::ClearPending => {
                state.pending_diagnostic_selection = None;
                state.diagnostic_selection_drag_point = None;
                Ok(Some(state.diagnostic_selection.is_some()))
            }
            PendingTerminalSelectionAction::Update(selection) => {
                state.diagnostic_selection = Some(selection);
                Ok(Some(true))
            }
        }
    })?;

    if let Some(consumed) = diagnostic_selection_result {
        if consumed {
            with_app_state(|state| render_current_frame(state, hwnd, None))?;
        }
        return Ok(consumed);
    }

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
            let layout = terminal_client_layout(hwnd, state)?;
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
        PendingDragAction::NotHandled => {
            let should_render = with_app_state(|state| {
                let layout = terminal_client_layout(hwnd, state)?;
                Ok(
                    terminal_interactive_region_contains(state, layout, previous_pointer)
                        || terminal_interactive_region_contains(state, layout, Some(point)),
                )
            })?;
            if should_render {
                with_app_state(|state| render_current_frame(state, hwnd, None))?;
                return Ok(true);
            }
            Ok(false)
        }
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
    let layout = terminal_client_layout(hwnd, state)?;
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
        let layout = terminal_client_layout(hwnd, state)?;
        if state.diagnostic_panel_visible && diagnostic_panel_text_rect(layout).contains(point) {
            state.pending_diagnostic_selection = None;
            state.diagnostic_selection_drag_point = None;
            if let Some(selection) = state.diagnostic_selection.take() {
                return Ok(RightClickTerminalPreparation::CopyDiagnostic(
                    cell_grid::extract_selected_text(
                        diagnostic_panel_text_rect(layout),
                        &build_diagnostic_panel_text(state, layout)?,
                        state.diagnostic_cell_width,
                        state.diagnostic_cell_height,
                        selection,
                    ),
                ));
            }

            return Ok(RightClickTerminalPreparation::CopyDiagnostic(
                build_diagnostic_panel_text(state, layout)?,
            ));
        }

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
        RightClickTerminalPreparation::CopyDiagnostic(selected_text) => {
            if !selected_text.is_empty()
                && let Err(error) = write_clipboard(&selected_text)
            {
                error!(?error, "failed to copy diagnostics text to the clipboard");
            }
            Ok(true)
        }
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
                        if state.terminal.take_repaint_requested() {
                            render_current_frame(state, hwnd, None)?;
                        }
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
                            if state.terminal.take_repaint_requested() {
                                render_current_frame(state, hwnd, None)?;
                            }
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
        let layout = terminal_client_layout(hwnd, state)?;
        Ok(terminal_drag_handle_contains(layout, point))
    })
}

fn diagnostic_panel_text_rect(layout: TerminalLayout) -> ClientRect {
    layout.diagnostic_panel_rect().inset(14)
}

fn scene_diagnostic_text_rect(layout: TerminalLayout) -> ClientRect {
    layout.terminal_panel_rect().inset(20)
}

fn diagnostic_panel_cell_from_client_point(
    layout: TerminalLayout,
    point: ClientPoint,
    cell_width: i32,
    cell_height: i32,
    clamp_to_bounds: bool,
) -> Option<TerminalCellPoint> {
    cell_grid::cell_from_client_point(
        diagnostic_panel_text_rect(layout),
        point,
        cell_width,
        cell_height,
        clamp_to_bounds,
    )
}

fn scene_diagnostic_cell_from_client_point(
    layout: TerminalLayout,
    point: ClientPoint,
    cell_width: i32,
    cell_height: i32,
    clamp_to_bounds: bool,
) -> Option<TerminalCellPoint> {
    cell_grid::cell_from_client_point(
        scene_diagnostic_text_rect(layout),
        point,
        cell_width,
        cell_height,
        clamp_to_bounds,
    )
}

fn scene_pretty_text_target(
    state: &SceneAppState,
    layout: TerminalLayout,
) -> Option<SceneSelectableTextTarget> {
    if state.diagnostics_visible {
        return None;
    }

    if state.scene_kind == SceneWindowKind::Logs {
        return Some(SceneSelectableTextTarget {
            rect: windows_scene::logs_selectable_text_rect(layout),
            text: logs_window_text(state, layout),
            cell_width: state.diagnostic_cell_width,
            cell_height: state.diagnostic_cell_height,
        });
    }

    if state.scene_kind == SceneWindowKind::TimelinePlaygroundDetail {
        let detail_state = timeline_playground_detail_window_state(state);
        let pretty_text = timeline_playground_detail_pretty_text(&detail_state);
        return Some(SceneSelectableTextTarget {
            rect: windows_scene::timeline_playground_detail_selectable_text_rect(layout),
            text: windows_scene::timeline_playground_detail_selectable_text(&pretty_text),
            cell_width: 8,
            cell_height: 15,
        });
    }

    if state.scene_kind != SceneWindowKind::AudioInputDeviceDetails {
        return None;
    }

    let scramble_input_device_identifiers = state
        .demo_mode_scramble_input_device_identifiers
        .is_enabled();
    let body_rect = layout.terminal_panel_rect().inset(24);
    let rect = windows_scene::audio_input_device_detail_selectable_text_rect(
        body_rect,
        state.audio_input_device_window.is_some(),
    );
    let text = windows_scene::audio_input_device_detail_info_text(
        state.audio_input_device_window.as_ref(),
        scramble_input_device_identifiers,
    );

    Some(SceneSelectableTextTarget {
        rect,
        text,
        cell_width: windows_scene::AUDIO_INPUT_DEVICE_DETAIL_TEXT_CELL_WIDTH,
        cell_height: windows_scene::AUDIO_INPUT_DEVICE_DETAIL_TEXT_CELL_HEIGHT,
    })
}

fn timeline_playground_detail_pretty_text(
    detail_state: &TimelinePlaygroundDetailWindowState,
) -> String {
    detail_state.detail.as_ref().map_or_else(
        || "No timeline item selected".to_owned(),
        |detail| format!("{}", detail.pretty()),
    )
}

fn scene_window_caption(state: &SceneAppState) -> String {
    if state.scene_kind == SceneWindowKind::TimelinePlaygroundDetail {
        let detail_state = timeline_playground_detail_window_state(state);
        if let Some(detail) = detail_state.detail.as_ref() {
            return format!("{} - {}", detail.title(), state.scene_kind.title());
        }
    }
    state.scene_kind.title().to_owned()
}

// timeline[impl playground.detail-title-prefix]
fn sync_scene_window_title(state: &mut SceneAppState, hwnd: WindowHandle) -> eyre::Result<()> {
    let caption = scene_window_caption(state);
    if state.last_applied_scene_window_title != caption {
        hwnd.set_title(&caption)?;
        state.last_applied_scene_window_title = caption;
    }
    Ok(())
}

fn scene_pretty_text_cell_from_client_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
    clamp_to_bounds: bool,
) -> Option<TerminalCellPoint> {
    let target = scene_pretty_text_target(state, layout)?;
    cell_grid::cell_from_client_point(
        target.rect,
        point,
        target.cell_width,
        target.cell_height,
        clamp_to_bounds,
    )
}

fn terminal_interactive_region_contains(
    state: &AppState,
    layout: TerminalLayout,
    point: Option<ClientPoint>,
) -> bool {
    point.is_some_and(|point| {
        layout.title_bar_rect().contains(point)
            || window_chrome_button_at_point(layout, point).is_some()
            || (state.diagnostic_panel_visible
                && diagnostic_panel_text_rect(layout).contains(point))
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
    let hit_test_code = u32::from(low_word_u16(lparam.0));
    if hit_test_code != HTCAPTION && hit_test_code != HTCLIENT {
        return Ok(false);
    }

    let point = cursor_client_point(hwnd)?;
    let cursor = with_app_state(|state| {
        let layout = terminal_client_layout(hwnd, state)?;
        Ok(terminal_cursor_for_point(state, layout, point))
    })?;

    if let Some(cursor) = cursor {
        set_system_cursor(cursor);
        return Ok(true);
    }

    Ok(false)
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

fn set_system_cursor(cursor: PCWSTR) {
    let cursor = load_cursor(cursor);
    // Safety: setting the cursor for the current WM_SETCURSOR handling path is valid.
    unsafe { SetCursor(Some(cursor)) };
}

fn terminal_cursor_for_point(
    state: &AppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<PCWSTR> {
    if window_chrome_button_at_point(layout, point).is_some() {
        return Some(IDC_HAND);
    }

    if state.diagnostic_panel_visible && diagnostic_panel_text_rect(layout).contains(point) {
        return Some(IDC_IBEAM);
    }

    if should_override_drag_cursor(state.in_move_size_loop)
        && terminal_drag_handle_contains(layout, point)
    {
        return Some(IDC_SIZEALL);
    }

    None
}

#[expect(
    clippy::too_many_lines,
    reason = "cursor routing stays centralized so pointer affordances match the active scene state"
)]
fn scene_cursor_for_point(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<PCWSTR> {
    if window_chrome_button_at_point(layout, point).is_some() {
        return Some(IDC_HAND);
    }

    if state.diagnostics_visible && scene_diagnostic_text_rect(layout).contains(point) {
        return Some(IDC_IBEAM);
    }

    if state.scene_kind == SceneWindowKind::Logs && !state.diagnostics_visible {
        if windows_scene::logs_control_at_point(layout, point).is_some() {
            return Some(IDC_HAND);
        }
        if windows_scene::logs_selectable_text_rect(layout).contains(point) {
            return Some(IDC_IBEAM);
        }
    }

    if state.scene_kind == SceneWindowKind::TimelineTranscriptionSettings
        && matches!(
            state.pressed_target,
            Some(ScenePressedTarget::TimelineTranscriptionSettingsTarget(_))
        )
    {
        return Some(IDC_SIZEALL);
    }

    if state.scene_kind == SceneWindowKind::TimelineTranscriptionSettings
        && (timeline_transcription_settings_model_row_at_point(layout, point).is_some()
            || timeline_transcription_settings_input_row_at_point(state, layout, point).is_some()
            || timeline_transcription_settings_output_row_at_point(state, layout, point).is_some()
            || timeline_transcription_settings_target_at_point(state, layout, point).is_some()
            || timeline_transcription_settings_socket_at_point(layout, point).is_some()
            || timeline_transcription_settings_add_text_track_button_at_point(layout, point))
    {
        return Some(IDC_HAND);
    }

    if state.scene_kind == SceneWindowKind::ModelWarning
        && (model_warning_open_button_at_point(state, layout, point).is_some()
            || model_warning_copy_button_at_point(state, layout, point).is_some()
            || windows_scene::model_warning_prepare_rect(layout).contains(point))
    {
        return Some(IDC_HAND);
    }

    if timeline_add_track_button_at_point(state, layout, point) {
        return Some(IDC_HAND);
    }

    if timeline_viewport_control_at_point(state, layout, point).is_some() {
        return Some(IDC_HAND);
    }

    if timeline_transport_play_button_at_point(state, layout, point)
        || timeline_track_playback_button_at_point(state, layout, point).is_some()
        || timeline_track_loopback_button_at_point(state, layout, point).is_some()
        || timeline_track_transcription_toggle_button_at_point(state, layout, point).is_some()
        || timeline_track_transcription_settings_button_at_point(state, layout, point).is_some()
        || timeline_track_record_button_at_point(state, layout, point).is_some()
    {
        return Some(IDC_HAND);
    }

    if timeline_track_reorder_handle_at_point(state, layout, point).is_some()
        || matches!(
            state.pressed_target,
            Some(ScenePressedTarget::TimelineTrackReorderHandle(_))
        )
    {
        return Some(IDC_SIZEALL);
    }

    if timeline_audio_head_at_point(state, layout, point).is_some() {
        return Some(IDC_SIZEWE);
    }

    if timeline_text_block_tooltip_at_point(state, layout, point).is_some() {
        return Some(IDC_HAND);
    }

    if state.timeline_pan_drag.is_some() {
        return Some(IDC_SIZEALL);
    }

    if timeline_selection_surface_at_point(state, layout, point) {
        return Some(IDC_CROSS);
    }

    if timeline_scroll_interaction_at_point(state, layout, point) {
        return Some(IDC_SIZEALL);
    }

    if !state.diagnostics_visible
        && scene_action_at_point(state.scene_kind, layout, point).is_some()
    {
        return Some(IDC_HAND);
    }

    if let Some(cell) = cursor_gallery_cell_at_point(state, layout, point) {
        // windowing[impl cursor-gallery.hover-cursor-shape]
        return Some(cursor_gallery_system_cursor(cell.spec.cursor));
    }

    if demo_mode_button_at_point(state, layout, point)
        || demo_mode_scramble_toggle_at_point(state, layout, point)
    {
        return Some(IDC_HAND);
    }

    if !state.diagnostics_visible
        && (legacy_recording_devices_button_at_point(state, layout, point)
            || audio_daemon_model_at_point(state, layout, point).is_some()
            || audio_daemon_cuda_check_button_at_point(state, layout, point)
            || audio_input_device_arm_button_at_point(state, layout, point)
            || audio_input_device_transcription_button_at_point(state, layout, point)
            || audio_input_device_transcription_flush_button_at_point(state, layout, point)
            || audio_input_device_play_pause_button_at_point(state, layout, point)
            || audio_input_device_loopback_button_at_point(state, layout, point)
            || audio_input_device_detail_legacy_recording_button_at_point(state, layout, point))
    {
        return Some(IDC_HAND);
    }

    if !state.diagnostics_visible
        && audio_input_timeline_head_at_point(state, layout, point).is_some()
    {
        return Some(IDC_SIZEALL);
    }

    if !state.diagnostics_visible && audio_input_timeline_at_point(state, layout, point).is_some() {
        return Some(IDC_IBEAM);
    }

    if should_override_drag_cursor(state.in_move_size_loop)
        && scene_drag_handle_contains(layout, point)
    {
        return Some(IDC_SIZEALL);
    }

    None
}

fn cursor_gallery_system_cursor(cursor: windows_scene::CursorGalleryCursorKind) -> PCWSTR {
    match cursor {
        windows_scene::CursorGalleryCursorKind::Arrow => IDC_ARROW,
        windows_scene::CursorGalleryCursorKind::Hand => IDC_HAND,
        windows_scene::CursorGalleryCursorKind::IBeam => IDC_IBEAM,
        windows_scene::CursorGalleryCursorKind::Cross => IDC_CROSS,
        windows_scene::CursorGalleryCursorKind::Wait => IDC_WAIT,
        windows_scene::CursorGalleryCursorKind::SizeAll => IDC_SIZEALL,
        windows_scene::CursorGalleryCursorKind::Help => IDC_HELP,
    }
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

    let layout = terminal_client_layout(hwnd, state)?;
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

fn update_terminal_chrome_tooltip(
    state: &mut AppState,
    hwnd: WindowHandle,
    point: ClientPoint,
) -> eyre::Result<()> {
    let layout = terminal_client_layout(hwnd, state)?;
    if update_window_chrome_tooltip(
        &mut state.chrome_tooltip,
        hwnd,
        layout,
        point,
        state.diagnostic_panel_visible,
        hwnd.is_zoomed(),
        state.pinned_topmost,
    )? {
        return Ok(());
    }

    state.chrome_tooltip.hide(hwnd);
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "scene tooltip routing intentionally centralizes the ordered hit-test precedence for timeline, audio, and chrome controls"
)]
fn update_scene_chrome_tooltip(
    state: &mut SceneAppState,
    hwnd: WindowHandle,
    point: ClientPoint,
) -> eyre::Result<()> {
    let layout = scene_client_layout(hwnd, state)?;
    if update_window_chrome_tooltip(
        &mut state.chrome_tooltip,
        hwnd,
        layout,
        point,
        state.diagnostics_visible,
        hwnd.is_zoomed(),
        state.pinned_topmost,
    )? {
        return Ok(());
    }

    if let Some((tooltip_text, anchor_rect)) = cursor_gallery_cell_tooltip(state, layout, point) {
        show_scene_tooltip(state, hwnd, point, tooltip_text, anchor_rect)?;
        return Ok(());
    }

    if let Some((tooltip_text, anchor_rect)) = demo_mode_control_tooltip(state, layout, point) {
        show_scene_tooltip(state, hwnd, point, tooltip_text, anchor_rect)?;
        return Ok(());
    }

    if let Some((tooltip_text, anchor_rect)) = logs_control_tooltip(state, layout, point) {
        show_scene_tooltip(state, hwnd, point, tooltip_text, anchor_rect)?;
        return Ok(());
    }

    if let Some((tooltip_text, anchor_rect)) =
        timeline_playground_item_tooltip(state, layout, point)?
    {
        // timeline[impl playground.hover-title-tooltip]
        show_scene_tooltip(state, hwnd, point, &tooltip_text, anchor_rect)?;
        return Ok(());
    }

    if let Some((tooltip_text, anchor_rect)) =
        timeline_viewport_control_tooltip(state, layout, point)
    {
        show_scene_tooltip(state, hwnd, point, tooltip_text, anchor_rect)?;
        return Ok(());
    }

    if let Some((tooltip_text, anchor_rect)) =
        timeline_transport_play_button_tooltip(state, layout, point)
    {
        show_scene_tooltip(state, hwnd, point, tooltip_text, anchor_rect)?;
        return Ok(());
    }

    if let Some((tooltip_text, anchor_rect)) =
        timeline_track_playback_button_tooltip(state, layout, point)
    {
        show_scene_tooltip(state, hwnd, point, tooltip_text, anchor_rect)?;
        return Ok(());
    }

    if let Some((tooltip_text, anchor_rect)) =
        timeline_track_loopback_button_tooltip(state, layout, point)
    {
        show_scene_tooltip(state, hwnd, point, tooltip_text, anchor_rect)?;
        return Ok(());
    }

    if let Some((tooltip_text, anchor_rect)) =
        timeline_track_transcription_toggle_tooltip(state, layout, point)
    {
        show_scene_tooltip(state, hwnd, point, tooltip_text, anchor_rect)?;
        return Ok(());
    }

    if let Some((tooltip_text, anchor_rect)) =
        timeline_track_transcription_settings_tooltip(state, layout, point)
    {
        show_scene_tooltip(state, hwnd, point, tooltip_text, anchor_rect)?;
        return Ok(());
    }

    if let Some((tooltip_text, anchor_rect)) =
        timeline_track_record_button_tooltip(state, layout, point)
    {
        show_scene_tooltip(state, hwnd, point, tooltip_text, anchor_rect)?;
        return Ok(());
    }

    if let Some((tooltip_text, anchor_rect)) = timeline_audio_head_tooltip(state, layout, point) {
        show_scene_tooltip(state, hwnd, point, tooltip_text, anchor_rect)?;
        return Ok(());
    }

    if let Some((tooltip_text, anchor_rect)) =
        timeline_transcription_settings_add_text_track_tooltip(layout, point)
    {
        show_scene_tooltip(state, hwnd, point, tooltip_text, anchor_rect)?;
        return Ok(());
    }

    if let Some((tooltip_text, anchor_rect)) =
        timeline_text_block_tooltip_at_point(state, layout, point)
    {
        show_scene_tooltip(state, hwnd, point, &tooltip_text, anchor_rect)?;
        return Ok(());
    }

    if let Some((tooltip_text, anchor_rect)) = scene_action_tooltip(state, layout, point) {
        show_scene_tooltip(state, hwnd, point, tooltip_text, anchor_rect)?;
        return Ok(());
    }

    if let Some((tooltip_text, anchor_rect)) = audio_input_legacy_tooltip(state, layout, point) {
        show_scene_tooltip(state, hwnd, point, tooltip_text, anchor_rect)?;
        return Ok(());
    }

    if let Some((tooltip_text, anchor_rect)) = audio_input_device_arm_tooltip(state, layout, point)
    {
        show_scene_tooltip(state, hwnd, point, tooltip_text, anchor_rect)?;
        return Ok(());
    }

    if let Some((tooltip_text, anchor_rect)) =
        audio_input_device_transcription_tooltip(state, layout, point)
    {
        show_scene_tooltip(state, hwnd, point, tooltip_text, anchor_rect)?;
        return Ok(());
    }

    if let Some((tooltip_text, anchor_rect)) =
        audio_input_device_transcription_flush_tooltip(state, layout, point)
    {
        show_scene_tooltip(state, hwnd, point, tooltip_text, anchor_rect)?;
        return Ok(());
    }

    if let Some((tooltip_text, anchor_rect)) =
        audio_input_device_play_pause_tooltip(state, layout, point)
    {
        show_scene_tooltip(state, hwnd, point, tooltip_text, anchor_rect)?;
        return Ok(());
    }

    if let Some((tooltip_text, anchor_rect)) =
        audio_input_device_loopback_tooltip(state, layout, point)
    {
        show_scene_tooltip(state, hwnd, point, tooltip_text, anchor_rect)?;
        return Ok(());
    }

    if let Some((tooltip_text, anchor_rect)) =
        audio_input_timeline_head_tooltip(state, layout, point)
    {
        show_scene_tooltip(state, hwnd, point, tooltip_text, anchor_rect)?;
        return Ok(());
    }

    state.chrome_tooltip.hide(hwnd);
    Ok(())
}

fn logs_control_tooltip(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<(&'static str, ClientRect)> {
    if state.scene_kind != SceneWindowKind::Logs || state.diagnostics_visible {
        return None;
    }
    let control = windows_scene::logs_control_at_point(layout, point)?;
    Some((
        windows_scene::logs_control_tooltip(control),
        windows_scene::logs_control_rect(layout, control),
    ))
}

fn timeline_playground_item_tooltip(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> eyre::Result<Option<(String, ClientRect)>> {
    if state.scene_kind != SceneWindowKind::TimelinePlayground || state.diagnostics_visible {
        return Ok(None);
    }
    let Some(playground) = state.timeline_playground.as_ref() else {
        return Ok(None);
    };
    let playground_layout = windows_scene::timeline_playground_layout(
        layout.terminal_panel_rect().inset(24),
        playground.vertical_scroll_offset,
    );
    let query = playground
        .query(u32::try_from(playground_layout.content_rect.width().max(1)).unwrap_or(1))?;
    let render_plan = playground.dataset.render_plan(&query);
    let Some((_rect, target)) = windows_scene::timeline_playground_item_hit_rects(
        playground_layout,
        &render_plan,
        playground.view_state(None),
        &[],
    )
    .into_iter()
    .find(|(rect, _)| rect.contains(point)) else {
        return Ok(None);
    };
    let Some(detail) =
        timeline_playground_detail_for_render_item(&playground.dataset, target.render_item)
    else {
        return Ok(None);
    };
    Ok(Some((
        detail.title().to_owned(),
        timeline_playground_cursor_tooltip_anchor(point)?,
    )))
}

// timeline[impl playground.hover-title-tooltip-cursor]
fn timeline_playground_cursor_tooltip_anchor(point: ClientPoint) -> eyre::Result<ClientRect> {
    let point = point.to_win32_point()?;
    Ok(ClientRect::new(point.x, point.y, point.x + 1, point.y + 1))
}

fn timeline_playground_dataset_time_bounds(dataset: &TimelineDataset) -> Option<(i64, i64)> {
    let mut bounds: Option<(i64, i64)> = None;
    for item in dataset.items() {
        let (start, end) = timeline_playground_item_time_range(item.kind());
        bounds = Some(match bounds {
            Some((current_start, current_end)) => (current_start.min(start), current_end.max(end)),
            None => (start, end),
        });
    }
    bounds
}

fn timeline_playground_dataset_intersects_range(
    dataset: &TimelineDataset,
    visible_start_ns: i64,
    visible_end_ns: i64,
) -> bool {
    dataset.items().iter().any(|item| {
        let (start, end) = timeline_playground_item_time_range(item.kind());
        start <= visible_end_ns && end >= visible_start_ns
    })
}

fn timeline_playground_nearest_item_at_or_before(
    dataset: &TimelineDataset,
    visible_start_ns: i64,
) -> Option<i64> {
    dataset
        .items()
        .iter()
        .filter_map(|item| {
            let (start, end) = timeline_playground_item_time_range(item.kind());
            (end < visible_start_ns).then_some((visible_start_ns - end, i64::midpoint(start, end)))
        })
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, center)| center)
}

fn timeline_playground_nearest_item_at_or_after(
    dataset: &TimelineDataset,
    visible_end_ns: i64,
) -> Option<i64> {
    dataset
        .items()
        .iter()
        .filter_map(|item| {
            let (start, end) = timeline_playground_item_time_range(item.kind());
            (start > visible_end_ns).then_some((start - visible_end_ns, i64::midpoint(start, end)))
        })
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, center)| center)
}

fn timeline_playground_item_time_range(kind: &TimelineItemKind) -> (i64, i64) {
    match kind {
        TimelineItemKind::Span(span) => {
            let start = span.start().as_i64();
            let end = span
                .end()
                .map_or(start, TimelineInstantNs::as_i64)
                .max(start);
            (start, end)
        }
        TimelineItemKind::Event(event) => {
            let at = event.at().as_i64();
            (at, at)
        }
    }
}

fn show_scene_tooltip(
    state: &mut SceneAppState,
    hwnd: WindowHandle,
    point: ClientPoint,
    tooltip_text: &str,
    anchor_rect: ClientRect,
) -> eyre::Result<()> {
    let anchor_rect = client_rect_to_screen_rect(hwnd, anchor_rect)?;
    let cursor_rect = pointer_cursor_screen_rect(hwnd, point)?;
    let monitor_bounds = monitor_work_rect(hwnd)?;
    let tooltip_origin = tooltip_origin(anchor_rect, cursor_rect, monitor_bounds, tooltip_text);
    state
        .chrome_tooltip
        .show_at(hwnd, tooltip_text, tooltip_origin)
}

// windowing[impl virtual-cursor.tooltips]
fn update_scene_virtual_cursor_tooltip(
    state: &mut SceneAppState,
    hwnd: WindowHandle,
) -> eyre::Result<()> {
    let Some(point) = state.scene_virtual_cursor else {
        state.chrome_tooltip.hide(hwnd);
        return Ok(());
    };

    update_scene_chrome_tooltip(state, hwnd, point)
}

// behavior[impl window.appearance.chrome.tooltips.popover]
// behavior[impl window.appearance.chrome.tooltips.cursor-clear]
// behavior[impl window.appearance.chrome.tooltips.monitor-clamped]
fn update_window_chrome_tooltip(
    tooltip: &mut ChromeTooltipController,
    hwnd: WindowHandle,
    layout: TerminalLayout,
    point: ClientPoint,
    diagnostics_active: bool,
    maximized: bool,
    pinned: bool,
) -> eyre::Result<bool> {
    let Some(button) = window_chrome_button_at_point(layout, point) else {
        return Ok(false);
    };

    let tooltip_text =
        window_chrome_button_tooltip_text(button, diagnostics_active, maximized, pinned);
    let anchor_rect = client_rect_to_screen_rect(hwnd, window_chrome_button_rect(layout, button))?;
    let cursor_rect = pointer_cursor_screen_rect(hwnd, point)?;
    let monitor_bounds = monitor_work_rect(hwnd)?;
    let tooltip_origin = tooltip_origin(anchor_rect, cursor_rect, monitor_bounds, tooltip_text);
    tooltip.show_at(hwnd, tooltip_text, tooltip_origin)?;
    Ok(true)
}

// behavior[impl window.appearance.scene-buttons.tooltips.popover]
fn scene_action_tooltip(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<(&'static str, ClientRect)> {
    if state.diagnostics_visible {
        return None;
    }

    let specs = windows_scene::scene_button_specs(state.scene_kind);
    let (button_rect, max_button_size) = if state.scene_kind == SceneWindowKind::TimelinePlayground
    {
        (
            windows_scene::timeline_playground_layout(layout.terminal_panel_rect().inset(24), 0)
                .controls_rect,
            76,
        )
    } else {
        (
            layout.terminal_panel_rect(),
            scaled_scene_button_size(state.dpi),
        )
    };
    let button_layouts =
        windows_scene::layout_scene_buttons(button_rect, specs.len(), max_button_size);

    specs
        .iter()
        .zip(button_layouts)
        .find_map(|(spec, button_layout)| {
            button_layout
                .hit_rect()
                .contains(point)
                .then_some((spec.tooltip, button_layout.hit_rect()))
        })
}

fn cursor_gallery_cell_tooltip(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<(&'static str, ClientRect)> {
    cursor_gallery_cell_at_point(state, layout, point)
        .map(|cell| (cell.spec.label, cell.hit_rect()))
}

fn demo_mode_control_tooltip(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<(&'static str, ClientRect)> {
    if state.diagnostics_visible || state.scene_kind != SceneWindowKind::DemoMode {
        return None;
    }

    let demo_layout = windows_scene::demo_mode_layout(layout.terminal_panel_rect().inset(30));
    if demo_layout.demo_button_bounds.contains(point) {
        return Some((
            "Demo Mode keeps showcase settings in one place.",
            demo_layout.demo_button_bounds,
        ));
    }
    demo_layout.scramble_toggle_bounds.contains(point).then_some((
        "Scramble input device identifiers for demos. Generated IDs keep a stable endpoint-like shape while hiding real microphone or input-device values.",
        demo_layout.scramble_toggle_bounds,
    ))
}

fn timeline_viewport_control_tooltip(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<(&'static str, ClientRect)> {
    let action = timeline_viewport_control_at_point(state, layout, point)?;
    let timeline_layout =
        windows_scene::timeline_document_layout(layout.terminal_panel_rect().inset(24));
    Some((
        windows_scene::timeline_viewport_control_tooltip(action)?,
        windows_scene::timeline_viewport_control_rect(timeline_layout, action)?,
    ))
}

fn timeline_transport_play_button_tooltip(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<(&'static str, ClientRect)> {
    if state.diagnostics_visible || !timeline_transport_play_button_at_point(state, layout, point) {
        return None;
    }

    let rect = windows_scene::timeline_transport_play_button_rect(timeline_layout(layout));
    Some((
        if state
            .audio_input_device_window
            .as_ref()
            .is_some_and(AudioInputDeviceWindowState::is_playing)
        {
            "Pause playback (Space)"
        } else {
            "Play recorded buffer (Space)"
        },
        rect,
    ))
}

fn timeline_track_record_button_tooltip(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<(&'static str, ClientRect)> {
    let row_index = timeline_track_record_button_at_point(state, layout, point)?;
    let rect =
        windows_scene::timeline_track_record_button_rect(timeline_layout(layout), row_index)?;
    Some((
        if state
            .audio_input_device_window
            .as_ref()
            .is_some_and(AudioInputDeviceWindowState::is_recording)
        {
            "Stop recording"
        } else {
            "Start recording"
        },
        rect,
    ))
}

fn timeline_track_playback_button_tooltip(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<(&'static str, ClientRect)> {
    let row_index = timeline_track_playback_button_at_point(state, layout, point)?;
    let rect =
        windows_scene::timeline_track_playback_button_rect(timeline_layout(layout), row_index)?;
    Some((
        if state
            .audio_input_device_window
            .as_ref()
            .is_some_and(AudioInputDeviceWindowState::is_playing)
        {
            "Pause playback"
        } else {
            "Play recorded buffer"
        },
        rect,
    ))
}

fn timeline_track_loopback_button_tooltip(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<(&'static str, ClientRect)> {
    let row_index = timeline_track_loopback_button_at_point(state, layout, point)?;
    let rect =
        windows_scene::timeline_track_loopback_button_rect(timeline_layout(layout), row_index)?;
    Some((
        if state
            .audio_input_device_window
            .as_ref()
            .is_some_and(|window| window.loopback_enabled)
        {
            "Disable microphone loopback"
        } else {
            "Enable microphone loopback"
        },
        rect,
    ))
}

fn timeline_track_transcription_toggle_tooltip(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<(&'static str, ClientRect)> {
    let row_index = timeline_track_transcription_toggle_button_at_point(state, layout, point)?;
    let rect = windows_scene::timeline_track_transcription_toggle_button_rect(
        timeline_layout(layout),
        row_index,
    )?;
    Some((
        if state
            .audio_input_device_window
            .as_ref()
            .is_some_and(|window| window.runtime.transcription.enabled)
        {
            "Pause transcription observation"
        } else {
            "Resume transcription observation"
        },
        rect,
    ))
}

fn timeline_track_transcription_settings_tooltip(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<(&'static str, ClientRect)> {
    let row_index = timeline_track_transcription_settings_button_at_point(state, layout, point)?;
    let rect = windows_scene::timeline_track_transcription_settings_button_rect(
        timeline_layout(layout),
        row_index,
    )?;
    Some(("Open transcription settings window", rect))
}

fn timeline_audio_head_tooltip(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<(&'static str, ClientRect)> {
    let audio_input_state = state.audio_input_device_window.as_ref()?;
    windows_scene::timeline_audio_head_grabbers(
        timeline_layout(layout),
        current_timeline_zoom_viewport(state),
        audio_input_state,
    )
    .into_iter()
    .find_map(|grabber| {
        grabber.rect.contains(point).then_some((
            match grabber.kind {
                AudioInputTimelineHeadKind::Recording => "Drag recording head",
                AudioInputTimelineHeadKind::Playback => "Drag playback head",
                AudioInputTimelineHeadKind::Transcription => "Drag transcription head",
            },
            grabber.rect,
        ))
    })
}

// audio[impl gui.legacy-recording-dialog]
fn audio_input_legacy_tooltip(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<(&'static str, ClientRect)> {
    if state.diagnostics_visible {
        return None;
    }

    if legacy_recording_devices_button_at_point(state, layout, point) {
        let body_rect = layout.terminal_panel_rect().inset(22);
        return Some((
            "Open Windows legacy Recording Devices (Alt+R)",
            windows_scene::audio_input_legacy_recording_dialog_button_rect(body_rect),
        ));
    }

    if audio_input_device_detail_legacy_recording_button_at_point(state, layout, point) {
        let body_rect = layout.terminal_panel_rect().inset(24);
        return Some((
            "Open Windows legacy Recording Devices",
            windows_scene::audio_input_device_detail_layout(body_rect).legacy_recording_button_rect,
        ));
    }

    None
}

// audio[impl gui.arm-for-record]
fn audio_input_device_arm_tooltip(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<(&'static str, ClientRect)> {
    if state.diagnostics_visible
        || state.scene_kind != SceneWindowKind::AudioInputDeviceDetails
        || state.audio_input_device_window.is_none()
    {
        return None;
    }

    let body_rect = layout.terminal_panel_rect().inset(24);
    let arm_button_rect =
        windows_scene::audio_input_device_detail_layout(body_rect).arm_button_rect;
    arm_button_rect.contains(point).then_some((
        if state
            .audio_input_device_window
            .as_ref()
            .is_some_and(AudioInputDeviceWindowState::is_recording)
        {
            "Stop recording (Enter)"
        } else {
            "Start recording (Enter)"
        },
        arm_button_rect,
    ))
}

fn audio_input_device_loopback_tooltip(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<(&'static str, ClientRect)> {
    if state.diagnostics_visible
        || state.scene_kind != SceneWindowKind::AudioInputDeviceDetails
        || state.audio_input_device_window.is_none()
    {
        return None;
    }

    let body_rect = layout.terminal_panel_rect().inset(24);
    let rect = windows_scene::audio_input_device_detail_layout(body_rect).loopback_button_rect;
    if !rect.contains(point) {
        return None;
    }

    Some((
        if state
            .audio_input_device_window
            .as_ref()
            .is_some_and(|window| window.loopback_enabled)
        {
            "Disable microphone loopback"
        } else {
            "Enable microphone loopback"
        },
        rect,
    ))
}

fn audio_input_device_transcription_tooltip(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<(&'static str, ClientRect)> {
    if state.diagnostics_visible
        || state.scene_kind != SceneWindowKind::AudioInputDeviceDetails
        || state.audio_input_device_window.is_none()
    {
        return None;
    }

    let body_rect = layout.terminal_panel_rect().inset(24);
    let rect = windows_scene::audio_input_device_detail_layout(body_rect).transcription_button_rect;
    if !rect.contains(point) {
        return None;
    }

    Some((
        if state
            .audio_input_device_window
            .as_ref()
            .is_some_and(|window| window.runtime.transcription.enabled)
        {
            "Disable transcription preview (T)"
        } else {
            "Enable transcription preview (T)"
        },
        rect,
    ))
}

fn audio_input_device_transcription_flush_tooltip(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<(&'static str, ClientRect)> {
    if state.diagnostics_visible
        || state.scene_kind != SceneWindowKind::AudioInputDeviceDetails
        || state.audio_input_device_window.is_none()
    {
        return None;
    }

    let body_rect = layout.terminal_panel_rect().inset(24);
    let rect =
        windows_scene::audio_input_device_detail_layout(body_rect).transcription_flush_button_rect;
    if !rect.contains(point) {
        return None;
    }

    Some(("Flush current transcription chunk (F)", rect))
}

fn audio_input_device_play_pause_tooltip(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<(&'static str, ClientRect)> {
    if state.diagnostics_visible
        || state.scene_kind != SceneWindowKind::AudioInputDeviceDetails
        || state.audio_input_device_window.is_none()
    {
        return None;
    }

    let body_rect = layout.terminal_panel_rect().inset(24);
    let rect = windows_scene::audio_input_device_detail_layout(body_rect).play_pause_button_rect;
    if !rect.contains(point) {
        return None;
    }

    Some((
        if state
            .audio_input_device_window
            .as_ref()
            .is_some_and(AudioInputDeviceWindowState::is_playing)
        {
            "Pause playback (Space)"
        } else {
            "Play recorded buffer (Space)"
        },
        rect,
    ))
}

fn audio_input_timeline_head_tooltip(
    state: &SceneAppState,
    layout: TerminalLayout,
    point: ClientPoint,
) -> Option<(&'static str, ClientRect)> {
    if state.diagnostics_visible || state.scene_kind != SceneWindowKind::AudioInputDeviceDetails {
        return None;
    }
    let body_rect = layout.terminal_panel_rect().inset(24);
    let waveform_rect = windows_scene::audio_input_device_detail_layout(body_rect).waveform_rect;
    let device_window = state.audio_input_device_window.as_ref()?;
    let grabber = windows_scene::audio_input_timeline_head_grabbers(waveform_rect, device_window)
        .into_iter()
        .find(|grabber| grabber.rect.contains(point))?;
    Some((
        match grabber.kind {
            AudioInputTimelineHeadKind::Recording => "Recording head",
            AudioInputTimelineHeadKind::Playback => "Playback head",
            AudioInputTimelineHeadKind::Transcription => "Transcription head",
        },
        grabber.rect,
    ))
}

fn window_chrome_button_tooltip_text(
    button: WindowChromeButton,
    diagnostics_active: bool,
    maximized: bool,
    pinned: bool,
) -> &'static str {
    match button {
        WindowChromeButton::Pin => {
            if pinned {
                "Unpin window from top"
            } else {
                "Keep window on top"
            }
        }
        WindowChromeButton::Diagnostics => {
            if diagnostics_active {
                "Hide diagnostics"
            } else {
                "Show diagnostics"
            }
        }
        WindowChromeButton::Minimize => "Minimize window",
        WindowChromeButton::MaximizeRestore => {
            if maximized {
                "Restore window"
            } else {
                "Maximize window"
            }
        }
        WindowChromeButton::Close => "Close window",
    }
}

fn client_rect_to_screen_rect(hwnd: WindowHandle, rect: ClientRect) -> eyre::Result<ScreenRect> {
    let top_left = client_to_screen_point(hwnd, ClientPoint::new(rect.left(), rect.top()))?;
    let bottom_right = client_to_screen_point(hwnd, ClientPoint::new(rect.right(), rect.bottom()))?;
    let top_left = top_left.to_win32_point()?;
    let bottom_right = bottom_right.to_win32_point()?;
    Ok(ScreenRect::new(
        top_left.x,
        top_left.y,
        bottom_right.x,
        bottom_right.y,
    ))
}

fn move_pointer_to_client_point(hwnd: WindowHandle, point: ClientPoint) -> eyre::Result<()> {
    let point = client_to_screen_point(hwnd, point)?.to_win32_point()?;
    // Safety: setting the OS cursor to a valid screen coordinate is permitted for the active UI interaction.
    unsafe { SetCursorPos(point.x, point.y) }
        .wrap_err("failed to move cursor to transcription settings target")?;
    Ok(())
}

fn pointer_cursor_screen_rect(hwnd: WindowHandle, point: ClientPoint) -> eyre::Result<ScreenRect> {
    const CURSOR_WIDTH: i32 = 24;
    const CURSOR_HEIGHT: i32 = 24;
    let point = client_to_screen_point(hwnd, point)?.to_win32_point()?;
    Ok(ScreenRect::new(
        point.x,
        point.y,
        point.x + CURSOR_WIDTH,
        point.y + CURSOR_HEIGHT,
    ))
}

fn monitor_work_rect(hwnd: WindowHandle) -> eyre::Result<ScreenRect> {
    // Safety: querying the nearest monitor for a live top-level window is valid.
    let monitor = unsafe { MonitorFromWindow(hwnd.raw(), MONITOR_FROM_FLAGS(2)) };
    if monitor.0.is_null() {
        eyre::bail!("failed to resolve monitor for tooltip placement")
    }

    let mut info = MONITORINFO {
        cbSize: u32::try_from(std::mem::size_of::<MONITORINFO>())
            .expect("MONITORINFO size must fit in u32"),
        ..Default::default()
    };
    // Safety: `info` is a valid out pointer for monitor metadata returned for the resolved monitor handle.
    if !unsafe { GetMonitorInfoW(monitor, &raw mut info) }.as_bool() {
        eyre::bail!("failed to query monitor bounds for tooltip placement")
    }

    Ok(ScreenRect::from_win32_rect(info.rcWork))
}

fn tooltip_origin(
    anchor_rect: ScreenRect,
    cursor_rect: ScreenRect,
    bounds: ScreenRect,
    text: &str,
) -> ScreenPoint {
    let (tooltip_width, tooltip_height) = chrome_tooltip_size(text, bounds);
    let margin = 6;
    let gap = 10;
    let min_left = bounds.left() + margin;
    let max_left = (bounds.right() - margin - tooltip_width).max(min_left);
    let min_top = bounds.top() + margin;
    let max_top = (bounds.bottom() - margin - tooltip_height).max(min_top);
    let above_top = anchor_rect.top() - gap - tooltip_height;
    let below_top = anchor_rect.bottom() + gap;
    let preferred_left = anchor_rect.left() + ((anchor_rect.width() - tooltip_width) / 2);
    let preferred_top = if above_top >= min_top {
        above_top
    } else {
        below_top
    };
    let ideal = ScreenPoint::new(
        preferred_left.clamp(min_left, max_left),
        preferred_top.clamp(min_top, max_top),
    );

    let mut candidates = vec![ideal];
    let horizontal_targets = [
        preferred_left,
        cursor_rect.left() - gap - tooltip_width,
        cursor_rect.right() + gap,
        anchor_rect.left() - gap - tooltip_width,
        anchor_rect.right() + gap,
        min_left,
        max_left,
    ];
    let vertical_targets = [above_top, below_top, min_top, max_top];

    for left in horizontal_targets {
        for top in vertical_targets {
            candidates.push(ScreenPoint::new(
                left.clamp(min_left, max_left),
                top.clamp(min_top, max_top),
            ));
        }
    }

    let best = candidates
        .into_iter()
        .map(|origin| {
            let origin = origin
                .to_win32_point()
                .expect("candidate tooltip point should stay integral");
            let rect = ScreenRect::new(
                origin.x,
                origin.y,
                origin.x + tooltip_width,
                origin.y + tooltip_height,
            );
            let intersects_cursor = rects_intersect(rect, cursor_rect);
            let overlaps_anchor = rects_intersect(rect, anchor_rect);
            let edge_penalty = distance_to_monitor_edge(rect, bounds);
            let anchor_distance = manhattan_distance_to_anchor(rect, anchor_rect);
            let cursor_penalty = if intersects_cursor { 1_000_000 } else { 0 };
            let anchor_penalty = if overlaps_anchor { 100_000 } else { 0 };
            let above_bonus = if rect.bottom() <= anchor_rect.top() {
                0
            } else {
                10_000
            };
            (
                cursor_penalty + anchor_penalty + above_bonus - edge_penalty + anchor_distance,
                rect,
            )
        })
        .min_by_key(|(score, _)| *score)
        .expect("candidate tooltip placements should not be empty");

    ScreenPoint::new(best.1.left(), best.1.top())
}

fn distance_to_monitor_edge(rect: ScreenRect, bounds: ScreenRect) -> i32 {
    let left = (rect.left() - bounds.left()).abs();
    let right = (bounds.right() - rect.right()).abs();
    let top = (rect.top() - bounds.top()).abs();
    let bottom = (bounds.bottom() - rect.bottom()).abs();
    left.min(right).min(top).min(bottom)
}

fn manhattan_distance_to_anchor(rect: ScreenRect, anchor: ScreenRect) -> i32 {
    let rect_center_x = rect.left() + (rect.width() / 2);
    let rect_center_y = rect.top() + (rect.height() / 2);
    let anchor_center_x = anchor.left() + (anchor.width() / 2);
    let anchor_center_y = anchor.top() + (anchor.height() / 2);
    (rect_center_x - anchor_center_x).abs() + (rect_center_y - anchor_center_y).abs()
}

fn chrome_tooltip_size(text: &str, bounds: ScreenRect) -> (i32, i32) {
    let glyph_width = 8;
    let glyph_height = 16;
    let horizontal_padding = 12;
    let vertical_padding = 8;
    let margin = 6;
    let text_width = glyph_width
        * i32::try_from(text.chars().count())
            .unwrap_or_default()
            .max(1);
    let width = (text_width + (horizontal_padding * 2))
        .min((bounds.width() - (margin * 2)).max(glyph_width + (horizontal_padding * 2)))
        .max(glyph_width + (horizontal_padding * 2));
    let height = glyph_height + (vertical_padding * 2);
    (width, height)
}

fn rects_intersect(a: ScreenRect, b: ScreenRect) -> bool {
    a.left() < b.right() && a.right() > b.left() && a.top() < b.bottom() && a.bottom() > b.top()
}

#[cfg(test)]
mod tests {
    use crate::timeline::TimelineItemInput;

    use super::*;

    fn timeline_test_state(document: TimelineDocument) -> SceneAppState {
        SceneAppState {
            app_home: AppHome(std::path::PathBuf::from(".")),
            hwnd: None,
            dpi: USER_DEFAULT_SCREEN_DPI,
            scene_kind: SceneWindowKind::Timeline,
            scene_opened_at: Instant::now(),
            vt_engine: VtEngineChoice::default(),
            audio_input_picker: AudioInputPickerState::default(),
            audio_input_picker_completion: AudioInputPickerCompletion::default(),
            audio_input_device_window: None,
            timeline_document: Some(document),
            timeline_transcription_settings: None,
            timeline_document_command_sender: None,
            timeline_document_command_receiver: None,
            timeline_document_command_target: None,
            model_warning: None,
            model_warning_prepare_started_at: None,
            timeline_playground: None,
            timeline_playground_detail: None,
            timeline_tool: TimelineInteractionTool::default(),
            timeline_selection: None,
            pending_timeline_selection: None,
            pending_timeline_text_block: None,
            pending_timeline_track_reorder: None,
            timeline_pan_drag: None,
            timeline_playground_pan_drag: None,
            timeline_zoom_animation: None,
            timeline_vertical_scroll_offset: 0,
            demo_mode_scramble_input_device_identifiers:
                DemoModeInputDeviceIdentifierScramble::default(),
            demo_mode_scramble_toggle_last_changed_at: None,
            scene_action_selected_index: 0,
            scene_virtual_cursor: None,
            pointer_position: None,
            pressed_target: None,
            pin_button_last_clicked_at: None,
            pinned_topmost: false,
            last_clicked_action: None,
            diagnostics_button_last_clicked_at: None,
            diagnostics_visible: false,
            diagnostic_selection: None,
            pending_diagnostic_selection: None,
            diagnostic_selection_drag_point: None,
            in_move_size_loop: false,
            window_focused: false,
            focused_render_interval_ms: 16,
            terminal_cell_width: 8,
            terminal_cell_height: 16,
            diagnostic_cell_width: 8,
            diagnostic_cell_height: 16,
            logs_scroll_offset: 0,
            logs_follow_tail: true,
            last_applied_scene_window_title: SceneWindowKind::Timeline.title().to_owned(),
            chrome_tooltip: ChromeTooltipController::default(),
            renderer: None,
        }
    }

    fn test_scene_layout() -> TerminalLayout {
        TerminalLayout {
            client_width: 1040,
            client_height: 680,
            cell_width: 8,
            cell_height: 16,
            diagnostic_panel_visible: false,
        }
    }

    #[test]
    fn shortcut_action_requires_control() {
        assert_eq!(
            window_shortcut_action(false, false, u32::from(VK_OEM_PLUS.0)),
            None
        );
    }

    #[test]
    fn ctrl_plus_maps_to_terminal_zoom_in() {
        assert_eq!(
            window_shortcut_action(true, false, u32::from(VK_OEM_PLUS.0)),
            Some(WindowShortcutAction::TerminalZoom(ShortcutStep::Increase))
        );
    }

    #[test]
    fn ctrl_minus_maps_to_terminal_zoom_out() {
        assert_eq!(
            window_shortcut_action(true, false, u32::from(VK_OEM_MINUS.0)),
            Some(WindowShortcutAction::TerminalZoom(ShortcutStep::Decrease))
        );
    }

    #[test]
    fn ctrl_shift_plus_maps_to_window_growth() {
        assert_eq!(
            window_shortcut_action(true, true, u32::from(VK_OEM_PLUS.0)),
            Some(WindowShortcutAction::WindowResize(ShortcutStep::Increase))
        );
        assert_eq!(window_resize_direction(ShortcutStep::Increase), 1);
    }

    #[test]
    fn ctrl_shift_minus_maps_to_window_shrink() {
        assert_eq!(
            window_shortcut_action(true, true, u32::from(VK_OEM_MINUS.0)),
            Some(WindowShortcutAction::WindowResize(ShortcutStep::Decrease))
        );
        assert_eq!(window_resize_direction(ShortcutStep::Decrease), -1);
    }

    #[test]
    // timeline[verify viewport.mouse-pan]
    // timeline[verify viewport.mouse-zoom-anchor]
    fn timeline_pan_hit_test_matches_wheel_zoom_surface_for_empty_timeline() {
        let state = timeline_test_state(TimelineDocument::blank());
        let layout = test_scene_layout();
        let timeline_layout = timeline_layout(layout);
        let time_axis_point = rect_center(timeline_layout.time_axis_rect);
        let scrollport_point = rect_center(timeline_layout.scrollport_rect);
        let track_list_point = rect_center(timeline_layout.track_list_rect);

        assert!(timeline_selection_surface_at_point(
            &state,
            layout,
            time_axis_point
        ));
        assert!(timeline_selection_surface_at_point(
            &state,
            layout,
            scrollport_point
        ));
        assert!(timeline_pan_interaction_at_point(
            &state,
            layout,
            time_axis_point
        ));
        assert!(timeline_pan_interaction_at_point(
            &state,
            layout,
            scrollport_point
        ));
        assert!(!timeline_pan_interaction_at_point(
            &state,
            layout,
            track_list_point
        ));
    }

    #[test]
    // timeline[verify playground.viewport-controls]
    fn timeline_playground_pan_hit_test_uses_playground_timeline_surface() {
        let mut state = timeline_test_state(TimelineDocument::blank());
        state.scene_kind = SceneWindowKind::TimelinePlayground;
        state.timeline_document = None;
        state.timeline_playground = Some(TimelinePlaygroundState::new().expect("playground"));
        let layout = test_scene_layout();
        let playground_layout =
            windows_scene::timeline_playground_layout(layout.terminal_panel_rect().inset(24), 0);

        assert!(timeline_playground_pan_interaction_at_point(
            &state,
            layout,
            rect_center(playground_layout.ruler_rect)
        ));
        assert!(timeline_playground_pan_interaction_at_point(
            &state,
            layout,
            rect_center(playground_layout.content_rect)
        ));
        assert!(!timeline_playground_pan_interaction_at_point(
            &state,
            layout,
            rect_center(playground_layout.controls_rect)
        ));
    }

    #[test]
    // timeline[verify playground.viewport-controls]
    fn timeline_playground_right_drag_pan_moves_visible_range() {
        let mut playground = TimelinePlaygroundState::new().expect("playground");
        playground.visible_start_ns = 1_000;
        playground.visible_end_ns = 2_000;
        let pan_drag = TimelinePlaygroundPanDrag {
            origin: ClientPoint::new(100, 20),
            origin_visible_start_ns: playground.visible_start_ns,
            origin_visible_end_ns: playground.visible_end_ns,
            origin_vertical_scroll_offset: playground.vertical_scroll_offset,
        };
        let layout = windows_scene::timeline_playground_layout(
            test_scene_layout().terminal_panel_rect().inset(24),
            0,
        );

        apply_timeline_playground_pan_drag(&mut playground, pan_drag, 25, 0, 100, layout, 18);

        assert_eq!(playground.visible_start_ns, 750);
        assert_eq!(playground.visible_end_ns, 1_750);
        assert_eq!(playground.zoom_animation, None);
    }

    #[test]
    // timeline[verify playground.pan-negative-time]
    fn timeline_playground_pan_can_move_before_zero() {
        let mut playground = TimelinePlaygroundState::new().expect("playground");
        playground.visible_start_ns = 0;
        playground.visible_end_ns = 1_000;

        playground.pan(-1);

        assert_eq!(playground.visible_start_ns, -250);
        assert_eq!(playground.visible_end_ns, 750);
    }

    #[test]
    // timeline[verify playground.pan-negative-time]
    fn timeline_playground_right_drag_can_move_zero_to_center() {
        let mut playground = TimelinePlaygroundState::new().expect("playground");
        playground.visible_start_ns = 0;
        playground.visible_end_ns = 1_000;
        let pan_drag = TimelinePlaygroundPanDrag {
            origin: ClientPoint::new(100, 20),
            origin_visible_start_ns: playground.visible_start_ns,
            origin_visible_end_ns: playground.visible_end_ns,
            origin_vertical_scroll_offset: playground.vertical_scroll_offset,
        };
        let layout = windows_scene::timeline_playground_layout(
            test_scene_layout().terminal_panel_rect().inset(24),
            0,
        );

        apply_timeline_playground_pan_drag(&mut playground, pan_drag, 50, 0, 100, layout, 18);

        assert_eq!(playground.visible_start_ns, -500);
        assert_eq!(playground.visible_end_ns, 500);
    }

    #[test]
    // timeline[verify playground.vertical-pan]
    fn timeline_playground_right_drag_pan_moves_vertical_offset() {
        let mut playground = TimelinePlaygroundState::new().expect("playground");
        let layout = windows_scene::timeline_playground_layout(
            test_scene_layout().terminal_panel_rect().inset(24),
            0,
        );
        let pan_drag = TimelinePlaygroundPanDrag {
            origin: ClientPoint::new(100, 100),
            origin_visible_start_ns: playground.visible_start_ns,
            origin_visible_end_ns: playground.visible_end_ns,
            origin_vertical_scroll_offset: 40,
        };

        apply_timeline_playground_pan_drag(&mut playground, pan_drag, 0, -20, 100, layout, 18);

        assert_eq!(playground.vertical_scroll_offset, 60);
    }

    #[test]
    // timeline[verify playground.live-tracing-pan]
    fn timeline_playground_right_drag_pan_stops_live_tail_reset() {
        let mut playground = TimelinePlaygroundState::new().expect("playground");
        playground.source_mode = TimelinePlaygroundSourceMode::LiveTracingEvents;
        playground.live_tracing_follow_tail = true;
        let layout = windows_scene::timeline_playground_layout(
            test_scene_layout().terminal_panel_rect().inset(24),
            0,
        );
        let pan_drag = TimelinePlaygroundPanDrag {
            origin: ClientPoint::new(100, 100),
            origin_visible_start_ns: playground.visible_start_ns,
            origin_visible_end_ns: playground.visible_end_ns,
            origin_vertical_scroll_offset: 0,
        };

        apply_timeline_playground_pan_drag(&mut playground, pan_drag, 10, 0, 100, layout, 18);

        assert!(!playground.live_tracing_follow_tail);
    }

    #[test]
    // timeline[verify playground.vertical-pan-clamp]
    fn timeline_playground_vertical_offset_reclamps_when_row_count_shrinks() {
        let mut playground = TimelinePlaygroundState::new().expect("playground");
        playground.vertical_scroll_offset = 10_000;
        let layout = windows_scene::timeline_playground_layout(
            test_scene_layout().terminal_panel_rect().inset(24),
            playground.vertical_scroll_offset,
        );

        playground.clamp_vertical_scroll_offset(layout, 1);

        assert_eq!(playground.vertical_scroll_offset, 0);
    }

    #[test]
    // timeline[verify playground.row-transition-animation]
    fn timeline_playground_row_positions_animate_when_prior_rows_disappear() {
        let mut dataset = TimelineDataset::new();
        dataset.push_event(
            TimelineItemInput::new("first").with_group_key("row-a"),
            TimelineInstantNs::new(0),
        );
        dataset.push_event(
            TimelineItemInput::new("second").with_group_key("row-b"),
            TimelineInstantNs::new(1_000),
        );
        dataset.compact();
        let mut playground = TimelinePlaygroundState::new().expect("playground");
        playground.dataset = dataset;

        playground.visible_start_ns = 900;
        playground.visible_end_ns = 1_100;
        let filtered_plan = playground
            .dataset
            .render_plan(&playground.query(1_000).expect("query"));
        let row_b_key = filtered_plan.rows()[0].key();
        playground.last_row_positions = vec![(
            row_b_key,
            windows_scene::timeline_playground_row_world_top(1),
        )];
        playground.update_row_position_animation(&filtered_plan);

        let visual_positions = playground.row_visual_positions(&filtered_plan);
        let row_b_visual_top = visual_positions
            .iter()
            .find_map(|(key, top)| (*key == row_b_key).then_some(*top))
            .expect("row-b visual position");
        assert!(row_b_visual_top > 0);
        assert_eq!(timeline_playground_row_positions(&filtered_plan)[0].1, 0);
    }

    #[test]
    // timeline[verify playground.hover-title-tooltip-stable]
    fn tooltip_controller_recognizes_unchanged_visible_request() {
        let controller = ChromeTooltipController {
            hwnd: None,
            text: PWSTRBuffer::default(),
            active_text: "Import capture".to_owned(),
            active_position: Some(ScreenPoint::new(40, 80)),
            visible: true,
        };

        assert!(controller.is_show_request_current("Import capture", ScreenPoint::new(40, 80)));
        assert!(!controller.is_show_request_current("Other", ScreenPoint::new(40, 80)));
        assert!(!controller.is_show_request_current("Import capture", ScreenPoint::new(41, 80)));
    }

    #[test]
    // timeline[verify playground.hover-title-tooltip-cursor]
    fn timeline_playground_tooltip_anchor_uses_cursor_point() {
        let anchor = timeline_playground_cursor_tooltip_anchor(ClientPoint::new(120, 240))
            .expect("cursor anchor");

        assert_eq!(anchor, ClientRect::new(120, 240, 121, 241));
    }

    #[test]
    // timeline[verify playground.detail-window-clamped]
    fn timeline_playground_detail_window_position_is_clamped_to_virtual_bounds() {
        let owner = ScreenRect::new(1800, 0, 1920, 900);
        let bounds = ScreenRect::new(0, 0, 1920, 1080);

        let rect = timeline_playground_detail_window_position_in_bounds(owner, bounds, 460, 760);

        assert!(rect.left() >= bounds.left());
        assert!(rect.right() <= bounds.right());
        assert!(rect.top() >= bounds.top());
        assert!(rect.bottom() <= bounds.bottom());
    }

    #[test]
    // timeline[verify playground.fit-content]
    fn timeline_playground_fit_content_allows_zero_centering() {
        let mut dataset = TimelineDataset::new();
        dataset.push_event(TimelineItemInput::new("origin"), TimelineInstantNs::new(0));
        let mut playground = TimelinePlaygroundState::new().expect("playground");
        playground.dataset = dataset;
        playground.visible_start_ns = 500;
        playground.visible_end_ns = 1_500;

        playground.fit_to_content();

        assert!(playground.visible_start_ns < 0);
        assert!(playground.visible_end_ns > 0);
    }

    #[test]
    // timeline[verify playground.pan-button-snap-item]
    fn timeline_playground_pan_left_snaps_nearest_content_into_view() {
        let mut dataset = TimelineDataset::new();
        dataset.push_event(TimelineItemInput::new("earlier"), TimelineInstantNs::new(0));
        let mut playground = TimelinePlaygroundState::new().expect("playground");
        playground.dataset = dataset;
        playground.visible_start_ns = 10_000;
        playground.visible_end_ns = 11_000;

        playground.pan(-1);

        assert!(playground.visible_start_ns < 0);
        assert!(playground.visible_end_ns > 0);
    }

    #[test]
    // timeline[verify playground.hover-detail-no-activate]
    fn timeline_playground_detail_window_style_does_not_activate() {
        let style = scene_window_ex_style(SceneWindowKind::TimelinePlaygroundDetail);

        assert_eq!(style & WS_EX_NOACTIVATE, WS_EX_NOACTIVATE);
        assert_eq!(style & WS_EX_TOOLWINDOW, WS_EX_TOOLWINDOW);
        assert_eq!(style & WS_EX_APPWINDOW, WINDOW_EX_STYLE(0));
    }

    #[test]
    // timeline[verify viewport.mouse-pan]
    // timeline[verify viewport.mouse-zoom-anchor]
    fn wheel_zoom_rebases_active_timeline_pan_drag() {
        let original_viewport =
            TimelineViewport::new(TimelineTimeNs::new(1_000), Time::new::<nanosecond>(100.0));
        let target_viewport =
            TimelineViewport::new(TimelineTimeNs::new(2_500), Time::new::<nanosecond>(25.0));
        let mut pan_drag = TimelinePanDrag {
            origin: ClientPoint::new(10, 20),
            origin_viewport: original_viewport,
            origin_vertical_scroll_offset: 12,
        };
        let zoom_point = ClientPoint::new(40, 60);

        rebase_timeline_pan_drag_after_zoom(&mut pan_drag, zoom_point, target_viewport, 48);

        assert_eq!(pan_drag.origin, zoom_point);
        assert_eq!(pan_drag.origin_viewport, target_viewport);
        assert_eq!(pan_drag.origin_vertical_scroll_offset, 48);
    }

    #[test]
    // timeline[verify viewport.mouse-pan]
    // timeline[verify viewport.mouse-zoom-anchor]
    fn timeline_pan_cancels_zoom_animation_at_current_viewport() {
        let start_viewport =
            TimelineViewport::new(TimelineTimeNs::new(1_000), Time::new::<nanosecond>(100.0));
        let target_viewport =
            TimelineViewport::new(TimelineTimeNs::new(2_500), Time::new::<nanosecond>(25.0));
        let mut document = TimelineDocument::blank();
        document.set_viewport(start_viewport);
        let mut state = timeline_test_state(document);
        state.timeline_zoom_animation = Some(TimelineZoomAnimation {
            start_viewport,
            target_viewport,
            started_at: Instant::now() - Duration::from_millis(500),
        });

        let canceled_viewport = cancel_timeline_zoom_animation_at_current_viewport(&mut state);

        assert_eq!(canceled_viewport, target_viewport);
        assert_eq!(state.timeline_zoom_animation, None);
        assert_eq!(
            state
                .timeline_document
                .as_ref()
                .expect("document should remain available")
                .viewport(),
            target_viewport
        );
    }

    #[test]
    // timeline[verify playground.mouse-zoom-anchor]
    fn timeline_playground_zoom_keeps_anchor_time_under_cursor() {
        let mut playground = TimelinePlaygroundState::new().expect("playground");
        playground.visible_start_ns = 0;
        playground.visible_end_ns = 1_000;

        playground.zoom_about_pixels(75, 100, 1, 2);
        let animation = playground.zoom_animation.expect("animation");

        assert_eq!(animation.target_visible_start_ns, 375);
        assert_eq!(animation.target_visible_end_ns, 875);
    }

    #[test]
    // timeline[verify playground.zoom-compounds]
    fn timeline_playground_fast_zoom_compounds_against_pending_target() {
        let mut playground = TimelinePlaygroundState::new().expect("playground");
        playground.visible_start_ns = 0;
        playground.visible_end_ns = 1_000;

        playground.zoom_about_pixels(50, 100, 1, 2);
        playground.zoom_about_pixels(50, 100, 1, 2);

        let animation = playground.zoom_animation.expect("animation");
        assert_eq!(animation.target_visible_start_ns, 375);
        assert_eq!(animation.target_visible_end_ns, 625);
    }

    #[test]
    // timeline[verify playground.live-tracing-events]
    fn timeline_playground_user_navigation_stops_live_tail_reset() {
        let mut playground = TimelinePlaygroundState::new().expect("playground");
        playground.source_mode = TimelinePlaygroundSourceMode::LiveTracingEvents;
        playground.live_tracing_follow_tail = true;

        playground.pan(1);

        assert!(!playground.live_tracing_follow_tail);
    }

    #[test]
    // timeline[verify playground.viewport-transition]
    fn timeline_playground_zoom_transition_uses_bounded_ease_in_out() {
        let mut playground = TimelinePlaygroundState::new().expect("playground");
        playground.visible_start_ns = 0;
        playground.visible_end_ns = 1_000;
        playground.zoom_animation = Some(TimelinePlaygroundZoomAnimation {
            start_visible_start_ns: 0,
            start_visible_end_ns: 1_000,
            target_visible_start_ns: 250,
            target_visible_end_ns: 750,
            started_at: Instant::now() - TIMELINE_ZOOM_ANIMATION_DURATION / 2,
        });

        playground.apply_zoom_animation();

        assert!(playground.zoom_animation.is_some());
        assert!(playground.visible_start_ns > 0);
        assert!(playground.visible_start_ns < 250);
        assert!(playground.visible_end_ns < 1_000);
        assert!(playground.visible_end_ns > 750);
        assert_eq!(ease_in_out(0.0), 0.0);
        assert_eq!(ease_in_out(1.0), 1.0);
        assert!((ease_in_out(0.5) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    // windowing[verify scene.text.keyboard-copy]
    fn ctrl_c_maps_to_scene_copy_selection() {
        assert_eq!(
            scene_window_shortcut_action(true, u32::from(b'C')),
            Some(SceneWindowShortcutAction::CopySelection)
        );
    }

    #[test]
    // windowing[verify scene.keyboard-close]
    fn ctrl_d_maps_to_scene_close_window() {
        assert_eq!(
            scene_window_shortcut_action(true, u32::from(b'D')),
            Some(SceneWindowShortcutAction::CloseWindow)
        );
    }

    #[test]
    // behavior[verify window.startup.size]
    fn initial_window_size_is_1300_by_900() {
        assert_eq!(INITIAL_WINDOW_WIDTH, 1300);
        assert_eq!(INITIAL_WINDOW_HEIGHT, 900);
    }

    // behavior[verify window.appearance.chrome.tooltips.cursor-clear]
    #[test]
    fn tooltip_origin_prefers_above_when_cursor_is_clear() {
        let anchor = ScreenRect::new(500, 300, 542, 342);
        let cursor = ScreenRect::new(510, 342, 534, 366);
        let bounds = ScreenRect::new(0, 0, 1920, 1080);

        let origin = tooltip_origin(anchor, cursor, bounds, "Close window")
            .to_win32_point()
            .expect("tooltip origin should convert to screen pixels");

        assert!(origin.y < anchor.top());
    }

    // behavior[verify window.appearance.chrome.tooltips.cursor-clear]
    #[test]
    fn tooltip_origin_avoids_cursor_aabb_when_above_intersects() {
        let anchor = ScreenRect::new(500, 300, 542, 342);
        let cursor = ScreenRect::new(480, 250, 620, 330);
        let bounds = ScreenRect::new(0, 0, 1920, 1080);

        let origin = tooltip_origin(anchor, cursor, bounds, "Close window")
            .to_win32_point()
            .expect("tooltip origin should convert to screen pixels");
        let (width, height) = chrome_tooltip_size("Close window", bounds);
        let tooltip = ScreenRect::new(origin.x, origin.y, origin.x + width, origin.y + height);

        assert!(!rects_intersect(tooltip, cursor));
        assert!(!rects_intersect(tooltip, anchor));
        assert!(tooltip.left() >= bounds.left());
        assert!(tooltip.right() <= bounds.right());
        assert!(tooltip.top() >= bounds.top());
        assert!(tooltip.bottom() <= bounds.bottom());
    }

    // behavior[verify window.appearance.chrome.tooltips.monitor-clamped]
    #[test]
    fn tooltip_origin_falls_below_when_above_would_escape_monitor_bounds() {
        let anchor = ScreenRect::new(1800, 10, 1842, 52);
        let cursor = ScreenRect::new(1804, 52, 1828, 76);
        let bounds = ScreenRect::new(0, 0, 1920, 1080);

        let origin = tooltip_origin(anchor, cursor, bounds, "Close window")
            .to_win32_point()
            .expect("tooltip origin should convert to screen pixels");
        let (width, height) = chrome_tooltip_size("Close window", bounds);

        assert!(origin.y >= anchor.bottom());
        assert!(origin.x + width <= bounds.right());
        assert!(origin.y + height <= bounds.bottom());
    }

    #[test]
    fn transcription_settings_add_text_track_tooltip_uses_button_rect() {
        let layout = TerminalLayout {
            client_width: 1040,
            client_height: 680,
            cell_width: 8,
            cell_height: 16,
            diagnostic_panel_visible: true,
        };
        let rect =
            windows_scene::timeline_transcription_settings_add_text_track_button_rect(layout);

        let tooltip = timeline_transcription_settings_add_text_track_tooltip(
            layout,
            ClientPoint::new(rect.left() + 4, rect.top() + 4),
        )
        .expect("hovering the add text track button should produce a tooltip");

        assert_eq!(tooltip.0, "Add a new text track for transcription output");
        assert_eq!(tooltip.1, rect);
    }

    #[test]
    fn scene_action_tooltip_uses_hovered_button_hit_rect_and_text() {
        let state = SceneAppState {
            app_home: AppHome(std::path::PathBuf::from(".")),
            hwnd: None,
            dpi: USER_DEFAULT_SCREEN_DPI,
            scene_kind: SceneWindowKind::Launcher,
            scene_opened_at: Instant::now(),
            vt_engine: VtEngineChoice::default(),
            audio_input_picker: AudioInputPickerState::default(),
            audio_input_picker_completion: AudioInputPickerCompletion::default(),
            audio_input_device_window: None,
            timeline_document: None,
            timeline_transcription_settings: None,
            timeline_document_command_sender: None,
            timeline_document_command_receiver: None,
            timeline_document_command_target: None,
            model_warning: None,
            model_warning_prepare_started_at: None,
            timeline_playground: None,
            timeline_playground_detail: None,
            timeline_tool: TimelineInteractionTool::default(),
            timeline_selection: None,
            pending_timeline_selection: None,
            pending_timeline_text_block: None,
            pending_timeline_track_reorder: None,
            timeline_pan_drag: None,
            timeline_playground_pan_drag: None,
            timeline_zoom_animation: None,
            timeline_vertical_scroll_offset: 0,
            demo_mode_scramble_input_device_identifiers:
                DemoModeInputDeviceIdentifierScramble::default(),
            demo_mode_scramble_toggle_last_changed_at: None,
            scene_action_selected_index: 0,
            scene_virtual_cursor: None,
            pointer_position: None,
            pressed_target: None,
            pin_button_last_clicked_at: None,
            pinned_topmost: false,
            last_clicked_action: None,
            diagnostics_button_last_clicked_at: None,
            diagnostics_visible: false,
            diagnostic_selection: None,
            pending_diagnostic_selection: None,
            diagnostic_selection_drag_point: None,
            in_move_size_loop: false,
            window_focused: false,
            focused_render_interval_ms: 16,
            terminal_cell_width: 8,
            terminal_cell_height: 16,
            diagnostic_cell_width: 8,
            diagnostic_cell_height: 16,
            logs_scroll_offset: 0,
            logs_follow_tail: true,
            last_applied_scene_window_title: SceneWindowKind::Launcher.title().to_owned(),
            chrome_tooltip: ChromeTooltipController::default(),
            renderer: None,
        };
        let layout = TerminalLayout {
            client_width: 1040,
            client_height: 680,
            cell_width: 8,
            cell_height: 16,
            diagnostic_panel_visible: false,
        };
        let specs = windows_scene::scene_button_specs(SceneWindowKind::Launcher);
        let button_layouts = windows_scene::layout_scene_buttons(
            layout.terminal_panel_rect(),
            specs.len(),
            scaled_scene_button_size(state.dpi),
        );
        let hover_point = ClientPoint::new(
            button_layouts[0].hit_rect().left() + 1,
            button_layouts[0].hit_rect().top() + 1,
        );

        let tooltip = scene_action_tooltip(&state, layout, hover_point)
            .expect("hovered scene button should expose native tooltip metadata");

        assert_eq!(tooltip.0, "Open terminal");
        assert_eq!(tooltip.1, button_layouts[0].hit_rect());
    }

    // windowing[verify virtual-cursor.tooltips]
    // windowing[verify demo-mode.input-device-identifier-scramble]
    #[test]
    fn demo_mode_toggle_exposes_hover_tooltip_and_navigation_rect() {
        let state = SceneAppState {
            app_home: AppHome(std::path::PathBuf::from(".")),
            hwnd: None,
            dpi: USER_DEFAULT_SCREEN_DPI,
            scene_kind: SceneWindowKind::DemoMode,
            scene_opened_at: Instant::now(),
            vt_engine: VtEngineChoice::default(),
            audio_input_picker: AudioInputPickerState::default(),
            audio_input_picker_completion: AudioInputPickerCompletion::default(),
            audio_input_device_window: None,
            timeline_document: None,
            timeline_transcription_settings: None,
            timeline_document_command_sender: None,
            timeline_document_command_receiver: None,
            timeline_document_command_target: None,
            model_warning: None,
            model_warning_prepare_started_at: None,
            timeline_playground: None,
            timeline_playground_detail: None,
            timeline_tool: TimelineInteractionTool::default(),
            timeline_selection: None,
            pending_timeline_selection: None,
            pending_timeline_text_block: None,
            pending_timeline_track_reorder: None,
            timeline_pan_drag: None,
            timeline_playground_pan_drag: None,
            timeline_zoom_animation: None,
            timeline_vertical_scroll_offset: 0,
            demo_mode_scramble_input_device_identifiers:
                DemoModeInputDeviceIdentifierScramble::default(),
            demo_mode_scramble_toggle_last_changed_at: None,
            scene_action_selected_index: 0,
            scene_virtual_cursor: None,
            pointer_position: None,
            pressed_target: None,
            pin_button_last_clicked_at: None,
            pinned_topmost: false,
            last_clicked_action: None,
            diagnostics_button_last_clicked_at: None,
            diagnostics_visible: false,
            diagnostic_selection: None,
            pending_diagnostic_selection: None,
            diagnostic_selection_drag_point: None,
            in_move_size_loop: false,
            window_focused: false,
            focused_render_interval_ms: 16,
            terminal_cell_width: 8,
            terminal_cell_height: 16,
            diagnostic_cell_width: 8,
            diagnostic_cell_height: 16,
            logs_scroll_offset: 0,
            logs_follow_tail: true,
            last_applied_scene_window_title: SceneWindowKind::DemoMode.title().to_owned(),
            chrome_tooltip: ChromeTooltipController::default(),
            renderer: None,
        };
        let layout = TerminalLayout {
            client_width: 1040,
            client_height: 680,
            cell_width: 8,
            cell_height: 16,
            diagnostic_panel_visible: false,
        };
        let rects = demo_mode_navigation_rects(layout);
        let toggle_point = rect_center(rects[1]);

        let tooltip = demo_mode_control_tooltip(&state, layout, toggle_point)
            .expect("demo mode toggle should expose native tooltip metadata");

        assert_eq!(rects.len(), 2);
        assert!(tooltip.0.contains("Scramble input device identifiers"));
        assert_eq!(tooltip.1, rects[1]);
    }

    #[test]
    fn timeline_viewport_control_tooltip_uses_timeline_control_hit_rect_and_text() {
        let state = SceneAppState {
            app_home: AppHome(std::path::PathBuf::from(".")),
            hwnd: None,
            dpi: USER_DEFAULT_SCREEN_DPI,
            scene_kind: SceneWindowKind::Timeline,
            scene_opened_at: Instant::now(),
            vt_engine: VtEngineChoice::default(),
            audio_input_picker: AudioInputPickerState::default(),
            audio_input_picker_completion: AudioInputPickerCompletion::default(),
            audio_input_device_window: None,
            timeline_document: Some(TimelineDocument::blank()),
            timeline_transcription_settings: None,
            timeline_document_command_sender: None,
            timeline_document_command_receiver: None,
            timeline_document_command_target: None,
            model_warning: None,
            model_warning_prepare_started_at: None,
            timeline_playground: None,
            timeline_playground_detail: None,
            timeline_tool: TimelineInteractionTool::default(),
            timeline_selection: None,
            pending_timeline_selection: None,
            pending_timeline_text_block: None,
            pending_timeline_track_reorder: None,
            timeline_pan_drag: None,
            timeline_playground_pan_drag: None,
            timeline_zoom_animation: None,
            timeline_vertical_scroll_offset: 0,
            demo_mode_scramble_input_device_identifiers:
                DemoModeInputDeviceIdentifierScramble::default(),
            demo_mode_scramble_toggle_last_changed_at: None,
            scene_action_selected_index: 0,
            scene_virtual_cursor: None,
            pointer_position: None,
            pressed_target: None,
            pin_button_last_clicked_at: None,
            pinned_topmost: false,
            last_clicked_action: None,
            diagnostics_button_last_clicked_at: None,
            diagnostics_visible: false,
            diagnostic_selection: None,
            pending_diagnostic_selection: None,
            diagnostic_selection_drag_point: None,
            in_move_size_loop: false,
            window_focused: false,
            focused_render_interval_ms: 16,
            terminal_cell_width: 8,
            terminal_cell_height: 16,
            diagnostic_cell_width: 8,
            diagnostic_cell_height: 16,
            logs_scroll_offset: 0,
            logs_follow_tail: true,
            last_applied_scene_window_title: SceneWindowKind::Timeline.title().to_owned(),
            chrome_tooltip: ChromeTooltipController::default(),
            renderer: None,
        };
        let layout = TerminalLayout {
            client_width: 1040,
            client_height: 680,
            cell_width: 8,
            cell_height: 16,
            diagnostic_panel_visible: false,
        };
        let timeline_layout =
            windows_scene::timeline_document_layout(layout.terminal_panel_rect().inset(24));
        let hover_rect = windows_scene::timeline_viewport_control_rect(
            timeline_layout,
            SceneAction::ZoomTimelineIn,
        )
        .expect("zoom in rect");
        let hover_point = rect_center(hover_rect);

        let tooltip = timeline_viewport_control_tooltip(&state, layout, hover_point)
            .expect("hovered viewport control should expose tooltip metadata");

        assert_eq!(tooltip.0, "Zoom timeline in");
        assert_eq!(tooltip.1, hover_rect);
    }

    #[test]
    // windowing[verify launcher.keyboard-navigation]
    fn launcher_menu_keyboard_navigation_uses_spatial_targets() {
        let specs = windows_scene::scene_button_specs(SceneWindowKind::Launcher);
        let rects = vec![
            ClientRect::new(0, 0, 10, 10),
            ClientRect::new(20, 0, 30, 10),
            ClientRect::new(20, 30, 30, 40),
            ClientRect::new(0, 20, 10, 30),
        ];
        let origin = ClientPoint::new(5, 5);

        assert_eq!(next_sequential_index(specs.len(), 0, -1), specs.len() - 1);
        assert_eq!(next_sequential_index(specs.len(), specs.len() - 1, 1), 0);
        assert_eq!(
            launcher_menu_navigation_from_virtual_key(u32::from(VK_RIGHT.0), false),
            Some(LauncherMenuNavigation::Spatial(
                SpatialNavigationDirection::Right
            ))
        );
        assert_eq!(
            launcher_menu_navigation_from_virtual_key(u32::from(VK_TAB.0), false),
            Some(LauncherMenuNavigation::Sequential(1))
        );
        assert_eq!(
            launcher_menu_navigation_from_virtual_key(u32::from(VK_TAB.0), true),
            Some(LauncherMenuNavigation::Sequential(-1))
        );
        assert_eq!(
            next_spatial_index(&rects, 0, origin, SpatialNavigationDirection::Right),
            Some(1)
        );
        assert_eq!(
            next_spatial_index(&rects, 0, origin, SpatialNavigationDirection::Down),
            Some(3)
        );
    }

    #[test]
    fn terminal_zoom_direction_keeps_plus_and_minus_semantics() {
        assert_eq!(terminal_zoom_direction(ShortcutStep::Increase), -1);
        assert_eq!(terminal_zoom_direction(ShortcutStep::Decrease), 1);
        assert_eq!(
            shortcut_step(u32::from(VK_OEM_PLUS.0)),
            Some(ShortcutStep::Increase)
        );
        assert_eq!(
            shortcut_step(u32::from(VK_OEM_MINUS.0)),
            Some(ShortcutStep::Decrease)
        );
    }

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
            diagnostic_panel_visible: true,
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

    // windowing[verify diagnostics.terminal.bottom-panel-toggle]
    #[test]
    fn build_client_layout_respects_explicit_diagnostic_visibility() {
        let rect = ClientRect::new(0, 0, 800, 600);
        let hidden = build_client_layout(rect, 8, 16, false);
        let visible = build_client_layout(rect, 8, 16, true);

        assert_eq!(hidden.diagnostic_panel_rect().height(), 0);
        assert!(visible.diagnostic_panel_rect().height() > 0);
        assert!(hidden.terminal_panel_rect().bottom() > visible.terminal_panel_rect().bottom());
    }

    #[test]
    fn move_size_resize_is_deferred_while_terminal_stays_visible() {
        let current = TerminalLayout {
            client_width: 1040,
            client_height: 680,
            cell_width: 8,
            cell_height: 16,
            diagnostic_panel_visible: true,
        };
        let next = TerminalLayout {
            client_width: 980,
            client_height: 540,
            cell_width: 8,
            cell_height: 16,
            diagnostic_panel_visible: true,
        };

        assert!(should_defer_terminal_resize_during_move_size(
            Some(current),
            next
        ));
    }

    #[test]
    fn move_size_resize_is_not_deferred_when_terminal_collapses_to_zero_rows() {
        let current = TerminalLayout {
            client_width: 1040,
            client_height: 680,
            cell_width: 8,
            cell_height: 16,
            diagnostic_panel_visible: true,
        };
        let next = TerminalLayout {
            client_width: 320,
            client_height: 40,
            cell_width: 8,
            cell_height: 16,
            diagnostic_panel_visible: true,
        };

        assert!(!should_defer_terminal_resize_during_move_size(
            Some(current),
            next
        ));
    }

    #[test]
    fn move_size_resize_is_not_deferred_when_restoring_from_zero_rows() {
        let current = TerminalLayout {
            client_width: 320,
            client_height: 40,
            cell_width: 8,
            cell_height: 16,
            diagnostic_panel_visible: true,
        };
        let next = TerminalLayout {
            client_width: 1040,
            client_height: 680,
            cell_width: 8,
            cell_height: 16,
            diagnostic_panel_visible: true,
        };

        assert!(!should_defer_terminal_resize_during_move_size(
            Some(current),
            next
        ));
    }

    // tool[verify tests.performance.terminal-throughput-pwsh-noprofile]
    #[test]
    fn terminal_throughput_measure_command_uses_pwsh_noprofile() {
        let command = terminal_throughput_benchmark_command(
            TerminalThroughputBenchmarkMode::MeasureCommandOutHost,
            8,
        )
        .expect("benchmark command should build");
        let argv = command
            .get_argv()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(
            argv.first().is_some_and(|arg| {
                std::path::Path::new(arg)
                    .file_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case("pwsh.exe"))
            }),
            "expected pwsh.exe launcher, got {argv:?}"
        );
        assert!(argv.iter().any(|arg| arg == "-NoProfile"));
        assert!(argv.iter().any(|arg| arg == "-Command"));
    }

    // tool[verify tests.performance.terminal-throughput-pwsh-noprofile]
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

    #[test]
    fn terminal_scrollbar_geometry_clamps_min_thumb_height_to_track_height() {
        let rect = ClientRect::new(0, 0, 16, 12);
        let geometry = terminal_scrollbar_geometry(
            rect,
            TerminalDisplayScrollbar {
                total: 10,
                offset: 0,
                visible: 1,
            },
        )
        .expect("scrollbar geometry should exist");

        assert_eq!(geometry.thumb_height, rect.height());
        assert_eq!(geometry.thumb_rect.bottom(), rect.bottom());
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

    #[test]
    fn custom_windows_disable_redirection_bitmap_for_transparent_composition() {
        assert_eq!(
            custom_window_ex_style(),
            WS_EX_APPWINDOW | WS_EX_NOREDIRECTIONBITMAP,
        );
    }

    #[test]
    fn dpi_scaling_keeps_default_dpi_values_unchanged() {
        assert_eq!(scale_for_dpi(1040, USER_DEFAULT_SCREEN_DPI), 1040);
        assert_eq!(scale_for_dpi(-16, USER_DEFAULT_SCREEN_DPI), -16);
    }

    #[test]
    fn dpi_scaling_scales_negative_font_heights_by_magnitude() {
        assert_eq!(scale_for_dpi(-16, 192), -32);
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

    #[test]
    fn scene_mouse_down_action_invokes_hit_action() {
        assert_eq!(
            scene_mouse_down_action(Some(SceneAction::OpenTerminal),),
            ScenePointerAction::Invoke(SceneAction::OpenTerminal)
        );
    }

    #[test]
    fn scene_mouse_down_action_ignores_empty_hit_target() {
        assert_eq!(
            scene_mouse_down_action(None),
            ScenePointerAction::NotHandled
        );
    }

    #[test]
    fn window_chrome_mouse_down_action_handles_diagnostics_on_press() {
        assert_eq!(
            window_chrome_mouse_down_action(Some(WindowChromeButton::Diagnostics),),
            WindowChromePointerAction::RenderOnly
        );
    }

    #[test]
    fn window_chrome_mouse_down_action_executes_minimize_on_press() {
        assert_eq!(
            window_chrome_mouse_down_action(Some(WindowChromeButton::Minimize),),
            WindowChromePointerAction::Execute(WindowChromeButton::Minimize)
        );
    }

    #[test]
    fn window_chrome_mouse_down_action_ignores_empty_hit_target() {
        assert_eq!(
            window_chrome_mouse_down_action(None),
            WindowChromePointerAction::NotHandled
        );
    }

    // behavior[verify window.appearance.chrome.runtime-terminal-title]
    #[test]
    fn resolved_visible_title_prefers_runtime_title_over_launch_seed() {
        let chrome = TerminalChromeState {
            runtime_title: Some("pwsh.exe".to_owned()),
            progress: TerminalProgressState::Hidden,
        };

        assert_eq!(
            resolved_visible_title(Some("seed"), &chrome),
            Some("pwsh.exe")
        );
    }

    // behavior[verify window.appearance.chrome.runtime-terminal-title]
    #[test]
    fn resolved_visible_title_falls_back_to_launch_seed_before_runtime_title_arrives() {
        let chrome = TerminalChromeState::default();

        assert_eq!(resolved_visible_title(Some("seed"), &chrome), Some("seed"));
    }

    #[test]
    fn resolved_window_caption_falls_back_to_default_caption_when_no_title_exists() {
        let chrome = TerminalChromeState::default();

        assert_eq!(resolved_window_caption(None, &chrome), WINDOW_TITLE);
    }

    #[test]
    fn resolved_window_caption_keeps_explicit_empty_runtime_title() {
        let chrome = TerminalChromeState {
            runtime_title: Some(String::new()),
            progress: TerminalProgressState::Hidden,
        };

        assert_eq!(resolved_window_caption(Some("seed"), &chrome), "");
    }

    // os[verify window.taskbar.progress.osc-9-4]
    #[test]
    fn taskbar_progress_flag_matches_supported_osc_progress_states() {
        assert_eq!(
            taskbar_progress_flag(TerminalProgressState::Hidden),
            TBPF_NOPROGRESS
        );
        assert_eq!(
            taskbar_progress_flag(TerminalProgressState::Normal(10)),
            TBPF_NORMAL
        );
        assert_eq!(
            taskbar_progress_flag(TerminalProgressState::Error(10)),
            TBPF_ERROR
        );
        assert_eq!(
            taskbar_progress_flag(TerminalProgressState::Indeterminate),
            TBPF_INDETERMINATE
        );
        assert_eq!(
            taskbar_progress_flag(TerminalProgressState::Warning(10)),
            TBPF_PAUSED
        );
    }

    #[test]
    fn taskbar_progress_value_is_only_reported_for_percentage_states() {
        assert_eq!(taskbar_progress_value(TerminalProgressState::Hidden), None);
        assert_eq!(
            taskbar_progress_value(TerminalProgressState::Indeterminate),
            None
        );
        assert_eq!(
            taskbar_progress_value(TerminalProgressState::Normal(42)),
            Some(42)
        );
        assert_eq!(
            taskbar_progress_value(TerminalProgressState::Error(17)),
            Some(17)
        );
        assert_eq!(
            taskbar_progress_value(TerminalProgressState::Warning(88)),
            Some(88)
        );
    }
}

fn should_override_drag_cursor(in_move_size_loop: bool) -> bool {
    !in_move_size_loop
}

fn should_render_from_poll_timer(in_move_size_loop: bool) -> bool {
    in_move_size_loop
}
