use std::cell::RefCell;
use std::marker::PhantomData;
use std::path::Path;
use std::thread;
use tracing::trace;

use eyre::Context;
use teamy_windows::clipboard::{read_clipboard, write_clipboard};
use teamy_windows::module::get_current_module;
use tracing::{debug, error, info};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CreateFontIndirectW, DeleteObject, EndPaint, GetDC,
    GetTextExtentPoint32W, HFONT, LOGFONTW, PAINTSTRUCT, ReleaseDC, SelectObject,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_LBUTTON, VK_MENU};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetCursorPos,
    GetSystemMetrics, GetWindowRect, HTCAPTION, HTCLIENT, IDC_ARROW, IDC_SIZEALL, LoadCursorW, MSG,
    PM_REMOVE, PeekMessageW, PostMessageW, PostQuitMessage, RegisterClassExW, SM_CXPADDEDBORDER,
    SM_CXSCREEN, SM_CXSIZEFRAME, SM_CYSCREEN, SM_CYSIZEFRAME, SW_SHOW, SYSTEM_METRICS_INDEX,
    SetCursor, SetTimer, ShowWindow, TranslateMessage, WM_CHAR, WM_DESTROY, WM_ENTERSIZEMOVE,
    WM_ERASEBKGND, WM_EXITSIZEMOVE, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCALCSIZE, WM_NCHITTEST, WM_NCLBUTTONDOWN, WM_PAINT, WM_QUIT,
    WM_RBUTTONUP, WM_SETCURSOR, WM_SIZE, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WNDCLASSEXW,
    WS_EX_APPWINDOW, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_THICKFRAME, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

use crate::paths::AppHome;

use super::WorkspaceWindowState;
use super::spatial::{
    ClientPoint, ClientRect, ScreenPoint, ScreenRect, ScreenToClientTransform, TerminalCellPoint,
    classify_resize_border_hit, drag_threshold_exceeded,
};
use super::windows_d3d12_renderer::{
    D3d12PanelRenderer, PanelEffect, RenderScene, build_panel_scene, push_centered_text,
    push_glyph, push_overlay_panel, push_panel, push_text_block,
};
use super::windows_dialogs::{
    PasteConfirmationChoice, paste_confirmation_required, show_multiline_paste_confirmation_dialog,
};
use super::windows_terminal::{
    POLL_INTERVAL_MS, POLL_TIMER_ID, TerminalDisplayCursorStyle, TerminalDisplayScrollbar,
    TerminalDisplayState, TerminalLayout, TerminalSelection, TerminalSelectionMode,
    TerminalSession, keyboard_mods,
};

unsafe extern "system" {
    fn SetCapture(hwnd: HWND) -> HWND;
    fn ReleaseCapture() -> i32;
}

const WINDOW_CLASS_NAME: PCWSTR = w!("TeamyStudioTerminalWindow");
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
    terminal_font_height: i32,
    terminal_cell_width: i32,
    terminal_cell_height: i32,
    output_font_height: i32,
    output_cell_width: i32,
    output_cell_height: i32,
    terminal: TerminalSession,
    renderer: Option<D3d12PanelRenderer>,
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
        self.window_thread.assert_window_thread();
        // Safety: installing a thread-owned timer on a live HWND is valid.
        let timer = unsafe { SetTimer(Some(self.hwnd), POLL_TIMER_ID, POLL_INTERVAL_MS, None) };
        if timer == 0 {
            eyre::bail!("failed to start terminal poll timer")
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

fn peek_message(message: &mut MSG) -> bool {
    // Safety: `message` is a valid out-pointer for PeekMessageW.
    unsafe { PeekMessageW(&raw mut *message, None, 0, 0, PM_REMOVE) }.into()
}

fn translate_message(message: &MSG) {
    // Safety: `message` was produced by PeekMessageW in this thread.
    let _ = unsafe { TranslateMessage(&raw const *message) };
}

fn dispatch_message(message: &MSG) {
    // Safety: `message` was produced by PeekMessageW in this thread.
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
pub fn run(
    app_home: &AppHome,
    working_dir: Option<&Path>,
    workspace_window: Option<WorkspaceWindowState>,
) -> eyre::Result<()> {
    let window_thread = WindowThread::current();
    let terminal_font_height = TERMINAL_FONT_HEIGHT;
    let (terminal_cell_width, terminal_cell_height) =
        measure_terminal_cell_size(terminal_font_height)?;
    let output_font_height = OUTPUT_FONT_HEIGHT;
    let (output_cell_width, output_cell_height) = measure_terminal_cell_size(output_font_height)?;
    let terminal = TerminalSession::new(app_home, working_dir)?;

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

    let hwnd = create_window(window_thread)?;
    let renderer = D3d12PanelRenderer::new(hwnd.raw())?;
    with_app_state(|state| {
        state.hwnd = Some(hwnd);
        state.renderer = Some(renderer);
        Ok(())
    })?;
    hwnd.show();

    with_app_state(|state| {
        let layout = client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?;
        state.terminal.resize(layout)
    })?;

    info!("Teamy Studio terminal window shown");
    message_loop()
}

/// os[impl window.appearance.os-chrome-none]
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

fn message_loop() -> eyre::Result<()> {
    loop {
        let mut message = MSG::default();
        while peek_message(&mut message) {
            if message.message == WM_QUIT {
                return Ok(());
            }

            translate_message(&message);
            dispatch_message(&message);
        }

        render_frame()?;
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
        WM_ENTERSIZEMOVE => handle_enter_size_move(hwnd),
        WM_EXITSIZEMOVE => handle_exit_size_move(hwnd),
        WM_SIZE => handle_size(hwnd),
        WM_TIMER if wparam.0 == POLL_TIMER_ID => handle_timer(hwnd),
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
        state.terminal.resize(layout)?;
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

fn render_frame() -> eyre::Result<()> {
    with_app_state(|state| {
        let Some(hwnd) = state.hwnd else {
            return Ok(());
        };
        render_current_frame(state, hwnd, None)
    })
}

/// behavior[impl window.interaction.drag.live]
/// behavior[impl window.interaction.resize.live]
/// behavior[impl window.interaction.resize.terminal-live-output]
/// behavior[impl window.interaction.resize.low-latency]
fn handle_poll_timer(hwnd: WindowHandle) -> eyre::Result<bool> {
    with_app_state(|state| {
        let result = state.terminal.pump()?;
        if result.should_close {
            return Ok(true);
        }

        let selection_scrolled = auto_scroll_pending_terminal_selection(state, hwnd)?;

        if should_render_from_poll_timer(state.in_move_size_loop) || selection_scrolled {
            render_current_frame(state, hwnd, None)?;
        }

        Ok(false)
    })
}

fn render_current_frame(
    state: &mut AppState,
    hwnd: WindowHandle,
    resize: Option<(u32, u32)>,
) -> eyre::Result<()> {
    if let Some((width, height)) = resize
        && let Some(renderer) = state.renderer.as_mut()
    {
        renderer.resize(width, height)?;
    }

    if (resize.is_some() || state.in_move_size_loop) && state.terminal.pump()?.should_close {
        hwnd.destroy();
        return Ok(());
    }

    let layout = client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?;
    let mut scene = build_panel_scene(layout);
    let cell_number = state
        .workspace_window
        .as_ref()
        .map_or(1, |workspace_window| workspace_window.cell_number);
    let output_text = build_output_panel_text(state);
    let terminal_display = state
        .terminal
        .visible_display_state_with_selection(state.terminal_selection)?;

    push_centered_text(
        &mut scene,
        layout.drag_handle_rect().to_win32_rect(),
        &cell_number.to_string(),
        [0.95, 0.95, 0.98, 1.0],
    );
    let terminal_rect = layout.terminal_viewport_rect().inset(4);
    let scrollbar_rect = layout.terminal_scrollbar_rect().inset(4);
    push_terminal_display(
        &mut scene,
        terminal_rect,
        state.terminal_cell_width,
        state.terminal_cell_height,
        &terminal_display,
    );
    push_terminal_scrollbar(
        &mut scene,
        scrollbar_rect,
        terminal_display.scrollbar,
        terminal_scrollbar_visual_state(state),
    );
    push_text_block(
        &mut scene,
        layout.result_panel_rect().inset(14).to_win32_rect(),
        &output_text,
        state.output_cell_width,
        state.output_cell_height,
        [0.96, 0.95, 0.90, 1.0],
    );

    let Some(renderer) = state.renderer.as_mut() else {
        return Ok(());
    };
    renderer.render(&scene)
}

/// behavior[impl window.appearance.terminal.cursor.legible-block]
fn push_terminal_display(
    scene: &mut RenderScene,
    terminal_rect: ClientRect,
    cell_width: i32,
    cell_height: i32,
    display: &TerminalDisplayState,
) {
    for background in &display.backgrounds {
        push_panel(
            scene,
            terminal_cell_rect(terminal_rect, background.cell, cell_width, cell_height)
                .to_win32_rect(),
            background.color,
            PanelEffect::TerminalFill,
        );
    }

    for glyph in &display.glyphs {
        push_glyph(
            scene,
            terminal_cell_rect(terminal_rect, glyph.cell, cell_width, cell_height).to_win32_rect(),
            glyph.character,
            glyph.color,
        );
    }

    if let Some(cursor) = display.cursor {
        let cell_rect = terminal_cell_rect(terminal_rect, cursor.cell, cell_width, cell_height);
        for rect in terminal_cursor_overlay_rects(cell_rect, cursor.style) {
            push_overlay_panel(
                scene,
                rect.to_win32_rect(),
                terminal_cursor_overlay_color(cursor.color, cursor.style),
                PanelEffect::TerminalCursor,
            );
        }
    }
}

/// behavior[impl window.appearance.terminal.scrollbar.shader]
fn push_terminal_scrollbar(
    scene: &mut RenderScene,
    scrollbar_rect: ClientRect,
    scrollbar: Option<super::windows_terminal::TerminalDisplayScrollbar>,
    visual_state: TerminalScrollbarVisualState,
) {
    if scrollbar_rect.width() <= 0 || scrollbar_rect.height() <= 0 {
        return;
    }

    push_panel(
        scene,
        scrollbar_rect.to_win32_rect(),
        if visual_state.track_hovered {
            [0.28, 0.10, 0.40, 0.90]
        } else {
            [0.19, 0.08, 0.28, 0.78]
        },
        PanelEffect::TerminalScrollbarTrack,
    );

    let Some(scrollbar) = scrollbar else {
        return;
    };
    let Some(geometry) = terminal_scrollbar_geometry(scrollbar_rect, scrollbar) else {
        return;
    };

    push_panel(
        scene,
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

fn terminal_render_rect(layout: TerminalLayout) -> ClientRect {
    layout.terminal_viewport_rect().inset(4)
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

    let (grid_cols, grid_rows) = layout.grid_size();
    let column = (relative_x / layout.cell_width.max(1)).clamp(0, i32::from(grid_cols) - 1);
    let row = (relative_y / layout.cell_height.max(1)).clamp(0, i32::from(grid_rows) - 1);
    Some(TerminalCellPoint::new(column, row))
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

fn build_output_panel_text(state: &AppState) -> String {
    if let Some(workspace_window) = &state.workspace_window {
        format!(
            "workspace {}\ncell {} of {}\n{} cols x {} rows",
            workspace_window.workspace.name,
            workspace_window.cell_number,
            workspace_window.workspace.cell_count,
            state.terminal.cols(),
            state.terminal.rows()
        )
    } else {
        format!(
            "standalone shell\n{} cols x {} rows",
            state.terminal.cols(),
            state.terminal.rows()
        )
    }
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
            state.terminal.resize(layout)?;
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
                return Ok(true);
            }

            return Ok(false);
        }

        state.terminal_selection = None;
        state.pending_terminal_selection = None;
        state.terminal_selection_drag_point = None;
        state.terminal_scrollbar_hovered_part = None;
        state.terminal_scrollbar_drag = None;
        state.pending_window_drag = Some(PendingWindowDrag { origin: point });
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
        return Ok(consumed);
    }

    let scrollbar_result =
        with_app_state(|state| handle_terminal_scrollbar_mouse_move(state, hwnd, point))?;

    if let Some(consumed) = scrollbar_result {
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
