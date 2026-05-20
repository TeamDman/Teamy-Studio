use std::borrow::Cow;
use std::ops::Deref;

use widestring::{U16CStr, U16CString};
use windows::core::{PCWSTR, Param, ParamValue};

/// Prevents `Self` from being dropped before the finish of a FFI call.
#[derive(Debug)]
pub struct PCWSTRGuard<'a> {
    string: Cow<'a, U16CStr>,
}

impl<'a> PCWSTRGuard<'a> {
    #[must_use]
    pub fn from_owned(string: U16CString) -> Self {
        Self {
            string: Cow::Owned(string),
        }
    }

    #[must_use]
    pub fn from_borrowed(string: &'a U16CStr) -> Self {
        Self {
            string: Cow::Borrowed(string),
        }
    }

    /// # Safety
    ///
    /// The returned pointer must not outlive this guard.
    #[must_use]
    pub unsafe fn as_ptr(&self) -> PCWSTR {
        PCWSTR(self.string.as_ptr())
    }

    #[must_use]
    pub fn as_wide(&self) -> &[u16] {
        self.string.as_slice()
    }
}

/// Builds a Win32 integer resource pointer such as `MAKEINTRESOURCEW`.
#[must_use]
pub fn int_resource_pcwstr(resource_id: usize) -> PCWSTR {
    PCWSTR(resource_id as *const u16)
}

impl Deref for PCWSTRGuard<'_> {
    type Target = U16CStr;

    fn deref(&self) -> &Self::Target {
        self.string.as_ref()
    }
}

/// MUST NOT implement this for `PCWSTRGuard` itself, only for `&PCWSTRGuard`,
/// to ensure the data the PCWSTR points to is valid for the lifetime of the parameter.
impl Param<PCWSTR> for &PCWSTRGuard<'_> {
    unsafe fn param(self) -> ParamValue<PCWSTR> {
        ParamValue::Borrowed(PCWSTR(self.string.as_ptr()))
    }
}

/// Included for postfix `.as_ref()` convenience.
impl<'a> AsRef<PCWSTRGuard<'a>> for PCWSTRGuard<'a> {
    fn as_ref(&self) -> &PCWSTRGuard<'a> {
        self
    }
}
