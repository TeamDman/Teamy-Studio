use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use eyre::eyre;
use widestring::{U16CStr, U16CString};
use windows::core::{PCWSTR, PWSTR};

use crate::win32_support::string::pcwstr_guard::PCWSTRGuard;

pub trait EasyPCWSTR<'a> {
    /// Convert the value into UTF-16 storage suitable for temporary Win32 calls.
    ///
    /// # Errors
    ///
    /// Returns an error when the source string must be converted and cannot be
    /// represented as a nul-terminated UTF-16 sequence.
    fn easy_pcwstr(self) -> eyre::Result<PCWSTRGuard<'a>>;
}

impl EasyPCWSTR<'static> for U16CString {
    fn easy_pcwstr(self) -> eyre::Result<PCWSTRGuard<'static>> {
        Ok(PCWSTRGuard::from_owned(self))
    }
}

impl EasyPCWSTR<'static> for &str {
    fn easy_pcwstr(self) -> eyre::Result<PCWSTRGuard<'static>> {
        Ok(PCWSTRGuard::from_owned(
            U16CString::from_str(self)
                .map_err(|error| eyre!("failed to convert &str to U16CString: {self}: {error}"))?,
        ))
    }
}

impl EasyPCWSTR<'static> for String {
    fn easy_pcwstr(self) -> eyre::Result<PCWSTRGuard<'static>> {
        Ok(PCWSTRGuard::from_owned(
            U16CString::from_str(&self).map_err(|error| {
                eyre!("failed to convert String to U16CString: {self}: {error}")
            })?,
        ))
    }
}

impl EasyPCWSTR<'static> for &OsString {
    fn easy_pcwstr(self) -> eyre::Result<PCWSTRGuard<'static>> {
        Ok(PCWSTRGuard::from_owned(U16CString::from_os_str_truncate(
            self,
        )))
    }
}

impl EasyPCWSTR<'static> for &OsStr {
    fn easy_pcwstr(self) -> eyre::Result<PCWSTRGuard<'static>> {
        Ok(PCWSTRGuard::from_owned(U16CString::from_os_str_truncate(
            self,
        )))
    }
}

impl<'a> EasyPCWSTR<'a> for &'a U16CStr {
    fn easy_pcwstr(self) -> eyre::Result<PCWSTRGuard<'a>> {
        Ok(PCWSTRGuard::from_borrowed(self))
    }
}

impl<'a> EasyPCWSTR<'a> for &'a U16CString {
    fn easy_pcwstr(self) -> eyre::Result<PCWSTRGuard<'a>> {
        Ok(PCWSTRGuard::from_borrowed(self.as_ucstr()))
    }
}

impl EasyPCWSTR<'static> for &PathBuf {
    fn easy_pcwstr(self) -> eyre::Result<PCWSTRGuard<'static>> {
        Ok(PCWSTRGuard::from_owned(U16CString::from_os_str_truncate(
            self.as_os_str(),
        )))
    }
}

impl EasyPCWSTR<'static> for &Path {
    fn easy_pcwstr(self) -> eyre::Result<PCWSTRGuard<'static>> {
        Ok(PCWSTRGuard::from_owned(U16CString::from_os_str_truncate(
            self.as_os_str(),
        )))
    }
}

impl EasyPCWSTR<'static> for PWSTR {
    fn easy_pcwstr(self) -> eyre::Result<PCWSTRGuard<'static>> {
        // Safety: `self` is expected to point to a valid nul-terminated UTF-16 string.
        let string = unsafe { U16CString::from_ptr_str(self.as_ptr()) };
        Ok(PCWSTRGuard::from_owned(string))
    }
}

impl EasyPCWSTR<'static> for PCWSTR {
    fn easy_pcwstr(self) -> eyre::Result<PCWSTRGuard<'static>> {
        // Safety: `self` is expected to point to a valid nul-terminated UTF-16 string.
        let string = unsafe { U16CString::from_ptr_str(self.as_ptr()) };
        Ok(PCWSTRGuard::from_owned(string))
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    use widestring::U16CString;

    use super::EasyPCWSTR;

    #[test]
    fn converts_common_string_types() -> eyre::Result<()> {
        "Hello, World!".easy_pcwstr()?;
        OsString::from("value").easy_pcwstr()?;
        "value".to_string().easy_pcwstr()?;
        PathBuf::from("value").easy_pcwstr()?;
        Path::new("value").easy_pcwstr()?;
        U16CString::from_str("value")?.easy_pcwstr()?;
        Ok(())
    }

    #[test]
    fn owns_existing_wide_storage_when_consumed() -> eyre::Result<()> {
        let borrowed = U16CString::from_str("value")?;
        let expected = borrowed.as_ucstr().as_slice().to_vec();
        let guard = borrowed.easy_pcwstr()?;

        assert_eq!(guard.as_wide(), expected);
        Ok(())
    }

    #[test]
    fn borrows_wide_slice() -> eyre::Result<()> {
        let borrowed = U16CString::from_str("value")?;
        let guard = borrowed.as_ucstr().easy_pcwstr()?;

        assert_eq!(guard.as_wide(), borrowed.as_ucstr().as_slice());
        Ok(())
    }
}
