use std::sync::{Mutex, OnceLock};

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
    PostQuitMessage, RegisterClassW, SW_MAXIMIZE, SW_MINIMIZE, SW_SHOW, ShowWindow,
    SetWindowPos, TranslateMessage, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_LBUTTONUP,
    WM_NCCALCSIZE, WM_NCHITTEST, WM_PAINT, WNDCLASSW, WS_EX_APPWINDOW, WS_MAXIMIZEBOX,
    WS_MINIMIZEBOX, WS_POPUP, WS_THICKFRAME, WS_VISIBLE, SWP_NOACTIVATE, SWP_NOZORDER,
};
use windows::core::{PCWSTR, w};

use crate::{
    FeatureValidationState, MainMenuLogicalButtonId, MainMenuSnapshot, build_main_menu_scene,
    layout_main_menu_button_cards,
};
use teamy_studio_shell::{
    GlyphQuad, RenderScene, ShellSceneLayout, SpriteId, SpriteQuad, WindowChromeButtonsState,
    d3d12_smoke_test_requested, initialize_dpi_awareness, scale_for_dpi,
    smoke_bootstrap_text_renderer_for_scene, system_dpi, window_dpi,
};

const MAIN_MENU_WINDOW_WIDTH: i32 = 1180;
const MAIN_MENU_WINDOW_HEIGHT: i32 = 760;
const MAIN_MENU_WINDOW_CLASS_NAME: PCWSTR = w!("TeamyStudioMainMenuWindow");
const MAIN_MENU_WINDOW_TITLE: PCWSTR = w!("Teamy Studio Main Menu");
const RESIZE_BORDER_THICKNESS: i32 = 8;

static MAIN_MENU_WINDOW_CLASS_REGISTERED: OnceLock<()> = OnceLock::new();
static CLICKED_BUTTON_IDS: OnceLock<Mutex<Vec<MainMenuLogicalButtonId>>> = OnceLock::new();
static MAIN_MENU_WINDOW_STATE: OnceLock<Mutex<Option<NativeMainMenuWindowState>>> = OnceLock::new();

#[derive(Clone, Debug)]
struct NativeMainMenuWindowState {
    snapshot: MainMenuSnapshot,
    chrome: WindowChromeButtonsState,
    dpi: u32,
}

unsafe extern "system" fn main_menu_window_proc(
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
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let _ = handle_left_button_up(hwnd, lparam);
            LRESULT(0)
        }
        WM_PAINT => paint_main_menu_window(hwnd),
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn main_menu_window_state() -> &'static Mutex<Option<NativeMainMenuWindowState>> {
    MAIN_MENU_WINDOW_STATE.get_or_init(|| Mutex::new(None))
}

fn ensure_main_menu_window_class_registered() -> Result<()> {
    if MAIN_MENU_WINDOW_CLASS_REGISTERED.get().is_some() {
        return Ok(());
    }

    let instance = unsafe { GetModuleHandleW(None) }
        .map_err(|error| eyre!("failed to get module handle: {error}"))?;
    let cursor = unsafe { LoadCursorW(Some(HINSTANCE::default()), IDC_ARROW) }
        .map_err(|error| eyre!("failed to load arrow cursor: {error}"))?;

    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        hInstance: instance.into(),
        lpszClassName: MAIN_MENU_WINDOW_CLASS_NAME,
        lpfnWndProc: Some(main_menu_window_proc),
        hCursor: cursor,
        ..Default::default()
    };

    let atom = unsafe { RegisterClassW(&class) };
    if atom == 0 {
        return Err(eyre!("failed to register main menu window class"));
    }

    let _ = MAIN_MENU_WINDOW_CLASS_REGISTERED.set(());
    Ok(())
}

fn handle_left_button_up(hwnd: HWND, lparam: LPARAM) -> Result<()> {
    let point = POINT {
        x: (lparam.0 & 0xFFFF) as i16 as i32,
        y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
    };
    let mut client_rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut client_rect) }
        .map_err(|error| eyre!("failed to get client rect for hit testing: {error}"))?;

    let window_state = main_menu_window_state()
        .lock()
        .expect("main menu window state should not be poisoned");
    let Some(window_state) = window_state.as_ref() else {
        return Ok(());
    };

    let dpi = window_state.dpi;
    let _ = window_state;
    let layout = ShellSceneLayout::for_main_menu_with_dpi(
        client_rect.right - client_rect.left,
        client_rect.bottom - client_rect.top,
        dpi,
    );

    if let Some(action) = chrome_click_action(layout, point) {
        perform_chrome_click(hwnd, action);
        return Ok(());
    }

    let window_state = main_menu_window_state()
        .lock()
        .expect("main menu window state should not be poisoned");
    let Some(window_state) = window_state.as_ref() else {
        return Ok(());
    };

    let card_layouts =
        layout_main_menu_button_cards(layout.body_rect, window_state.snapshot.buttons().len());
    if let Some(button) = window_state
        .snapshot
        .buttons()
        .iter()
        .zip(card_layouts.iter())
        .find_map(|(button, layout)| {
            rect_contains(layout.card_rect, point.x, point.y)
                .then_some(button)
                .filter(|button| button.validation_state == FeatureValidationState::Validated)
        })
    {
        CLICKED_BUTTON_IDS
            .get_or_init(|| Mutex::new(Vec::new()))
            .lock()
            .expect("main menu click queue should not be poisoned")
            .push(button.logical_button_id);
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

fn paint_main_menu_window(hwnd: HWND) -> LRESULT {
    let mut paint = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
    if !hdc.0.is_null() {
        let _ = render_main_menu_scene(hdc, hwnd);
        unsafe {
            let _ = EndPaint(hwnd, &paint);
        };
    }
    LRESULT(0)
}

fn render_main_menu_scene(hdc: HDC, hwnd: HWND) -> Result<()> {
    let mut client_rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut client_rect) }
        .map_err(|error| eyre!("failed to get client rect for painting: {error}"))?;
    let dpi = {
        let window_state = main_menu_window_state()
            .lock()
            .expect("main menu window state should not be poisoned");
        window_state.as_ref().map_or_else(system_dpi, |state| state.dpi)
    };
    let layout = ShellSceneLayout::for_main_menu_with_dpi(
        client_rect.right - client_rect.left,
        client_rect.bottom - client_rect.top,
        dpi,
    );

    let (snapshot, chrome) = {
        let window_state = main_menu_window_state()
            .lock()
            .expect("main menu window state should not be poisoned");
        let Some(window_state) = window_state.as_ref() else {
            return Ok(());
        };
        (window_state.snapshot.clone(), window_state.chrome)
    };

    let scene = build_main_menu_scene(&snapshot, layout, chrome, &[]);
    draw_scene(hdc, &scene)
}

fn draw_scene(hdc: HDC, scene: &RenderScene) -> Result<()> {
    for panel in &scene.panels {
        draw_panel(hdc, panel)?;
    }
    for sprite in &scene.sprites {
        draw_sprite(hdc, sprite)?;
    }
    for glyph in &scene.glyphs {
        draw_glyph(hdc, glyph)?;
    }
    Ok(())
}

fn draw_panel(hdc: HDC, panel: &teamy_studio_shell::PanelRect) -> Result<()> {
    fill_rect_with_color(hdc, panel.rect, panel.color)?;
    match panel.effect {
        teamy_studio_shell::PanelEffect::BlueBackground => {
            stroke_rect(hdc, panel.rect, darken(panel.color, 0.32), 1)?;
            stroke_rect(hdc, inset_rect(panel.rect, 1), lighten(panel.color, 0.06), 1)?;
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
        teamy_studio_shell::PanelEffect::SceneButtonCard => {
            stroke_rect(hdc, panel.rect, lighten(panel.color, 0.04), 1)?;
            fill_rect_with_color(hdc, inset_rect(panel.rect, 8), darken(panel.color, 0.52))?;
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

fn fill_rect_with_color(hdc: HDC, rect: RECT, color: [f32; 4]) -> Result<()> {
    let brush = unsafe { CreateSolidBrush(rgb(color)) };
    if brush.0.is_null() {
        return Err(eyre!("failed to create brush"));
    }
    unsafe { FillRect(hdc, &rect, brush) };
    let _ = unsafe { DeleteObject(brush.into()) };
    Ok(())
}

fn draw_sprite(hdc: HDC, sprite: &SpriteQuad) -> Result<()> {
    match sprite.sprite {
        SpriteId::Terminal => draw_terminal_sprite(hdc, sprite.rect),
        SpriteId::Storage => draw_storage_sprite(hdc, sprite.rect),
        SpriteId::Audio => draw_audio_sprite(hdc, sprite.rect),
        SpriteId::CursorArrow => draw_cursor_arrow_sprite(hdc, sprite.rect),
    }
}

fn draw_glyph(hdc: HDC, glyph: &GlyphQuad) -> Result<()> {
    draw_bitmap_glyph(hdc, glyph.rect, glyph.character, glyph.color)
}

fn rgb(color: [f32; 4]) -> COLORREF {
    COLORREF(
        u32::from((color[0].clamp(0.0, 1.0) * 255.0) as u8)
            | (u32::from((color[1].clamp(0.0, 1.0) * 255.0) as u8) << 8)
            | (u32::from((color[2].clamp(0.0, 1.0) * 255.0) as u8) << 16),
    )
}

fn rect_contains(rect: RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
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

    let dpi = {
        let window_state = main_menu_window_state()
            .lock()
            .expect("main menu window state should not be poisoned");
        window_state.as_ref().map_or_else(system_dpi, |state| state.dpi)
    };
    let layout = ShellSceneLayout::for_main_menu_with_dpi(width, height, dpi);
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
    let dpi = window_dpi(hwnd);
    if let Some(state) = main_menu_window_state()
        .lock()
        .expect("main menu window state should not be poisoned")
        .as_mut()
    {
        state.dpi = dpi;
    }

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

fn draw_terminal_sprite(hdc: HDC, rect: RECT) -> Result<()> {
    let bezel = inset_rect(rect, 6);
    let screen = inset_rect(bezel, 8);
    fill_rect_with_color(hdc, bezel, [0.18, 0.24, 0.32, 1.0])?;
    fill_rect_with_color(hdc, screen, [0.03, 0.06, 0.10, 1.0])?;
    draw_pixel_block(hdc, screen, 1, 1, 1, 1, [0.34, 0.83, 1.0, 1.0])?;
    draw_pixel_block(hdc, screen, 2, 2, 1, 1, [0.34, 0.83, 1.0, 1.0])?;
    draw_pixel_block(hdc, screen, 4, 4, 3, 1, [0.34, 0.83, 1.0, 1.0])?;
    let stand_top = RECT {
        left: rect.left + ((rect.right - rect.left) * 34 / 100),
        top: rect.top + ((rect.bottom - rect.top) * 78 / 100),
        right: rect.left + ((rect.right - rect.left) * 66 / 100),
        bottom: rect.top + ((rect.bottom - rect.top) * 84 / 100),
    };
    let stand_base = RECT {
        left: rect.left + ((rect.right - rect.left) * 22 / 100),
        top: rect.top + ((rect.bottom - rect.top) * 86 / 100),
        right: rect.left + ((rect.right - rect.left) * 78 / 100),
        bottom: rect.top + ((rect.bottom - rect.top) * 92 / 100),
    };
    fill_rect_with_color(hdc, stand_top, [0.25, 0.31, 0.44, 1.0])?;
    fill_rect_with_color(hdc, stand_base, [0.20, 0.26, 0.38, 1.0])
}

fn draw_storage_sprite(hdc: HDC, rect: RECT) -> Result<()> {
    let rack_color = [0.45, 0.78, 0.64, 1.0];
    for row in 0..3 {
        let top = rect.top + ((rect.bottom - rect.top) * (18 + row * 22) / 100);
        let rack = RECT {
            left: rect.left + ((rect.right - rect.left) * 16 / 100),
            top,
            right: rect.left + ((rect.right - rect.left) * 84 / 100),
            bottom: top + ((rect.bottom - rect.top) * 12 / 100),
        };
        fill_rect_with_color(hdc, rack, darken(rack_color, 0.18))?;
        stroke_rect(hdc, rack, rack_color, 1)?;
        let led = RECT {
            left: rack.left + 8,
            top: rack.top + 4,
            right: rack.left + 16,
            bottom: rack.top + 8,
        };
        fill_rect_with_color(hdc, led, [0.75, 0.96, 0.84, 1.0])?;
    }
    Ok(())
}

fn draw_audio_sprite(hdc: HDC, rect: RECT) -> Result<()> {
    let cable = RECT {
        left: rect.left + ((rect.right - rect.left) * 28 / 100),
        top: rect.top + ((rect.bottom - rect.top) * 46 / 100),
        right: rect.left + ((rect.right - rect.left) * 80 / 100),
        bottom: rect.top + ((rect.bottom - rect.top) * 56 / 100),
    };
    fill_rect_with_color(hdc, cable, [0.26, 0.26, 0.28, 1.0])?;
    let left_plug = RECT {
        left: rect.left + ((rect.right - rect.left) * 18 / 100),
        top: rect.top + ((rect.bottom - rect.top) * 42 / 100),
        right: rect.left + ((rect.right - rect.left) * 38 / 100),
        bottom: rect.top + ((rect.bottom - rect.top) * 62 / 100),
    };
    let right_plug = RECT {
        left: rect.left + ((rect.right - rect.left) * 54 / 100),
        top: rect.top + ((rect.bottom - rect.top) * 42 / 100),
        right: rect.left + ((rect.right - rect.left) * 74 / 100),
        bottom: rect.top + ((rect.bottom - rect.top) * 62 / 100),
    };
    fill_rect_with_color(hdc, left_plug, [0.84, 0.71, 0.39, 1.0])?;
    fill_rect_with_color(hdc, right_plug, [0.79, 0.28, 0.18, 1.0])?;
    let left_tip = RECT {
        left: left_plug.left - 6,
        top: left_plug.top + 6,
        right: left_plug.left,
        bottom: left_plug.bottom - 6,
    };
    let right_tip = RECT {
        left: right_plug.right,
        top: right_plug.top + 6,
        right: right_plug.right + 6,
        bottom: right_plug.bottom - 6,
    };
    fill_rect_with_color(hdc, left_tip, [0.96, 0.90, 0.66, 1.0])?;
    fill_rect_with_color(hdc, right_tip, [0.98, 0.86, 0.70, 1.0])
}

fn draw_cursor_arrow_sprite(hdc: HDC, rect: RECT) -> Result<()> {
    let color = [0.97, 0.90, 0.08, 1.0];
    let shadow = [0.02, 0.02, 0.03, 0.42];
    let pixels = [
        (0, 0, 1, 8),
        (1, 1, 1, 8),
        (2, 2, 1, 8),
        (3, 3, 1, 8),
        (4, 4, 1, 7),
        (5, 5, 1, 6),
        (6, 6, 1, 5),
        (7, 7, 1, 4),
        (8, 8, 1, 3),
        (9, 9, 1, 2),
        (3, 8, 4, 1),
        (4, 9, 4, 1),
        (5, 10, 4, 1),
        (6, 11, 3, 1),
        (5, 12, 2, 1),
        (7, 10, 1, 4),
    ];
    for (x, y, w, h) in pixels {
        draw_pixel_block(hdc, rect, x + 1, y + 1, w, h, shadow)?;
        draw_pixel_block(hdc, rect, x, y, w, h, color)?;
    }
    Ok(())
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
                bottom: origin_y + (i32::try_from(row_index).unwrap_or_default() + 1) * pixel_height,
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

fn draw_pixel_block(
    hdc: HDC,
    rect: RECT,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    color: [f32; 4],
) -> Result<()> {
    let cell_width = ((rect.right - rect.left) / 16).max(1);
    let cell_height = ((rect.bottom - rect.top) / 16).max(1);
    let block = RECT {
        left: rect.left + x * cell_width,
        top: rect.top + y * cell_height,
        right: rect.left + (x + width) * cell_width,
        bottom: rect.top + (y + height) * cell_height,
    };
    fill_rect_with_color(hdc, block, color)
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

pub fn run_native_main_menu_window(snapshot: &MainMenuSnapshot) -> Result<()> {
    run_native_main_menu_window_with_click_handler(snapshot, |_| Ok(()))
}

pub fn run_native_main_menu_window_with_click_handler<F>(
    snapshot: &MainMenuSnapshot,
    mut on_click: F,
) -> Result<()>
where
    F: FnMut(MainMenuLogicalButtonId) -> Result<()>,
{
    initialize_dpi_awareness();
    ensure_main_menu_window_class_registered()?;
    CLICKED_BUTTON_IDS.get_or_init(|| Mutex::new(Vec::new()));
    let dpi = system_dpi();
    *main_menu_window_state()
        .lock()
        .expect("main menu window state should not be poisoned") =
        Some(NativeMainMenuWindowState {
            snapshot: snapshot.clone(),
            chrome: WindowChromeButtonsState {
                focused: true,
                ..Default::default()
            },
            dpi,
        });

    let instance = unsafe { GetModuleHandleW(None) }
        .map_err(|error| eyre!("failed to get module handle: {error}"))?;

    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW,
            MAIN_MENU_WINDOW_CLASS_NAME,
            MAIN_MENU_WINDOW_TITLE,
            WS_POPUP | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_VISIBLE,
            120,
            80,
            scale_for_dpi(MAIN_MENU_WINDOW_WIDTH, dpi),
            scale_for_dpi(MAIN_MENU_WINDOW_HEIGHT, dpi),
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
    .map_err(|error| eyre!("failed to create main menu window: {error}"))?;

    if let Some(state) = main_menu_window_state()
        .lock()
        .expect("main menu window state should not be poisoned")
        .as_mut()
    {
        state.dpi = window_dpi(hwnd);
    }

    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
    }

    let smoke_bootstrap = if d3d12_smoke_test_requested() {
        let mut client_rect = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client_rect) }.map_err(|error| {
            eyre!("failed to get main menu client rect for D3D12 smoke bootstrap: {error}")
        })?;
        let (snapshot, chrome, dpi) = {
            let window_state = main_menu_window_state()
                .lock()
                .expect("main menu window state should not be poisoned");
            let state = window_state
                .as_ref()
                .expect("main menu window state should exist before smoke bootstrap");
            (state.snapshot.clone(), state.chrome, state.dpi)
        };
        let layout = ShellSceneLayout::for_main_menu_with_dpi(
            client_rect.right - client_rect.left,
            client_rect.bottom - client_rect.top,
            dpi,
        );
        let scene = build_main_menu_scene(&snapshot, layout, chrome, &[]);
        Some(
            smoke_bootstrap_text_renderer_for_scene(hwnd, &scene)
                .map_err(|error| eyre!("failed to bootstrap main menu D3D12 smoke path: {error:#}"))?,
        )
    } else {
        None
    };

    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
        if result == -1 {
            return Err(eyre!("main menu message loop failed"));
        }
        if result == 0 {
            break;
        }

        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        let click_queue = CLICKED_BUTTON_IDS
            .get()
            .expect("main menu click queue should exist before entering the message loop");
        let button_ids = {
            let mut queued = click_queue
                .lock()
                .expect("main menu click queue should not be poisoned");
            std::mem::take(&mut *queued)
        };
        for logical_button_id in button_ids {
            on_click(logical_button_id)?;
        }
    }

    *main_menu_window_state()
        .lock()
        .expect("main menu window state should not be poisoned") = None;
    let _ = smoke_bootstrap;
    Ok(())
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::RECT;

    use super::rect_contains;

    #[test]
    fn rect_contains_uses_half_open_bounds() {
        let rect = RECT {
            left: 10,
            top: 20,
            right: 30,
            bottom: 40,
        };

        assert!(rect_contains(rect, 10, 20));
        assert!(rect_contains(rect, 29, 39));
        assert!(!rect_contains(rect, 30, 39));
        assert!(!rect_contains(rect, 29, 40));
    }
}
