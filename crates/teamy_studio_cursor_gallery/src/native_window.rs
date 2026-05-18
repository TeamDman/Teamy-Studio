use std::sync::OnceLock;
use std::thread;

use eyre::{Result, eyre};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, HDC, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetClientRect, GetMessageW, GetWindowRect, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION,
    HTCLIENT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, IDC_ARROW, LoadCursorW, MSG,
    RegisterClassW, SW_MAXIMIZE, SW_MINIMIZE, SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
    ShowWindow, TranslateMessage, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_LBUTTONUP,
    WM_NCCALCSIZE, WM_NCHITTEST, WM_PAINT, WNDCLASSW, WS_EX_APPWINDOW, WS_MAXIMIZEBOX,
    WS_MINIMIZEBOX, WS_POPUP, WS_THICKFRAME, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

use teamy_studio_shell::{
    GlyphQuad, PanelEffect, RenderScene, ShellSceneLayout, WindowChromeButtonsState,
    initialize_dpi_awareness, preferred_background_color, preferred_title_bar_color, push_panel,
    push_text_block, push_title_text, push_window_chrome_buttons, push_window_garden_frame,
    scale_for_dpi, system_dpi, window_dpi,
};

const CURSOR_GALLERY_WINDOW_WIDTH: i32 = 1200;
const CURSOR_GALLERY_WINDOW_HEIGHT: i32 = 820;
const CURSOR_GALLERY_WINDOW_CLASS_NAME: PCWSTR = w!("TeamyStudioCursorGalleryWindow");
const CURSOR_GALLERY_WINDOW_TITLE: PCWSTR = w!("Cursor Gallery");
const RESIZE_BORDER_THICKNESS: i32 = 8;

static CURSOR_GALLERY_WINDOW_CLASS_REGISTERED: OnceLock<()> = OnceLock::new();

unsafe extern "system" fn cursor_gallery_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCALCSIZE => LRESULT(0),
        WM_NCHITTEST => non_client_hit_test(hwnd, lparam),
        WM_ERASEBKGND => LRESULT(1),
        WM_DPICHANGED => handle_dpi_changed(hwnd, lparam),
        WM_DESTROY => LRESULT(0),
        WM_LBUTTONUP => {
            let _ = handle_left_button_up(hwnd, lparam);
            LRESULT(0)
        }
        WM_PAINT => paint_cursor_gallery_window(hwnd),
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn ensure_cursor_gallery_window_class_registered() -> Result<()> {
    if CURSOR_GALLERY_WINDOW_CLASS_REGISTERED.get().is_some() {
        return Ok(());
    }

    let instance = unsafe { GetModuleHandleW(None) }
        .map_err(|error| eyre!("failed to get module handle: {error}"))?;
    let cursor = unsafe { LoadCursorW(Some(HINSTANCE::default()), IDC_ARROW) }
        .map_err(|error| eyre!("failed to load arrow cursor: {error}"))?;

    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        hInstance: instance.into(),
        lpszClassName: CURSOR_GALLERY_WINDOW_CLASS_NAME,
        lpfnWndProc: Some(cursor_gallery_window_proc),
        hCursor: cursor,
        ..Default::default()
    };

    let atom = unsafe { RegisterClassW(&class) };
    if atom == 0 {
        return Err(eyre!("failed to register cursor gallery window class"));
    }

    let _ = CURSOR_GALLERY_WINDOW_CLASS_REGISTERED.set(());
    Ok(())
}

pub fn open_native_cursor_gallery_window() -> Result<()> {
    initialize_dpi_awareness();
    ensure_cursor_gallery_window_class_registered()?;

    let instance = unsafe { GetModuleHandleW(None) }
        .map_err(|error| eyre!("failed to get module handle: {error}"))?;
    let dpi = system_dpi();

    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW,
            CURSOR_GALLERY_WINDOW_CLASS_NAME,
            CURSOR_GALLERY_WINDOW_TITLE,
            WS_POPUP | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_VISIBLE,
            180,
            120,
            scale_for_dpi(CURSOR_GALLERY_WINDOW_WIDTH, dpi),
            scale_for_dpi(CURSOR_GALLERY_WINDOW_HEIGHT, dpi),
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
    .map_err(|error| eyre!("failed to create cursor gallery window: {error}"))?;

    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
    }

    Ok(())
}

pub fn open_native_cursor_gallery_window_on_thread() -> Result<()> {
    thread::Builder::new()
        .name("teamy-cursor-gallery-ui".to_owned())
        .spawn(run_cursor_gallery_window_thread)
        .map_err(|error| eyre!("failed to spawn cursor gallery UI thread: {error}"))?;
    Ok(())
}

fn run_cursor_gallery_window_thread() {
    let hwnd = match open_native_cursor_gallery_window_inner() {
        Ok(hwnd) => hwnd,
        Err(_) => return,
    };

    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
        if result <= 0 {
            break;
        }

        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        if unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(hwnd)) }.as_bool() {
            continue;
        }
        break;
    }
}

fn open_native_cursor_gallery_window_inner() -> Result<HWND> {
    initialize_dpi_awareness();
    ensure_cursor_gallery_window_class_registered()?;

    let instance = unsafe { GetModuleHandleW(None) }
        .map_err(|error| eyre!("failed to get module handle: {error}"))?;
    let dpi = system_dpi();

    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW,
            CURSOR_GALLERY_WINDOW_CLASS_NAME,
            CURSOR_GALLERY_WINDOW_TITLE,
            WS_POPUP | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_VISIBLE,
            180,
            120,
            scale_for_dpi(CURSOR_GALLERY_WINDOW_WIDTH, dpi),
            scale_for_dpi(CURSOR_GALLERY_WINDOW_HEIGHT, dpi),
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
    .map_err(|error| eyre!("failed to create cursor gallery window: {error}"))?;

    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
    }

    Ok(hwnd)
}

fn handle_left_button_up(hwnd: HWND, lparam: LPARAM) -> Result<()> {
    let point = POINT {
        x: (lparam.0 & 0xFFFF) as i16 as i32,
        y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
    };
    let mut client_rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut client_rect) }
        .map_err(|error| eyre!("failed to get client rect for hit testing: {error}"))?;
    let layout = ShellSceneLayout::for_main_menu_with_dpi(
        client_rect.right - client_rect.left,
        client_rect.bottom - client_rect.top,
        window_dpi(hwnd),
    );

    if let Some(action) = chrome_click_action(layout, point) {
        perform_chrome_click(hwnd, action);
    }

    Ok(())
}

fn perform_chrome_click(hwnd: HWND, action: ChromeClickAction) {
    match action {
        ChromeClickAction::Close => unsafe {
            let _ = DestroyWindow(hwnd);
        },
        ChromeClickAction::Minimize => unsafe {
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
        },
        ChromeClickAction::MaximizeRestore => unsafe {
            let _ = ShowWindow(hwnd, SW_MAXIMIZE);
        },
        ChromeClickAction::Pin | ChromeClickAction::Diagnostics | ChromeClickAction::Latency => {}
    }
}

fn paint_cursor_gallery_window(hwnd: HWND) -> LRESULT {
    let mut paint = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
    if !hdc.0.is_null() {
        let _ = render_cursor_gallery_scene(hdc, hwnd);
        unsafe {
            let _ = EndPaint(hwnd, &paint);
        };
    }
    LRESULT(0)
}

fn render_cursor_gallery_scene(hdc: HDC, hwnd: HWND) -> Result<()> {
    let mut client_rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut client_rect) }
        .map_err(|error| eyre!("failed to get client rect for painting: {error}"))?;
    let dpi = window_dpi(hwnd);
    let layout = ShellSceneLayout::for_main_menu_with_dpi(
        client_rect.right - client_rect.left,
        client_rect.bottom - client_rect.top,
        dpi,
    );
    let scene = build_cursor_gallery_placeholder_scene(layout, dpi);

    for panel in &scene.panels {
        draw_panel(hdc, panel)?;
    }
    for glyph in &scene.glyphs {
        draw_glyph(hdc, glyph)?;
    }
    Ok(())
}

fn build_cursor_gallery_placeholder_scene(layout: ShellSceneLayout, dpi: u32) -> RenderScene {
    let mut scene = RenderScene::empty();
    push_panel(
        &mut scene,
        layout.content_frame_rect,
        preferred_background_color(),
        PanelEffect::BlueBackground,
    );
    push_window_garden_frame(&mut scene, layout);
    push_panel(
        &mut scene,
        layout.title_bar_rect,
        preferred_title_bar_color(true),
        PanelEffect::TitleBar,
    );
    push_panel(
        &mut scene,
        RECT {
            left: layout.title_bar_rect.left,
            top: layout.title_bar_rect.bottom - scale_for_dpi(4, dpi),
            right: layout.title_bar_rect.right,
            bottom: layout.title_bar_rect.bottom,
        },
        [0.02, 0.30, 0.18, 0.98],
        PanelEffect::GardenFrame,
    );
    push_panel(
        &mut scene,
        layout.body_rect,
        [0.08, 0.09, 0.11, 0.985],
        PanelEffect::SceneBody,
    );
    let content_rect = inset_rect(layout.body_rect, scale_for_dpi(28, dpi));
    push_panel(
        &mut scene,
        content_rect,
        [0.04, 0.05, 0.07, 0.98],
        PanelEffect::SceneBody,
    );
    push_window_chrome_buttons(
        &mut scene,
        layout,
        WindowChromeButtonsState {
            focused: true,
            ..Default::default()
        },
    );
    push_title_text(
        &mut scene,
        layout.title_text_rect,
        "Cursor Gallery",
        [0.95, 0.95, 0.98, 1.0],
    );

    let headline_rect = RECT {
        left: content_rect.left + scale_for_dpi(22, dpi),
        top: content_rect.top + scale_for_dpi(24, dpi),
        right: content_rect.right - scale_for_dpi(22, dpi),
        bottom: content_rect.top + scale_for_dpi(72, dpi),
    };
    let body_text_rect = RECT {
        left: content_rect.left + scale_for_dpi(22, dpi),
        top: headline_rect.bottom + scale_for_dpi(20, dpi),
        right: content_rect.right - scale_for_dpi(22, dpi),
        bottom: content_rect.bottom - scale_for_dpi(24, dpi),
    };

    push_title_text(
        &mut scene,
        headline_rect,
        "Custom Window Host",
        [0.90, 0.94, 0.99, 1.0],
    );
    push_text_block(
        &mut scene,
        body_text_rect,
        "This window is no longer the shell placeholder. The migrated cursor-gallery renderer is still pending, but this path now uses the same custom chrome and DPI-aware scene layout as the launcher.",
        scale_for_dpi(10, dpi).max(8),
        scale_for_dpi(18, dpi).max(12),
        [0.80, 0.84, 0.90, 1.0],
    );

    scene
}

fn draw_panel(hdc: HDC, panel: &teamy_studio_shell::PanelRect) -> Result<()> {
    fill_rect_with_color(hdc, panel.rect, panel.color)?;
    match panel.effect {
        teamy_studio_shell::PanelEffect::BlueBackground => {
            stroke_rect(hdc, panel.rect, darken(panel.color, 0.32), 1)?;
            stroke_rect(
                hdc,
                inset_rect(panel.rect, 1),
                lighten(panel.color, 0.06),
                1,
            )?;
        }
        teamy_studio_shell::PanelEffect::TitleBar => {
            let bottom_highlight = RECT {
                left: panel.rect.left,
                top: panel.rect.bottom - 1,
                right: panel.rect.right,
                bottom: panel.rect.bottom,
            };
            fill_rect_with_color(hdc, bottom_highlight, [0.02, 0.30, 0.18, 1.0])?;
        }
        teamy_studio_shell::PanelEffect::WindowChromePin
        | teamy_studio_shell::PanelEffect::WindowChromeDiagnostics
        | teamy_studio_shell::PanelEffect::WindowChromeLatency
        | teamy_studio_shell::PanelEffect::WindowChromeMinimize
        | teamy_studio_shell::PanelEffect::WindowChromeMaximize
        | teamy_studio_shell::PanelEffect::WindowChromeRestore
        | teamy_studio_shell::PanelEffect::WindowChromeClose => {
            stroke_rect(hdc, panel.rect, lighten(panel.color, 0.08), 1)?;
            draw_chrome_button_icon(hdc, panel)?;
        }
        _ => {}
    }
    Ok(())
}

fn draw_glyph(hdc: HDC, glyph: &GlyphQuad) -> Result<()> {
    draw_bitmap_glyph(hdc, glyph.rect, glyph.character, glyph.color)
}

fn fill_rect_with_color(hdc: HDC, rect: RECT, color: [f32; 4]) -> Result<()> {
    let brush = unsafe { CreateSolidBrush(rgb(color)) };
    if brush.0.is_null() {
        return Err(eyre!("failed to create brush"));
    }
    unsafe { FillRect(hdc, &rect, brush) };
    let _ = unsafe { DeleteObject(brush.into()) };
    Ok(())
}

fn rgb(color: [f32; 4]) -> COLORREF {
    COLORREF(
        u32::from((color[0].clamp(0.0, 1.0) * 255.0) as u8)
            | (u32::from((color[1].clamp(0.0, 1.0) * 255.0) as u8) << 8)
            | (u32::from((color[2].clamp(0.0, 1.0) * 255.0) as u8) << 16),
    )
}

fn non_client_hit_test(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let screen_point = POINT {
        x: (lparam.0 & 0xFFFF) as i16 as i32,
        y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
    };
    let mut window_rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut window_rect) }.is_err() {
        return LRESULT(isize::try_from(HTCLIENT).unwrap_or_default());
    }
    let client_x = screen_point.x - window_rect.left;
    let client_y = screen_point.y - window_rect.top;
    let width = window_rect.right - window_rect.left;
    let height = window_rect.bottom - window_rect.top;

    if let Some(hit) = resize_hit_test(width, height, client_x, client_y) {
        return LRESULT(isize::try_from(hit).unwrap_or_default());
    }

    let layout = ShellSceneLayout::for_main_menu_with_dpi(width, height, window_dpi(hwnd));
    let client_point = POINT {
        x: client_x,
        y: client_y,
    };
    if chrome_click_action(layout, client_point).is_some() {
        return LRESULT(isize::try_from(HTCLIENT).unwrap_or_default());
    }
    if rect_contains(layout.title_bar_rect, client_x, client_y) {
        return LRESULT(isize::try_from(HTCAPTION).unwrap_or_default());
    }

    LRESULT(isize::try_from(HTCLIENT).unwrap_or_default())
}

fn handle_dpi_changed(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let suggested_rect = unsafe { *(lparam.0 as *const RECT) };
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            suggested_rect.left,
            suggested_rect.top,
            suggested_rect.right - suggested_rect.left,
            suggested_rect.bottom - suggested_rect.top,
            SWP_NOACTIVATE | SWP_NOZORDER,
        );
    }
    LRESULT(0)
}

fn resize_hit_test(width: i32, height: i32, x: i32, y: i32) -> Option<u32> {
    let left = x < RESIZE_BORDER_THICKNESS;
    let right = x >= width - RESIZE_BORDER_THICKNESS;
    let top = y < RESIZE_BORDER_THICKNESS;
    let bottom = y >= height - RESIZE_BORDER_THICKNESS;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChromeClickAction {
    Pin,
    Diagnostics,
    Latency,
    Minimize,
    MaximizeRestore,
    Close,
}

fn chrome_click_action(layout: ShellSceneLayout, point: POINT) -> Option<ChromeClickAction> {
    if rect_contains(layout.pin_button_rect, point.x, point.y) {
        Some(ChromeClickAction::Pin)
    } else if rect_contains(layout.diagnostics_button_rect, point.x, point.y) {
        Some(ChromeClickAction::Diagnostics)
    } else if rect_contains(layout.latency_button_rect, point.x, point.y) {
        Some(ChromeClickAction::Latency)
    } else if rect_contains(layout.minimize_button_rect, point.x, point.y) {
        Some(ChromeClickAction::Minimize)
    } else if rect_contains(layout.maximize_restore_button_rect, point.x, point.y) {
        Some(ChromeClickAction::MaximizeRestore)
    } else if rect_contains(layout.close_button_rect, point.x, point.y) {
        Some(ChromeClickAction::Close)
    } else {
        None
    }
}

fn rect_contains(rect: RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

fn draw_chrome_button_icon(hdc: HDC, panel: &teamy_studio_shell::PanelRect) -> Result<()> {
    let rect = inset_rect(panel.rect, 5);
    let color = match panel.effect {
        teamy_studio_shell::PanelEffect::WindowChromeClose => [0.95, 0.92, 0.94, 1.0],
        _ => [0.84, 0.88, 0.94, 1.0],
    };
    match panel.effect {
        teamy_studio_shell::PanelEffect::WindowChromePin => {
            fill_rect_with_color(
                hdc,
                RECT {
                    left: rect.left + 4,
                    top: rect.top + 2,
                    right: rect.right - 4,
                    bottom: rect.top + 5,
                },
                color,
            )?;
            fill_rect_with_color(
                hdc,
                RECT {
                    left: (rect.left + rect.right) / 2 - 1,
                    top: rect.top + 5,
                    right: (rect.left + rect.right) / 2 + 1,
                    bottom: rect.bottom - 2,
                },
                color,
            )?;
        }
        teamy_studio_shell::PanelEffect::WindowChromeDiagnostics
        | teamy_studio_shell::PanelEffect::WindowChromeLatency => {
            for index in 0..3 {
                let bar = RECT {
                    left: rect.left + 2 + index * 5,
                    top: rect.bottom - 4 - index * 3,
                    right: rect.left + 5 + index * 5,
                    bottom: rect.bottom - 2,
                };
                fill_rect_with_color(hdc, bar, color)?;
            }
        }
        teamy_studio_shell::PanelEffect::WindowChromeMinimize => {
            fill_rect_with_color(
                hdc,
                RECT {
                    left: rect.left + 3,
                    top: rect.bottom - 4,
                    right: rect.right - 3,
                    bottom: rect.bottom - 2,
                },
                color,
            )?;
        }
        teamy_studio_shell::PanelEffect::WindowChromeMaximize => {
            stroke_rect(hdc, inset_rect(rect, 3), color, 1)?;
        }
        teamy_studio_shell::PanelEffect::WindowChromeRestore => {
            stroke_rect(
                hdc,
                RECT {
                    left: rect.left + 5,
                    top: rect.top + 3,
                    right: rect.right - 3,
                    bottom: rect.bottom - 5,
                },
                color,
                1,
            )?;
            stroke_rect(
                hdc,
                RECT {
                    left: rect.left + 3,
                    top: rect.top + 5,
                    right: rect.right - 5,
                    bottom: rect.bottom - 3,
                },
                color,
                1,
            )?;
        }
        teamy_studio_shell::PanelEffect::WindowChromeClose => {
            diagonal_stroke(hdc, rect, color, true)?;
            diagonal_stroke(hdc, rect, color, false)?;
        }
        _ => {}
    }
    Ok(())
}

fn draw_bitmap_glyph(hdc: HDC, rect: RECT, character: char, color: [f32; 4]) -> Result<()> {
    let glyph = glyph_pattern(character);
    let width = (rect.right - rect.left).max(1);
    let height = (rect.bottom - rect.top).max(1);
    let pixel_width = (width / 5).max(1);
    let pixel_height = (height / 7).max(1);
    let origin_x = rect.left + ((width - pixel_width * 5).max(0) / 2);
    let origin_y = rect.top + ((height - pixel_height * 7).max(0) / 2);

    for (row_index, row_bits) in glyph.iter().enumerate() {
        for column in 0..5 {
            if (row_bits >> (4 - column)) & 1 == 0 {
                continue;
            }
            let block = RECT {
                left: origin_x + column * pixel_width,
                top: origin_y + i32::try_from(row_index).unwrap_or_default() * pixel_height,
                right: origin_x + (column + 1) * pixel_width,
                bottom: origin_y
                    + (i32::try_from(row_index).unwrap_or_default() + 1) * pixel_height,
            };
            fill_rect_with_color(hdc, block, color)?;
        }
    }

    Ok(())
}

fn glyph_pattern(character: char) -> [u8; 7] {
    match character.to_ascii_uppercase() {
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0F, 0x10, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x01, 0x01, 0x01, 0x01, 0x11, 0x11, 0x0E],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x06, 0x08, 0x10, 0x1F],
        '3' => [0x1E, 0x01, 0x01, 0x0E, 0x01, 0x01, 0x1E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x10, 0x1E, 0x01, 0x01, 0x1E],
        '6' => [0x0E, 0x10, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x01, 0x0E],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '?' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x00, 0x04],
        _ => [0x00; 7],
    }
}

fn inset_rect(rect: RECT, inset: i32) -> RECT {
    RECT {
        left: rect.left + inset,
        top: rect.top + inset,
        right: rect.right - inset,
        bottom: rect.bottom - inset,
    }
}

fn stroke_rect(hdc: HDC, rect: RECT, color: [f32; 4], thickness: i32) -> Result<()> {
    fill_rect_with_color(
        hdc,
        RECT {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.top + thickness,
        },
        color,
    )?;
    fill_rect_with_color(
        hdc,
        RECT {
            left: rect.left,
            top: rect.bottom - thickness,
            right: rect.right,
            bottom: rect.bottom,
        },
        color,
    )?;
    fill_rect_with_color(
        hdc,
        RECT {
            left: rect.left,
            top: rect.top + thickness,
            right: rect.left + thickness,
            bottom: rect.bottom - thickness,
        },
        color,
    )?;
    fill_rect_with_color(
        hdc,
        RECT {
            left: rect.right - thickness,
            top: rect.top + thickness,
            right: rect.right,
            bottom: rect.bottom - thickness,
        },
        color,
    )
}

fn diagonal_stroke(hdc: HDC, rect: RECT, color: [f32; 4], forward: bool) -> Result<()> {
    let size = (rect.right - rect.left).min(rect.bottom - rect.top).max(1);
    for index in 0..size {
        let x = if forward {
            rect.left + index
        } else {
            rect.right - index - 1
        };
        let y = rect.top + index;
        fill_rect_with_color(
            hdc,
            RECT {
                left: x,
                top: y,
                right: x + 2,
                bottom: y + 2,
            },
            color,
        )?;
    }
    Ok(())
}

fn darken(color: [f32; 4], amount: f32) -> [f32; 4] {
    [
        (color[0] - amount).max(0.0),
        (color[1] - amount).max(0.0),
        (color[2] - amount).max(0.0),
        color[3],
    ]
}

fn lighten(color: [f32; 4], amount: f32) -> [f32; 4] {
    [
        (color[0] + amount).min(1.0),
        (color[1] + amount).min(1.0),
        (color[2] + amount).min(1.0),
        color[3],
    ]
}
