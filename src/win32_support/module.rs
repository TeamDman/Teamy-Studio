use tracing::debug;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::GetModuleHandleExW;

/// Return the current process module handle.
///
/// # Errors
///
/// Returns an error when the module handle cannot be queried from the OS.
pub fn get_current_module() -> eyre::Result<HMODULE> {
    // Safety: requesting the current module handle with a null module name is the documented API usage.
    unsafe {
        let mut out = HMODULE::default();
        GetModuleHandleExW(Default::default(), None, &raw mut out)?;
        debug!(handle = ?out, "Got current module");
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn gets_current_module() -> eyre::Result<()> {
        let module = super::get_current_module()?;
        assert_ne!(module.0, std::ptr::null_mut());
        Ok(())
    }
}
