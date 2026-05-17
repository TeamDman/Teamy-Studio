use std::ptr;

use eyre::{Context, Result, bail};
use widestring::{U16CStr, U16CString};
use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{CF_TEXT, CF_UNICODETEXT};

/// Read the current clipboard text as Unicode when available, otherwise ANSI.
///
/// # Errors
///
/// Returns an error when the clipboard cannot be opened, does not contain text,
/// or the clipboard data buffer cannot be accessed.
pub fn read_clipboard() -> Result<String> {
    let _guard = ClipboardGuard::open().wrap_err("failed to open clipboard")?;

    // Safety: querying clipboard format availability does not borrow the clipboard data.
    if unsafe { IsClipboardFormatAvailable(u32::from(CF_UNICODETEXT.0)).is_ok() } {
        // Safety: the clipboard is open on this thread and the format id is valid.
        let handle = unsafe { GetClipboardData(u32::from(CF_UNICODETEXT.0))? };
        if handle.is_invalid() {
            bail!("unicode clipboard handle was invalid");
        }
        read_clipboard_unicode(HGLOBAL(handle.0))
    // Safety: querying clipboard format availability does not borrow the clipboard data.
    } else if unsafe { IsClipboardFormatAvailable(u32::from(CF_TEXT.0)).is_ok() } {
        // Safety: the clipboard is open on this thread and the format id is valid.
        let handle = unsafe { GetClipboardData(u32::from(CF_TEXT.0))? };
        if handle.is_invalid() {
            bail!("ansi clipboard handle was invalid");
        }
        read_clipboard_ascii(HGLOBAL(handle.0))
    } else {
        bail!("no text data on the clipboard");
    }
}

/// Replace the clipboard contents with UTF-16 text.
///
/// # Errors
///
/// Returns an error when the clipboard cannot be opened, emptied, populated, or
/// the temporary global buffer cannot be allocated or locked.
pub fn write_clipboard(value: impl AsRef<str>) -> Result<()> {
    let _guard = ClipboardGuard::open().wrap_err("failed to open clipboard")?;
    // Safety: the clipboard is open on this thread and may be emptied before new data is set.
    unsafe { EmptyClipboard() }.wrap_err("failed to empty clipboard")?;

    let wide =
        U16CString::from_str(value.as_ref()).wrap_err("failed to convert string to UTF-16")?;
    let slice = wide.as_slice_with_nul();
    let size = std::mem::size_of_val(slice);
    // Safety: allocating a movable global buffer is required by the clipboard contract.
    let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, size) }
        .wrap_err("failed to allocate clipboard buffer")?;
    if handle.is_invalid() {
        bail!("failed to allocate clipboard buffer");
    }

    // Safety: locking the newly allocated global handle yields writable memory for the buffer copy.
    let lock = unsafe { GlobalLock(handle) };
    if lock.is_null() {
        bail!("failed to lock clipboard buffer");
    }

    // Safety: `lock` points to `size` writable bytes and `slice` includes the terminating nul.
    unsafe { ptr::copy_nonoverlapping(slice.as_ptr(), lock.cast::<u16>(), slice.len()) };
    // Safety: unlocking the global handle after the copy releases the temporary writable view.
    let _ = unsafe { GlobalUnlock(handle) };

    // Safety: ownership of `handle` transfers to the clipboard on success.
    unsafe { SetClipboardData(u32::from(CF_UNICODETEXT.0), Some(HANDLE(handle.0))) }
        .wrap_err("failed to set clipboard data")?;

    Ok(())
}

fn read_clipboard_ascii(handle: HGLOBAL) -> Result<String> {
    // Safety: the clipboard handle names a live global buffer for the requested text format.
    let lock = unsafe { GlobalLock(handle) };
    if lock.is_null() {
        bail!("failed to lock clipboard data");
    }

    let data_ptr = lock as *const u8;
    // Safety: CF_TEXT data is stored as a nul-terminated ANSI string.
    let c_str = unsafe { std::ffi::CStr::from_ptr(data_ptr.cast::<i8>()) };
    let result = String::from_utf8_lossy(c_str.to_bytes()).to_string();
    // Safety: unlocking the global buffer releases the borrowed view.
    let _ = unsafe { GlobalUnlock(handle) };
    Ok(result)
}

fn read_clipboard_unicode(handle: HGLOBAL) -> Result<String> {
    // Safety: the clipboard handle names a live global buffer for the requested text format.
    let lock = unsafe { GlobalLock(handle) };
    if lock.is_null() {
        bail!("failed to lock clipboard data");
    }

    let data_ptr = lock as *const u16;
    // Safety: CF_UNICODETEXT data is stored as a nul-terminated UTF-16 string.
    let wide = unsafe { U16CStr::from_ptr_str(data_ptr) };
    let result = wide.to_string_lossy().clone();
    // Safety: unlocking the global buffer releases the borrowed view.
    let _ = unsafe { GlobalUnlock(handle) };
    Ok(result)
}

struct ClipboardGuard;

impl ClipboardGuard {
    fn open() -> Result<Self> {
        // Safety: opening the process clipboard is valid without an owner window for this use case.
        unsafe { OpenClipboard(None) }.wrap_err("failed to open clipboard")?;
        Ok(Self)
    }
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        // Safety: closing the clipboard is the required paired cleanup for a successful open.
        let _ = unsafe { CloseClipboard() };
    }
}
