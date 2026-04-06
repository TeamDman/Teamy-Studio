use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject, FillRect, HDC};

pub const PANEL_WINDOW_ALPHA: u8 = 204;
pub const WINDOW_DEBUG_BACKGROUND: COLORREF = COLORREF(0x00A0_6000);

pub fn paint_background_layer(hdc: HDC, client_width: i32, client_height: i32) -> eyre::Result<()> {
    let brush = unsafe { CreateSolidBrush(WINDOW_DEBUG_BACKGROUND) };
    if brush.0.is_null() {
        eyre::bail!("failed to create window background brush");
    }

    let rect = RECT {
        left: 0,
        top: 0,
        right: client_width,
        bottom: client_height,
    };
    let _ = unsafe { FillRect(hdc, &rect, brush) };
    let _ = unsafe { DeleteObject(brush.into()) };
    Ok(())
}
