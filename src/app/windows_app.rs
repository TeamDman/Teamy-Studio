use std::cell::RefCell;

use eyre::Context;
use teamy_windows::module::get_current_module;
use tracing::info;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CLEARTYPE_QUALITY, CreateCompatibleBitmap, CreateCompatibleDC,
    CreateFontIndirectW, DeleteDC, DeleteObject, EndPaint, GetDC, GetTextExtentPoint32W,
    HBITMAP, HDC, HGDIOBJ, HFONT, InvalidateRect, LOGFONTW, PAINTSTRUCT, ReleaseDC, SRCCOPY,
    SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetClientRect, GetSystemMetrics, GetWindowRect, HTCAPTION, HTCLIENT, IDC_ARROW, LWA_ALPHA,
    LoadCursorW, MSG, PostQuitMessage, RegisterClassExW, SM_CXSCREEN, SM_CYSCREEN, SW_SHOW,
    SetLayeredWindowAttributes, SetTimer, ShowWindow, TranslateMessage, WM_CHAR, WM_DESTROY,
    WM_ERASEBKGND, WM_KEYDOWN, WM_NCHITTEST, WM_PAINT, WM_SIZE, WM_TIMER, WNDCLASSEXW,
    WS_EX_APPWINDOW, WS_EX_LAYERED, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_THICKFRAME,
    WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

use super::windows_terminal::{
    DRAG_STRIP_HEIGHT, POLL_INTERVAL_MS, POLL_TIMER_ID, TerminalLayout, TerminalSession,
    WINDOW_ALPHA, keyboard_mods,
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
///
/// # Errors
///
/// This function will return an error if the window class, font, terminal session, or message loop fails.
pub fn run() -> eyre::Result<()> {
    let (font, cell_width, cell_height) = create_terminal_font()?;
    let terminal = TerminalSession::new()?;

    APP_STATE.with(|state| {
        *state.borrow_mut() = Some(AppState {
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
        eyre::bail!("failed to register terminal window class")
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

    unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), WINDOW_ALPHA, LWA_ALPHA) }
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

extern "system" fn window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
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
        WM_TIMER if wparam.0 == POLL_TIMER_ID => match with_app_state(|state| state.terminal.pump()) {
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
        },
        WM_CHAR => match with_app_state(|state| state.terminal.handle_char(wparam.0 as u32)) {
            Ok(consumed) => {
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
        WM_KEYDOWN => match with_app_state(|state| {
            let was_down = ((lparam.0 >> 30) & 1) != 0;
            state.terminal.handle_keydown(wparam.0 as u32, was_down, keyboard_mods())
        }) {
            Ok(consumed) => {
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
        WM_NCHITTEST => {
            let default_hit = unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
            if default_hit.0 != isize::try_from(HTCLIENT).expect("HTCLIENT fits in isize") {
                return default_hit;
            }

            let y = extract_signed_coordinate(lparam.0 >> 16);
            let mut window_rect = RECT::default();
            let got_rect = unsafe { GetWindowRect(hwnd, &mut window_rect) }.is_ok();
            if got_rect && y - window_rect.top < DRAG_STRIP_HEIGHT {
                return LRESULT(isize::try_from(HTCAPTION).expect("HTCAPTION fits in isize"));
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
    let mut paint = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
    if hdc.0.is_null() {
        eyre::bail!("failed to begin painting")
    }

    let result = with_app_state(|state| {
        let layout = client_layout(hwnd, state.cell_width, state.cell_height)?;
        let buffer_dc = create_memory_dc(hdc)?;
        let buffer_bitmap = create_compatible_bitmap(hdc, layout.client_width, layout.client_height)?;
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

        let overlay = format!(
            "Teamy Studio  |  drag top strip to move  |  {}x{}",
            state.terminal.cols(),
            state.terminal.rows()
        );
        state.terminal.paint(buffer_dc.0, layout, &overlay)?;

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
    for (slot, value) in font.lfFaceName.iter_mut().zip("Cascadia Mono".encode_utf16()) {
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

fn with_app_state<T>(f: impl FnOnce(&mut AppState) -> eyre::Result<T>) -> eyre::Result<T> {
    APP_STATE.with(|state| {
        let mut borrowed = state.borrow_mut();
        let app_state = borrowed
            .as_mut()
            .ok_or_else(|| eyre::eyre!("application state was not initialized"))?;
        f(app_state)
    })
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