use eyre::Context;
use teamy_windows::module::get_current_module;
use teamy_windows::string::EasyPCWSTR;
use tracing::info;
use windows::Win32::Foundation::COLORREF;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::LRESULT;
use windows::Win32::Foundation::POINT;
use windows::Win32::Foundation::RECT;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::Graphics::Gdi::BeginPaint;
use windows::Win32::Graphics::Gdi::CreateSolidBrush;
use windows::Win32::Graphics::Gdi::DeleteObject;
use windows::Win32::Graphics::Gdi::EndPaint;
use windows::Win32::Graphics::Gdi::FillRect;
use windows::Win32::Graphics::Gdi::GetMonitorInfoW;
use windows::Win32::Graphics::Gdi::MONITOR_DEFAULTTONEAREST;
use windows::Win32::Graphics::Gdi::MONITORINFO;
use windows::Win32::Graphics::Gdi::MonitorFromPoint;
use windows::Win32::Graphics::Gdi::PAINTSTRUCT;
use windows::Win32::UI::WindowsAndMessaging::CreateWindowExW;
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;
use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
use windows::Win32::UI::WindowsAndMessaging::DispatchMessageW;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows::Win32::UI::WindowsAndMessaging::GetMessageW;
use windows::Win32::UI::WindowsAndMessaging::HTCAPTION;
use windows::Win32::UI::WindowsAndMessaging::IDC_SIZEALL;
use windows::Win32::UI::WindowsAndMessaging::LWA_ALPHA;
use windows::Win32::UI::WindowsAndMessaging::LoadCursorW;
use windows::Win32::UI::WindowsAndMessaging::MSG;
use windows::Win32::UI::WindowsAndMessaging::PostQuitMessage;
use windows::Win32::UI::WindowsAndMessaging::RegisterClassExW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOW;
use windows::Win32::UI::WindowsAndMessaging::SetCursor;
use windows::Win32::UI::WindowsAndMessaging::SetLayeredWindowAttributes;
use windows::Win32::UI::WindowsAndMessaging::ShowWindow;
use windows::Win32::UI::WindowsAndMessaging::TranslateMessage;
use windows::Win32::UI::WindowsAndMessaging::WM_CLOSE;
use windows::Win32::UI::WindowsAndMessaging::WM_DESTROY;
use windows::Win32::UI::WindowsAndMessaging::WM_KEYDOWN;
use windows::Win32::UI::WindowsAndMessaging::WM_NCHITTEST;
use windows::Win32::UI::WindowsAndMessaging::WM_PAINT;
use windows::Win32::UI::WindowsAndMessaging::WM_SETCURSOR;
use windows::Win32::UI::WindowsAndMessaging::WNDCLASSEXW;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_APPWINDOW;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_LAYERED;
use windows::Win32::UI::WindowsAndMessaging::WS_POPUP;
use windows::Win32::UI::WindowsAndMessaging::WS_VISIBLE;
use windows::core::w;

const WINDOW_CLASS_NAME: windows::core::PCWSTR = w!("TeamyStudioWindow");
const WINDOW_ALPHA: u8 = 128;
const WINDOW_TITLE: &str = "Teamy Studio";
const WINDOW_RED: COLORREF = COLORREF(0x0000_00FF);

fn windows_struct_size_u32<T>() -> u32 {
    u32::try_from(std::mem::size_of::<T>()).expect("Windows ABI struct size must fit in u32")
}

/// Launch the Teamy Studio window and block until it closes.
///
/// # Errors
///
/// This function will return an error if the window class, monitor query, or message loop fails.
pub fn run() -> eyre::Result<()> {
    let hwnd = create_window()?;

    // Safety: `hwnd` is a valid window returned by `CreateWindowExW` and remains valid for the
    // duration of this startup sequence.
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
    }

    info!("Teamy Studio window shown");
    message_loop()
}

fn create_window() -> eyre::Result<HWND> {
    let instance = get_current_module()?;

    let window_class = WNDCLASSEXW {
        cbSize: windows_struct_size_u32::<WNDCLASSEXW>(),
        lpfnWndProc: Some(window_proc),
        hInstance: instance.into(),
        hCursor: {
            // Safety: Loading a shared system cursor by resource id does not transfer ownership.
            unsafe { LoadCursorW(None, IDC_SIZEALL)? }
        },
        lpszClassName: WINDOW_CLASS_NAME,
        ..Default::default()
    };

    // Safety: `window_class` points to a fully initialized window class structure for the current module.
    let atom = unsafe { RegisterClassExW(&raw const window_class) };
    if atom == 0 {
        info!("Teamy Studio window class already registered");
    }

    let initial_rect = initial_window_rect()?;
    let title = WINDOW_TITLE.easy_pcwstr()?;

    // Safety: The window class is registered, the title buffer outlives this call, and the position
    // and size are derived from the active monitor bounds.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW | WS_EX_LAYERED,
            WINDOW_CLASS_NAME,
            title.as_ref(),
            WS_POPUP | WS_VISIBLE,
            initial_rect.left,
            initial_rect.top,
            initial_rect.right - initial_rect.left,
            initial_rect.bottom - initial_rect.top,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
    .wrap_err("Failed to create Teamy Studio window")?;

    // Safety: `hwnd` is the live layered window created above, and setting alpha only adjusts its opacity.
    unsafe {
        SetLayeredWindowAttributes(hwnd, COLORREF(0), WINDOW_ALPHA, LWA_ALPHA)
            .wrap_err("Failed to set Teamy Studio window opacity")?;
    };

    info!(
        x = initial_rect.left,
        y = initial_rect.top,
        width = initial_rect.right - initial_rect.left,
        height = initial_rect.bottom - initial_rect.top,
        "Created Teamy Studio window"
    );

    Ok(hwnd)
}

fn initial_window_rect() -> eyre::Result<RECT> {
    let mut cursor_position = POINT::default();

    // Safety: Windows writes the current cursor location into the provided `POINT` buffer.
    unsafe { GetCursorPos(&raw mut cursor_position) }
        .wrap_err("Failed to query cursor position for Teamy Studio startup")?;

    // Safety: `cursor_position` contains the current screen-space cursor coordinates.
    let monitor = unsafe { MonitorFromPoint(cursor_position, MONITOR_DEFAULTTONEAREST) };
    let mut monitor_info = MONITORINFO {
        cbSize: windows_struct_size_u32::<MONITORINFO>(),
        ..Default::default()
    };

    // Safety: `monitor` came from `MonitorFromPoint`, and Windows writes into the supplied monitor info buffer.
    unsafe { GetMonitorInfoW(monitor, &raw mut monitor_info) }
        .ok()
        .wrap_err("Failed to query monitor bounds for Teamy Studio startup")?;

    let monitor_rect = monitor_info.rcMonitor;
    let monitor_width = monitor_rect.right - monitor_rect.left;
    let monitor_height = monitor_rect.bottom - monitor_rect.top;
    let window_width = monitor_width / 2;
    let window_height = monitor_height / 2;
    let window_left = monitor_rect.left + (monitor_width - window_width) / 2;
    let window_top = monitor_rect.top + (monitor_height - window_height) / 2;

    Ok(RECT {
        left: window_left,
        top: window_top,
        right: window_left + window_width,
        bottom: window_top + window_height,
    })
}

fn message_loop() -> eyre::Result<()> {
    loop {
        let mut message = MSG::default();

        // Safety: Windows writes the next thread message into the supplied `MSG` buffer.
        let status = unsafe { GetMessageW(&raw mut message, None, 0, 0) }.0;
        if status == -1 {
            eyre::bail!("Failed to read Teamy Studio window messages")
        }
        if status == 0 {
            return Ok(());
        }

        // Safety: `message` was populated by `GetMessageW` above.
        let _ = unsafe { TranslateMessage(&raw const message) };
        // Safety: `message` was populated by `GetMessageW` above.
        unsafe { DispatchMessageW(&raw const message) };
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CLOSE => {
            // Safety: `hwnd` is the current top-level Teamy Studio window.
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_DESTROY => {
            // Safety: Posting quit ends this thread's message loop after the window is destroyed.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        WM_KEYDOWN if u32::try_from(wparam.0) == Ok(0x1B) => {
            // Safety: `hwnd` is the current top-level Teamy Studio window.
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_SETCURSOR => {
            // Safety: Loading a shared system cursor by resource id does not transfer ownership.
            if let Ok(cursor) = unsafe { LoadCursorW(None, IDC_SIZEALL) } {
                // Safety: Setting the thread cursor to a shared system cursor is valid for this message.
                let _ = unsafe { SetCursor(Some(cursor)) };
                return LRESULT(1);
            }
            LRESULT(0)
        }
        WM_NCHITTEST => LRESULT(isize::try_from(HTCAPTION).expect("HTCAPTION must fit in isize")),
        WM_PAINT => {
            paint_window(hwnd);
            LRESULT(0)
        }
        _ => {
            // Safety: Delegating unhandled messages back to the default window procedure is required by Win32.
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
    }
}

fn paint_window(hwnd: HWND) {
    let mut paint_struct = PAINTSTRUCT::default();

    // Safety: `hwnd` is the window being painted, and Windows fills `paint_struct` during `BeginPaint`.
    let hdc = unsafe { BeginPaint(hwnd, &raw mut paint_struct) };
    // Safety: Creating a GDI solid brush with a constant RGB value is safe.
    let brush = unsafe { CreateSolidBrush(WINDOW_RED) };
    // Safety: `hdc` and the paint rect come from `BeginPaint`, and `brush` is a valid GDI brush handle.
    let _ = unsafe { FillRect(hdc, &raw const paint_struct.rcPaint, brush) };
    // Safety: `brush` was created by `CreateSolidBrush` in this function and has not been freed yet.
    let _ = unsafe { DeleteObject(brush.into()) };
    // Safety: `paint_struct` was initialized by `BeginPaint` for this `hwnd` and must be paired with `EndPaint`.
    let _ = unsafe { EndPaint(hwnd, &raw const paint_struct) };
}
