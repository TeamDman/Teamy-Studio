use std::cell::RefCell;
use std::path::Path;
use std::thread;

use eyre::Context;
use teamy_windows::module::get_current_module;
use tracing::{debug, error, info};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CLEARTYPE_QUALITY, CreateCompatibleBitmap, CreateCompatibleDC,
    CreateFontIndirectW, DeleteDC, DeleteObject, EndPaint, GetDC, GetTextExtentPoint32W, HBITMAP,
    HDC, HFONT, HGDIOBJ, InvalidateRect, LOGFONTW, PAINTSTRUCT, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    GetSystemMetrics, GetWindowRect, HTCAPTION, HTCLIENT, IDC_ARROW, LWA_ALPHA, LoadCursorW, MSG,
    PostQuitMessage, RegisterClassExW, SM_CXSCREEN, SM_CYSCREEN, SW_SHOW,
    SetLayeredWindowAttributes, SetTimer, ShowWindow, TranslateMessage, WM_CHAR, WM_DESTROY,
    WM_ERASEBKGND, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONUP, WM_NCHITTEST, WM_PAINT, WM_SIZE,
    WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WNDCLASSEXW, WS_EX_APPWINDOW, WS_EX_LAYERED,
    WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_THICKFRAME, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

use crate::paths::AppHome;

use super::WorkspaceWindowState;
use super::windows_background::PANEL_WINDOW_ALPHA;
use super::windows_terminal::{
    CellChrome, POLL_INTERVAL_MS, POLL_TIMER_ID, TerminalLayout, TerminalSession, keyboard_mods,
};

const WINDOW_CLASS_NAME: PCWSTR = w!("TeamyStudioTerminalWindow");
const WINDOW_TITLE: &str = "Teamy Studio Terminal";
const FONT_HEIGHT: i32 = -20;
const INITIAL_WINDOW_WIDTH: i32 = 1040;
const INITIAL_WINDOW_HEIGHT: i32 = 680;

thread_local! {
    static APP_STATE: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

struct AppState {
    app_home: AppHome,
    workspace_window: Option<WorkspaceWindowState>,
    font: FontHandle,
    cell_width: i32,
    cell_height: i32,
    terminal: TerminalSession,
}

struct FontHandle(HFONT);

struct MemoryDc(HDC);

struct BitmapHandle(HBITMAP);

struct SelectObjectGuard {
    hdc: HDC,
    object: HGDIOBJ,
}

impl Drop for FontHandle {
    fn drop(&mut self) {
        let _ = unsafe { DeleteObject(self.0.into()) };
    }
}

impl Drop for MemoryDc {
    fn drop(&mut self) {
        let _ = unsafe { DeleteDC(self.0) };
    }
}

impl Drop for BitmapHandle {
    fn drop(&mut self) {
        let _ = unsafe { DeleteObject(self.0.into()) };
    }
}

impl Drop for SelectObjectGuard {
    fn drop(&mut self) {
        let _ = unsafe { SelectObject(self.hdc, self.object) };
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
    let (font, cell_width, cell_height) = create_terminal_font()?;
    let terminal = TerminalSession::new(app_home, working_dir)?;

    APP_STATE.with(|state| {
        *state.borrow_mut() = Some(AppState {
            app_home: app_home.clone(),
            workspace_window,
            font,
            cell_width,
            cell_height,
            terminal,
        });
    });

    let hwnd = create_window()?;
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
    }

    with_app_state(|state| {
        let layout = client_layout(hwnd, state.cell_width, state.cell_height)?;
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
            WS_EX_APPWINDOW | WS_EX_LAYERED,
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

    unsafe { SetLayeredWindowAttributes(hwnd, Default::default(), PANEL_WINDOW_ALPHA, LWA_ALPHA) }
        .wrap_err("failed to enable layered window alpha")?;

    let timer = unsafe { SetTimer(Some(hwnd), POLL_TIMER_ID, POLL_INTERVAL_MS, None) };
    if timer == 0 {
        eyre::bail!("failed to start terminal poll timer")
    }

    Ok(hwnd)
}

fn message_loop() -> eyre::Result<()> {
    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
        let value = result.0;
        if value == -1 {
            eyre::bail!("message loop failed")
        }
        if value == 0 {
            break;
        }

        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_SIZE => match with_app_state(|state| {
            let layout = client_layout(hwnd, state.cell_width, state.cell_height)?;
            state.terminal.resize(layout)
        }) {
            Ok(()) => {
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            Err(error) => fail_and_close(hwnd, error),
        },
        WM_TIMER if wparam.0 == POLL_TIMER_ID => {
            match with_app_state(|state| state.terminal.pump()) {
                Ok(result) => {
                    if result.needs_repaint {
                        unsafe {
                            let _ = InvalidateRect(Some(hwnd), None, false);
                        }
                    }
                    if result.should_close {
                        unsafe {
                            let _ = DestroyWindow(hwnd);
                        }
                    }
                    LRESULT(0)
                }
                Err(error) => fail_and_close(hwnd, error),
            }
        }
        WM_CHAR => {
            // cli[impl window.interaction.input]
            match with_app_state(|state| state.terminal.handle_char(wparam.0 as u32, lparam.0)) {
                Ok(consumed) => {
                    debug!(
                        message = "WM_CHAR",
                        code_unit = wparam.0 as u32,
                        lparam = lparam.0,
                        consumed,
                        "processed keyboard char message"
                    );
                    if consumed {
                        unsafe {
                            let _ = InvalidateRect(Some(hwnd), None, false);
                        }
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
                    unsafe {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
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
                    unsafe {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                    return LRESULT(0);
                }
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            Err(error) => fail_and_close(hwnd, error),
        },
        WM_PAINT => match paint_window(hwnd) {
            Ok(()) => LRESULT(0),
            Err(error) => fail_and_close(hwnd, error),
        },
        WM_LBUTTONUP => match handle_left_button_up(hwnd, lparam) {
            Ok(handled) => {
                if handled {
                    unsafe {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                    LRESULT(0)
                } else {
                    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
                }
            }
            Err(error) => fail_and_close(hwnd, error),
        },
        WM_NCHITTEST => {
            // cli[impl window.interaction.drag]
            let default_hit = unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
            if default_hit.0 != isize::try_from(HTCLIENT).expect("HTCLIENT fits in isize") {
                return default_hit;
            }

            match hit_test_drag_handle(hwnd, lparam) {
                Ok(true) => {
                    return LRESULT(isize::try_from(HTCAPTION).expect("HTCAPTION fits in isize"));
                }
                Ok(false) => {}
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

fn paint_window(hwnd: HWND) -> eyre::Result<()> {
    // cli[impl window.appearance.chrome]
    let mut paint = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
    if hdc.0.is_null() {
        eyre::bail!("failed to begin painting")
    }

    let result = with_app_state(|state| {
        let layout = client_layout(hwnd, state.cell_width, state.cell_height)?;
        let buffer_dc = create_memory_dc(hdc)?;
        let buffer_bitmap =
            create_compatible_bitmap(hdc, layout.client_width, layout.client_height)?;
        let previous_bitmap = unsafe { SelectObject(buffer_dc.0, buffer_bitmap.0.into()) };
        let _bitmap_guard = SelectObjectGuard {
            hdc: buffer_dc.0,
            object: previous_bitmap,
        };

        let previous_font = unsafe { SelectObject(buffer_dc.0, state.font.0.into()) };
        let _font_guard = SelectObjectGuard {
            hdc: buffer_dc.0,
            object: previous_font,
        };

        let chrome = cell_chrome(state);
        state.terminal.paint(buffer_dc.0, layout, &chrome)?;

        unsafe {
            BitBlt(
                hdc,
                0,
                0,
                layout.client_width.max(1),
                layout.client_height.max(1),
                Some(buffer_dc.0),
                0,
                0,
                SRCCOPY,
            )
        }
        .wrap_err("failed to copy buffered frame to window")?;

        Ok(())
    });

    unsafe {
        let _ = EndPaint(hwnd, &paint);
    }
    result
}

fn create_memory_dc(hdc: HDC) -> eyre::Result<MemoryDc> {
    let memory_dc = unsafe { CreateCompatibleDC(Some(hdc)) };
    if memory_dc.0.is_null() {
        eyre::bail!("failed to create memory device context")
    }
    Ok(MemoryDc(memory_dc))
}

fn create_compatible_bitmap(hdc: HDC, width: i32, height: i32) -> eyre::Result<BitmapHandle> {
    let bitmap = unsafe { CreateCompatibleBitmap(hdc, width.max(1), height.max(1)) };
    if bitmap.0.is_null() {
        eyre::bail!("failed to create offscreen bitmap")
    }
    Ok(BitmapHandle(bitmap))
}

fn create_terminal_font() -> eyre::Result<(FontHandle, i32, i32)> {
    let font_definition = terminal_font_definition();
    let font = unsafe { CreateFontIndirectW(&font_definition) };
    if font.0.is_null() {
        eyre::bail!("failed to create terminal font")
    }

    let hdc = unsafe { GetDC(None) };
    if hdc.0.is_null() {
        eyre::bail!("failed to acquire screen DC for font metrics")
    }

    let previous_font = unsafe { SelectObject(hdc, font.into()) };
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

    Ok((FontHandle(font), size.cx.max(8), size.cy.max(16)))
}

fn terminal_font_definition() -> LOGFONTW {
    let mut font = LOGFONTW {
        lfHeight: FONT_HEIGHT,
        lfQuality: CLEARTYPE_QUALITY,
        ..Default::default()
    };
    for (slot, value) in font
        .lfFaceName
        .iter_mut()
        .zip("Cascadia Mono".encode_utf16())
    {
        *slot = value;
    }
    font
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

fn cell_chrome(state: &AppState) -> CellChrome {
    let result_text = if let Some(workspace_window) = &state.workspace_window {
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
    };

    CellChrome {
        cell_number: state
            .workspace_window
            .as_ref()
            .map_or(1, |workspace_window| workspace_window.cell_number),
        result_text,
    }
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
        let Some(workspace_window) = state.workspace_window.clone() else {
            return Ok(false);
        };

        let layout = client_layout(hwnd, state.cell_width, state.cell_height)?;
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

fn hit_test_drag_handle(hwnd: HWND, lparam: LPARAM) -> eyre::Result<bool> {
    with_app_state(|state| {
        let layout = client_layout(hwnd, state.cell_width, state.cell_height)?;
        let point = screen_to_client_point(hwnd, lparam)?;
        Ok(point_in_rect(point, layout.drag_handle_rect()))
    })
}

fn screen_to_client_point(hwnd: HWND, lparam: LPARAM) -> eyre::Result<POINT> {
    let mut window_rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut window_rect) }.is_err() {
        eyre::bail!("failed to query window rect")
    }

    let point = POINT {
        x: extract_signed_coordinate(lparam.0),
        y: extract_signed_coordinate(lparam.0 >> 16),
    };

    Ok(POINT {
        x: point.x - window_rect.left,
        y: point.y - window_rect.top,
    })
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

fn wide_null_terminated(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
