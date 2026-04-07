use std::cell::RefCell;
use std::path::Path;
use std::thread;

use eyre::Context;
use teamy_windows::module::get_current_module;
use tracing::{debug, error, info};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CreateFontIndirectW, DeleteObject, EndPaint, GetDC,
    GetTextExtentPoint32W, HFONT, LOGFONTW, PAINTSTRUCT, ReleaseDC, SelectObject,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
    GetCursorPos, GetWindowRect, SetCursor,
    GetSystemMetrics, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT,
    HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, IDC_ARROW, IDC_SIZEALL, LoadCursorW, MSG, PM_REMOVE,
    PeekMessageW, PostMessageW, PostQuitMessage, RegisterClassExW, SM_CXPADDEDBORDER, SM_CXSCREEN,
    SM_CXSIZEFRAME, SM_CYSCREEN, SM_CYSIZEFRAME, SW_SHOW, SYSTEM_METRICS_INDEX, SetTimer,
    ShowWindow, TranslateMessage, WM_CHAR, WM_DESTROY,
    WM_ENTERSIZEMOVE, WM_ERASEBKGND, WM_EXITSIZEMOVE, WM_KEYDOWN, WM_KEYUP,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCALCSIZE,
    WM_NCHITTEST, WM_NCLBUTTONDOWN, WM_PAINT, WM_QUIT, WM_SETCURSOR, WM_SIZE,
    WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WNDCLASSEXW, WS_EX_APPWINDOW, WS_MAXIMIZEBOX,
    WS_MINIMIZEBOX, WS_POPUP, WS_THICKFRAME, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

use crate::paths::AppHome;

use super::WorkspaceWindowState;
use super::windows_d3d12_renderer::{
    D3d12PanelRenderer, PanelEffect, RenderScene, build_panel_scene, push_centered_text,
    push_glyph, push_overlay_panel, push_panel, push_text_block,
};
use super::windows_terminal::{
    POLL_INTERVAL_MS, POLL_TIMER_ID, TerminalDisplayCursorStyle, TerminalDisplayState,
    TerminalLayout, TerminalSession, keyboard_mods,
};

const WINDOW_CLASS_NAME: PCWSTR = w!("TeamyStudioTerminalWindow");
const WINDOW_TITLE: &str = "Teamy Studio Terminal";
const TERMINAL_FONT_HEIGHT: i32 = -32;
const OUTPUT_FONT_HEIGHT: i32 = -32;
const FONT_FAMILY: &str = "CaskaydiaCove Nerd Font Mono";
const MIN_FONT_HEIGHT: i32 = -12;
const MAX_FONT_HEIGHT: i32 = -72;
const FONT_ZOOM_STEP: i32 = 2;
const INITIAL_WINDOW_WIDTH: i32 = 1040;
const INITIAL_WINDOW_HEIGHT: i32 = 680;
const DRAG_START_THRESHOLD_PX: i32 = 0;
const MIN_RESIZE_BORDER_THICKNESS: i32 = 1;

thread_local! {
    static APP_STATE: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

struct AppState {
    app_home: AppHome,
    hwnd: Option<HWND>,
    workspace_window: Option<WorkspaceWindowState>,
    pending_window_drag: Option<PendingWindowDrag>,
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

#[derive(Clone, Copy, Debug, PartialEq)]
struct PendingWindowDrag {
    origin: POINT,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingDragAction {
    NotHandled,
    Consumed,
    StartSystemDrag,
}

impl PendingDragAction {
    fn clears_pending_drag(self) -> bool {
        matches!(self, Self::NotHandled | Self::StartSystemDrag)
    }
}

struct FontHandle(HFONT);

impl Drop for FontHandle {
    fn drop(&mut self) {
        let _ = unsafe { DeleteObject(self.0.into()) };
    }
}

/// Launch the Teamy Studio terminal window and block until it closes.
/// cli[impl window.startup.centered]
/// cli[impl window.startup.size]
/// cli[impl window.appearance.translucent]
///
/// # Errors
///
/// This function will return an error if the window class, font, terminal session, or message loop fails.
pub fn run(
    app_home: &AppHome,
    working_dir: Option<&Path>,
    workspace_window: Option<WorkspaceWindowState>,
) -> eyre::Result<()> {
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

    let hwnd = create_window()?;
    let renderer = D3d12PanelRenderer::new(hwnd)?;
    with_app_state(|state| {
        state.hwnd = Some(hwnd);
        state.renderer = Some(renderer);
        Ok(())
    })?;
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
    }

    with_app_state(|state| {
        let layout = client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?;
        state.terminal.resize(layout)
    })?;

    info!("Teamy Studio terminal window shown");
    message_loop()
}

fn create_window() -> eyre::Result<HWND> {
    let instance = get_current_module().wrap_err("failed to get module handle")?;

    let class = WNDCLASSEXW {
        cbSize: u32::try_from(std::mem::size_of::<WNDCLASSEXW>())
            .expect("WNDCLASSEXW size must fit in u32"),
        hInstance: instance.into(),
        lpszClassName: WINDOW_CLASS_NAME,
        lpfnWndProc: Some(window_proc),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() },
        ..Default::default()
    };
    let atom = unsafe { RegisterClassExW(&class) };
    if atom == 0 {
        debug!(
            "terminal window class already registered or registration deferred to create-window path"
        );
    }

    let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let screen_height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    let x = (screen_width - INITIAL_WINDOW_WIDTH) / 2;
    let y = (screen_height - INITIAL_WINDOW_HEIGHT) / 2;
    let title = wide_null_terminated(WINDOW_TITLE);

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

    let timer = unsafe { SetTimer(Some(hwnd), POLL_TIMER_ID, POLL_INTERVAL_MS, None) };
    if timer == 0 {
        eyre::bail!("failed to start terminal poll timer")
    }

    Ok(hwnd)
}

fn message_loop() -> eyre::Result<()> {
    loop {
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.into() {
            if message.message == WM_QUIT {
                return Ok(());
            }

            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
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
    match message {
        WM_NCCALCSIZE => LRESULT(0),
        WM_ENTERSIZEMOVE => match with_app_state(|state| {
            state.in_move_size_loop = true;
            render_current_frame(state, hwnd, None)?;
            Ok(())
        }) {
            Ok(()) => LRESULT(0),
            Err(error) => fail_and_close(hwnd, error),
        },
        WM_EXITSIZEMOVE => match with_app_state(|state| {
            state.in_move_size_loop = false;
            render_current_frame(state, hwnd, None)?;
            Ok(())
        }) {
            Ok(()) => LRESULT(0),
            Err(error) => fail_and_close(hwnd, error),
        },
        WM_SIZE => match with_app_state(|state| {
            let layout =
                client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?;
            state.terminal.resize(layout)?;
            render_current_frame(
                state,
                hwnd,
                Some((layout.client_width as u32, layout.client_height as u32)),
            )?;
            Ok(())
        }) {
            Ok(()) => LRESULT(0),
            Err(error) => fail_and_close(hwnd, error),
        },
        WM_TIMER if wparam.0 == POLL_TIMER_ID => match handle_poll_timer(hwnd) {
            Ok(should_close) => {
                if should_close {
                    unsafe {
                        let _ = DestroyWindow(hwnd);
                    }
                }
                LRESULT(0)
            }
            Err(error) => fail_and_close(hwnd, error),
        },
        WM_CHAR => {
            // cli[impl window.interaction.input]
            match with_app_state(|state| state.terminal.handle_char(wparam.0 as u32, lparam.0)) {
                Ok(result) => {
                    debug!(
                        message = "WM_CHAR",
                        code_unit = wparam.0 as u32,
                        lparam = lparam.0,
                        consumed = result,
                        "processed keyboard char message"
                    );
                    if result {
                        return LRESULT(0);
                    }
                    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
                }
                Err(error) => fail_and_close(hwnd, error),
            }
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => match with_app_state(|state| {
            // cli[impl window.interaction.input]
            let was_down = ((lparam.0 >> 30) & 1) != 0;
            state.terminal.handle_key_event(
                wparam.0 as u32,
                lparam.0,
                was_down,
                false,
                keyboard_mods(wparam.0 as u32, lparam.0, false),
            )
        }) {
            Ok(consumed) => {
                debug!(
                    message = if message == WM_SYSKEYDOWN {
                        "WM_SYSKEYDOWN"
                    } else {
                        "WM_KEYDOWN"
                    },
                    vkey = wparam.0 as u32,
                    lparam = lparam.0,
                    was_down = ((lparam.0 >> 30) & 1) != 0,
                    consumed,
                    "processed keyboard down message"
                );
                if consumed {
                    return LRESULT(0);
                }
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            Err(error) => fail_and_close(hwnd, error),
        },
        WM_KEYUP | WM_SYSKEYUP => match with_app_state(|state| {
            // cli[impl window.interaction.input]
            state.terminal.handle_key_event(
                wparam.0 as u32,
                lparam.0,
                false,
                true,
                keyboard_mods(wparam.0 as u32, lparam.0, true),
            )
        }) {
            Ok(consumed) => {
                debug!(
                    message = if message == WM_SYSKEYUP {
                        "WM_SYSKEYUP"
                    } else {
                        "WM_KEYUP"
                    },
                    vkey = wparam.0 as u32,
                    lparam = lparam.0,
                    consumed,
                    "processed keyboard up message"
                );
                if consumed {
                    return LRESULT(0);
                }
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            Err(error) => fail_and_close(hwnd, error),
        },
        WM_LBUTTONDOWN => match handle_left_button_down(hwnd, lparam) {
            Ok(true) => LRESULT(0),
            Ok(false) => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
            Err(error) => fail_and_close(hwnd, error),
        },
        WM_MOUSEMOVE => match handle_mouse_move(hwnd, wparam, lparam) {
            Ok(true) => LRESULT(0),
            Ok(false) => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
            Err(error) => fail_and_close(hwnd, error),
        },
        WM_PAINT => match acknowledge_paint(hwnd) {
            Ok(()) => LRESULT(0),
            Err(error) => fail_and_close(hwnd, error),
        },
        WM_LBUTTONUP => match handle_left_button_up(hwnd, lparam) {
            Ok(handled) => {
                if handled {
                    LRESULT(0)
                } else {
                    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
                }
            }
            Err(error) => fail_and_close(hwnd, error),
        },
        WM_MOUSEWHEEL => match handle_mouse_wheel(hwnd, wparam, lparam) {
            Ok(handled) => {
                if handled {
                    LRESULT(0)
                } else {
                    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
                }
            }
            Err(error) => fail_and_close(hwnd, error),
        },
        WM_SETCURSOR => match handle_set_cursor(hwnd, lparam) {
            Ok(true) => LRESULT(1),
            Ok(false) => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
            Err(error) => fail_and_close(hwnd, error),
        },
        WM_NCHITTEST => {
            let point = match screen_to_client_point(hwnd, lparam) {
                Ok(point) => point,
                Err(error) => return fail_and_close(hwnd, error),
            };
            match hit_test_resize_border(hwnd, point) {
                Ok(Some(hit)) => return hit,
                Ok(None) => {}
                Err(error) => return fail_and_close(hwnd, error),
            }
            LRESULT(isize::try_from(HTCLIENT).expect("HTCLIENT fits in isize"))
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_DESTROY => {
            APP_STATE.with(|state| {
                let _ = state.borrow_mut().take();
            });
            unsafe {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn acknowledge_paint(hwnd: HWND) -> eyre::Result<()> {
    let mut paint = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
    if hdc.0.is_null() {
        eyre::bail!("failed to begin painting")
    }

    unsafe {
        let _ = EndPaint(hwnd, &paint);
    }
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

fn handle_poll_timer(hwnd: HWND) -> eyre::Result<bool> {
    with_app_state(|state| {
        let result = state.terminal.pump()?;
        if result.should_close {
            return Ok(true);
        }

        if should_render_from_poll_timer(state.in_move_size_loop) {
            render_current_frame(state, hwnd, None)?;
        }

        Ok(false)
    })
}

fn render_current_frame(
    state: &mut AppState,
    hwnd: HWND,
    resize: Option<(u32, u32)>,
) -> eyre::Result<()> {
    if let Some((width, height)) = resize {
        if let Some(renderer) = state.renderer.as_mut() {
            renderer.resize(width, height)?;
        }
    }

    if (resize.is_some() || state.in_move_size_loop) && state.terminal.pump()?.should_close {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return Ok(());
    }

    let layout = client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?;
    let mut scene = build_panel_scene(layout);
    let cell_number = state
        .workspace_window
        .as_ref()
        .map_or(1, |workspace_window| workspace_window.cell_number);
    let output_text = build_output_panel_text(state);
    let terminal_display = state.terminal.visible_display_state()?;

    push_centered_text(
        &mut scene,
        layout.drag_handle_rect(),
        &cell_number.to_string(),
        [0.95, 0.95, 0.98, 1.0],
    );
    let terminal_rect = inset_rect(layout.terminal_rect(), 4);
    push_terminal_display(
        &mut scene,
        terminal_rect,
        state.terminal_cell_width,
        state.terminal_cell_height,
        terminal_display,
    );
    push_text_block(
        &mut scene,
        inset_rect(layout.result_panel_rect(), 14),
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

fn push_terminal_display(
    scene: &mut RenderScene,
    terminal_rect: RECT,
    cell_width: i32,
    cell_height: i32,
    display: TerminalDisplayState,
) {
    for background in display.backgrounds {
        push_panel(
            scene,
            terminal_cell_rect(
                terminal_rect,
                background.column,
                background.row,
                cell_width,
                cell_height,
            ),
            background.color,
            PanelEffect::TerminalFill,
        );
    }

    for glyph in display.glyphs {
        push_glyph(
            scene,
            terminal_cell_rect(
                terminal_rect,
                glyph.column,
                glyph.row,
                cell_width,
                cell_height,
            ),
            glyph.character,
            glyph.color,
        );
    }

    if let Some(cursor) = display.cursor {
        let cell_rect = terminal_cell_rect(
            terminal_rect,
            cursor.column,
            cursor.row,
            cell_width,
            cell_height,
        );
        for rect in terminal_cursor_overlay_rects(cell_rect, cursor.style) {
            push_overlay_panel(
                scene,
                rect,
                terminal_cursor_overlay_color(cursor.color, cursor.style),
                PanelEffect::TerminalCursor,
            );
        }
    }
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
    terminal_rect: RECT,
    column: i32,
    row: i32,
    cell_width: i32,
    cell_height: i32,
) -> RECT {
    let left = terminal_rect.left + (column * cell_width);
    let top = terminal_rect.top + (row * cell_height);
    RECT {
        left,
        top,
        right: left + cell_width,
        bottom: top + cell_height,
    }
}

fn terminal_cursor_overlay_rects(
    cell_rect: RECT,
    style: TerminalDisplayCursorStyle,
) -> Vec<RECT> {
    let width = (cell_rect.right - cell_rect.left).max(1);
    let height = (cell_rect.bottom - cell_rect.top).max(1);
    let thickness = (width.min(height) / 6).clamp(2, 4);

    match style {
        TerminalDisplayCursorStyle::Bar => vec![RECT {
            left: cell_rect.left,
            top: cell_rect.top,
            right: (cell_rect.left + thickness).min(cell_rect.right),
            bottom: cell_rect.bottom,
        }],
        TerminalDisplayCursorStyle::Block => vec![cell_rect],
        TerminalDisplayCursorStyle::Underline => vec![RECT {
            left: cell_rect.left,
            top: (cell_rect.bottom - thickness).max(cell_rect.top),
            right: cell_rect.right,
            bottom: cell_rect.bottom,
        }],
        TerminalDisplayCursorStyle::BlockHollow => vec![
            RECT {
                left: cell_rect.left,
                top: cell_rect.top,
                right: cell_rect.right,
                bottom: (cell_rect.top + thickness).min(cell_rect.bottom),
            },
            RECT {
                left: cell_rect.left,
                top: (cell_rect.bottom - thickness).max(cell_rect.top),
                right: cell_rect.right,
                bottom: cell_rect.bottom,
            },
            RECT {
                left: cell_rect.left,
                top: cell_rect.top,
                right: (cell_rect.left + thickness).min(cell_rect.right),
                bottom: cell_rect.bottom,
            },
            RECT {
                left: (cell_rect.right - thickness).max(cell_rect.left),
                top: cell_rect.top,
                right: cell_rect.right,
                bottom: cell_rect.bottom,
            },
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

fn inset_rect(rect: RECT, amount: i32) -> RECT {
    RECT {
        left: rect.left + amount,
        top: rect.top + amount,
        right: (rect.right - amount).max(rect.left + amount),
        bottom: (rect.bottom - amount).max(rect.top + amount),
    }
}

fn measure_terminal_cell_size(font_height: i32) -> eyre::Result<(i32, i32)> {
    let font_definition = terminal_font_definition(font_height);
    let font = unsafe { CreateFontIndirectW(&font_definition) };
    if font.0.is_null() {
        eyre::bail!("failed to create terminal font")
    }
    let font = FontHandle(font);

    let hdc = unsafe { GetDC(None) };
    if hdc.0.is_null() {
        eyre::bail!("failed to acquire screen DC for font metrics")
    }

    let previous_font = unsafe { SelectObject(hdc, font.0.into()) };
    let glyph = ['W' as u16];
    let mut size = SIZE::default();
    let measured = unsafe { GetTextExtentPoint32W(hdc, &glyph, &mut size) }.as_bool();
    let _ = unsafe { SelectObject(hdc, previous_font) };
    unsafe {
        ReleaseDC(None, hdc);
    }

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

fn handle_mouse_wheel(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> eyre::Result<bool> {
    // cli[impl window.interaction.zoom.terminal]
    // cli[impl window.interaction.zoom.output]
    let ctrl_down = unsafe { (GetKeyState(i32::from(VK_CONTROL.0)) & (0x8000_u16 as i16)) != 0 };
    if !ctrl_down {
        return Ok(false);
    }

    with_app_state(|state| {
        let layout = client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?;
        let point = screen_to_client_point(hwnd, lparam)?;
        let in_terminal = point_in_rect(point, layout.terminal_rect());
        let in_output = point_in_rect(point, layout.result_panel_rect());
        if !in_terminal && !in_output {
            return Ok(false);
        }

        let wheel_delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
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
        state.output_font_height = next_font_height;
        state.output_cell_width = cell_width;
        state.output_cell_height = cell_height;
        Ok(true)
    })
}

fn client_layout(hwnd: HWND, cell_width: i32, cell_height: i32) -> eyre::Result<TerminalLayout> {
    let mut rect = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut rect) }.is_err() {
        eyre::bail!("failed to query client rect")
    }
    Ok(TerminalLayout {
        client_width: rect.right - rect.left,
        client_height: rect.bottom - rect.top,
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

fn handle_left_button_up(hwnd: HWND, lparam: LPARAM) -> eyre::Result<bool> {
    with_app_state(|state| {
        if state.pending_window_drag.take().is_some() {
            return Ok(true);
        }

        let Some(workspace_window) = state.workspace_window.clone() else {
            return Ok(false);
        };

        let layout = client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?;
        let point = POINT {
            x: extract_signed_coordinate(lparam.0),
            y: extract_signed_coordinate(lparam.0 >> 16),
        };
        if !point_in_rect(point, layout.plus_button_rect()) {
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

fn handle_left_button_down(hwnd: HWND, lparam: LPARAM) -> eyre::Result<bool> {
    let point = POINT {
        x: extract_signed_coordinate(lparam.0),
        y: extract_signed_coordinate(lparam.0 >> 16),
    };
    let in_drag_handle = hit_test_drag_handle_point(hwnd, point)?;

    with_app_state(|state| {
        state.pending_window_drag = None;
        if !in_drag_handle {
            return Ok(false);
        }

        state.pending_window_drag = Some(PendingWindowDrag { origin: point });
        Ok(true)
    })
}

fn handle_mouse_move(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> eyre::Result<bool> {
    let point = POINT {
        x: extract_signed_coordinate(lparam.0),
        y: extract_signed_coordinate(lparam.0 >> 16),
    };

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

fn update_pending_drag_action(
    pending_drag: PendingWindowDrag,
    point: POINT,
    left_button_down: bool,
    threshold_x: i32,
    threshold_y: i32,
) -> PendingDragAction {
    if !left_button_down {
        return PendingDragAction::NotHandled;
    }

    if !drag_threshold_exceeded(
        pending_drag.origin,
        point,
        threshold_x,
        threshold_y,
    ) {
        return PendingDragAction::Consumed;
    }

    PendingDragAction::StartSystemDrag
}

fn hit_test_drag_handle_point(hwnd: HWND, point: POINT) -> eyre::Result<bool> {
    with_app_state(|state| {
        let layout = client_layout(hwnd, state.terminal_cell_width, state.terminal_cell_height)?;
        Ok(point_in_rect(point, layout.drag_handle_rect()))
    })
}

fn begin_system_window_drag(hwnd: HWND, client_point: POINT) -> eyre::Result<()> {
    let screen_point = client_to_screen_point(hwnd, client_point)?;
    let (wparam, lparam) = system_drag_message(screen_point);
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_NCLBUTTONDOWN, wparam, lparam);
    }
    Ok(())
}

fn system_drag_message(screen_point: POINT) -> (WPARAM, LPARAM) {
    (
        WPARAM(usize::try_from(HTCAPTION).expect("HTCAPTION fits in usize")),
        LPARAM(pack_point_lparam(screen_point)),
    )
}

fn screen_to_client_point(hwnd: HWND, lparam: LPARAM) -> eyre::Result<POINT> {
    let screen_point = POINT {
        x: extract_signed_coordinate(lparam.0),
        y: extract_signed_coordinate(lparam.0 >> 16),
    };
    screen_to_client_point_from_screen(hwnd, screen_point)
}

fn cursor_client_point(hwnd: HWND) -> eyre::Result<POINT> {
    let mut screen_point = POINT::default();
    if unsafe { GetCursorPos(&mut screen_point) }.is_err() {
        eyre::bail!("failed to query cursor position")
    }
    screen_to_client_point_from_screen(hwnd, screen_point)
}

fn screen_to_client_point_from_screen(hwnd: HWND, screen_point: POINT) -> eyre::Result<POINT> {
    let mut window_rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut window_rect) }.is_err() {
        eyre::bail!("failed to query window rect")
    }

    Ok(POINT {
        x: screen_point.x - window_rect.left,
        y: screen_point.y - window_rect.top,
    })
}

fn client_to_screen_point(hwnd: HWND, client_point: POINT) -> eyre::Result<POINT> {
    let mut window_rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut window_rect) }.is_err() {
        eyre::bail!("failed to query window rect")
    }

    Ok(POINT {
        x: window_rect.left + client_point.x,
        y: window_rect.top + client_point.y,
    })
}

fn handle_set_cursor(hwnd: HWND, lparam: LPARAM) -> eyre::Result<bool> {
    if !should_override_drag_cursor(with_app_state(|state| Ok(state.in_move_size_loop))?) {
        return Ok(false);
    }

    let hit_test_code = (lparam.0 & 0xFFFF) as u32;
    if hit_test_code != HTCAPTION && hit_test_code != HTCLIENT {
        return Ok(false);
    }

    let point = cursor_client_point(hwnd)?;
    if !hit_test_drag_handle_point(hwnd, point)? {
        return Ok(false);
    }

    let move_cursor = unsafe { LoadCursorW(None, IDC_SIZEALL).unwrap_or_default() };
    unsafe {
        SetCursor(Some(move_cursor));
    }
    Ok(true)
}

fn hit_test_resize_border(hwnd: HWND, point: POINT) -> eyre::Result<Option<LRESULT>> {
    let mut client_rect = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut client_rect) }.is_err() {
        eyre::bail!("failed to query client rect for hit testing")
    }

    let resize_border_x = resize_border_thickness(SM_CXSIZEFRAME);
    let resize_border_y = resize_border_thickness(SM_CYSIZEFRAME);
    let hit = classify_resize_border_hit(client_rect, point, resize_border_x, resize_border_y);

    Ok(hit.map(|code| LRESULT(isize::try_from(code).expect("hit-test code fits in isize"))))
}

fn resize_border_thickness(size_frame_metric: SYSTEM_METRICS_INDEX) -> i32 {
    let padded_border = unsafe { GetSystemMetrics(SM_CXPADDEDBORDER) };
    let size_frame = unsafe { GetSystemMetrics(size_frame_metric) };
    (size_frame + padded_border).max(MIN_RESIZE_BORDER_THICKNESS)
}

fn point_in_rect(point: POINT, rect: RECT) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

fn fail_and_close(hwnd: HWND, error: eyre::Error) -> LRESULT {
    tracing::error!(?error, "terminal window failed");
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
    LRESULT(0)
}

fn extract_signed_coordinate(value: isize) -> i32 {
    (value as i16) as i32
}

fn pack_point_lparam(point: POINT) -> isize {
    let x = (point.x as u16) as u32;
    let y = (point.y as u16) as u32;
    ((y << 16) | x) as isize
}

fn drag_threshold_exceeded(
    origin: POINT,
    current: POINT,
    threshold_x: i32,
    threshold_y: i32,
) -> bool {
    if threshold_x <= 0 || threshold_y <= 0 {
        return true;
    }

    (current.x - origin.x).abs() >= threshold_x || (current.y - origin.y).abs() >= threshold_y
}

fn wide_null_terminated(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_border_prefers_top_left_corner() {
        let client_rect = RECT {
            left: 0,
            top: 0,
            right: 400,
            bottom: 300,
        };
        let point = POINT { x: 2, y: 3 };

        let hit = classify_resize_border_hit(client_rect, point, 8, 8);

        assert_eq!(hit, Some(HTTOPLEFT));
    }

    #[test]
    fn resize_border_prefers_bottom_right_corner() {
        let client_rect = RECT {
            left: 0,
            top: 0,
            right: 400,
            bottom: 300,
        };
        let point = POINT { x: 399, y: 299 };

        let hit = classify_resize_border_hit(client_rect, point, 8, 8);

        assert_eq!(hit, Some(HTBOTTOMRIGHT));
    }

    #[test]
    fn resize_border_ignores_interior_points() {
        let client_rect = RECT {
            left: 0,
            top: 0,
            right: 400,
            bottom: 300,
        };
        let point = POINT { x: 200, y: 120 };

        let hit = classify_resize_border_hit(client_rect, point, 8, 8);

        assert_eq!(hit, None);
    }

    #[test]
    fn hollow_cursor_builds_four_border_rects() {
        let rects = terminal_cursor_overlay_rects(
            RECT {
                left: 10,
                top: 20,
                right: 18,
                bottom: 36,
            },
            TerminalDisplayCursorStyle::BlockHollow,
        );

        assert_eq!(rects.len(), 4);
        assert_eq!(rects[0].top, 20);
        assert_eq!(rects[1].bottom, 36);
        assert_eq!(rects[2].left, 10);
        assert_eq!(rects[3].right, 18);
    }

    #[test]
    fn drag_cursor_override_is_disabled_during_native_move_size() {
        assert!(should_override_drag_cursor(false));
        assert!(!should_override_drag_cursor(true));
    }

    #[test]
    fn zero_drag_threshold_has_no_deadzone() {
        assert!(drag_threshold_exceeded(
            POINT { x: 10, y: 20 },
            POINT { x: 10, y: 20 },
            0,
            0,
        ));
    }

    #[test]
    fn positive_drag_threshold_requires_real_motion() {
        assert!(!drag_threshold_exceeded(
            POINT { x: 10, y: 20 },
            POINT { x: 10, y: 20 },
            1,
            1,
        ));
    }

    #[test]
    fn drag_threshold_starts_native_drag_after_one_pixel_of_motion() {
        assert!(drag_threshold_exceeded(
            POINT { x: 10, y: 20 },
            POINT { x: 11, y: 20 },
            1,
            1,
        ));
        assert!(drag_threshold_exceeded(
            POINT { x: 10, y: 20 },
            POINT { x: 10, y: 21 },
            1,
            1,
        ));
    }

    #[test]
    fn pending_drag_is_consumed_before_threshold_is_crossed() {
        let action = update_pending_drag_action(
            PendingWindowDrag {
                origin: POINT { x: 10, y: 20 },
            },
            POINT { x: 10, y: 20 },
            true,
            1,
            1,
        );

        assert_eq!(action, PendingDragAction::Consumed);
        assert!(!action.clears_pending_drag());
    }

    #[test]
    fn pending_drag_starts_immediately_when_threshold_is_zero() {
        let action = update_pending_drag_action(
            PendingWindowDrag {
                origin: POINT { x: 10, y: 20 },
            },
            POINT { x: 10, y: 20 },
            true,
            0,
            0,
        );

        assert_eq!(action, PendingDragAction::StartSystemDrag);
        assert!(action.clears_pending_drag());
    }

    #[test]
    fn pending_drag_requests_native_drag_after_threshold_is_crossed() {
        let action = update_pending_drag_action(
            PendingWindowDrag {
                origin: POINT { x: 10, y: 20 },
            },
            POINT { x: 11, y: 20 },
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
                origin: POINT { x: 10, y: 20 },
            },
            POINT { x: 10, y: 20 },
            false,
            DRAG_START_THRESHOLD_PX,
            DRAG_START_THRESHOLD_PX,
        );

        assert_eq!(action, PendingDragAction::NotHandled);
        assert!(action.clears_pending_drag());
    }

    #[test]
    fn system_drag_message_targets_caption_with_screen_coordinates() {
        let (wparam, lparam) = system_drag_message(POINT { x: 300, y: 400 });

        assert_eq!(wparam.0, usize::try_from(HTCAPTION).unwrap());
        assert_eq!(lparam.0, pack_point_lparam(POINT { x: 300, y: 400 }));
    }

    #[test]
    fn timer_render_path_stays_active_only_during_move_size() {
        assert!(!should_render_from_poll_timer(false));
        assert!(should_render_from_poll_timer(true));
    }

    #[test]
    fn block_cursor_overlay_is_translucent() {
        let color = terminal_cursor_overlay_color(
            [0.8, 0.9, 1.0, 1.0],
            TerminalDisplayCursorStyle::Block,
        );

        assert_eq!(color, [0.8, 0.9, 1.0, 0.42]);
    }
}

fn should_override_drag_cursor(in_move_size_loop: bool) -> bool {
    !in_move_size_loop
}

fn should_render_from_poll_timer(in_move_size_loop: bool) -> bool {
    in_move_size_loop
}

fn classify_resize_border_hit(
    client_rect: RECT,
    point: POINT,
    resize_border_x: i32,
    resize_border_y: i32,
) -> Option<u32> {
    let left = point.x < client_rect.left + resize_border_x;
    let right = point.x >= client_rect.right - resize_border_x;
    let top = point.y < client_rect.top + resize_border_y;
    let bottom = point.y >= client_rect.bottom - resize_border_y;

    if top && left {
        Some(HTTOPLEFT)
    } else if top && right {
        Some(HTTOPRIGHT)
    } else if bottom && left {
        Some(HTBOTTOMLEFT)
    } else if bottom && right {
        Some(HTBOTTOMRIGHT)
    } else if left {
        Some(HTLEFT)
    } else if right {
        Some(HTRIGHT)
    } else if top {
        Some(HTTOP)
    } else if bottom {
        Some(HTBOTTOM)
    } else {
        None
    }
}
