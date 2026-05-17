use eyre::Context;
use widestring::U16CString;
use windows::core::PWSTR;

/// Mutable UTF-16 storage for Win32 APIs that keep a writable `PWSTR` pointer.
#[derive(Debug)]
pub struct PWSTRBuffer {
    string: Vec<u16>,
}

impl PWSTRBuffer {
    /// # Errors
    ///
    /// Returns an error when `value` cannot be represented as a nul-terminated
    /// UTF-16 string.
    pub fn from_text(value: &str) -> eyre::Result<Self> {
        let string = U16CString::from_str(value)
            .wrap_err("failed to convert string to mutable UTF-16 storage")?;
        Ok(Self {
            string: string.into_vec_with_nul(),
        })
    }

    /// # Errors
    ///
    /// Returns an error when `value` cannot be represented as a nul-terminated
    /// UTF-16 string.
    pub fn set(&mut self, value: &str) -> eyre::Result<()> {
        *self = Self::from_text(value)?;
        Ok(())
    }

    #[must_use]
    pub fn as_pwstr(&mut self) -> PWSTR {
        PWSTR(self.string.as_mut_ptr())
    }

    #[must_use]
    pub fn as_wide(&self) -> &[u16] {
        &self.string
    }
}

impl Default for PWSTRBuffer {
    fn default() -> Self {
        Self { string: vec![0] }
    }
}

#[cfg(test)]
mod tests {
    use super::PWSTRBuffer;

    #[test]
    fn converts_and_updates_strings() -> eyre::Result<()> {
        let mut buffer = PWSTRBuffer::from_text("hello")?;
        assert_eq!(buffer.as_wide(), &[104, 101, 108, 108, 111, 0]);

        buffer.set("bye")?;
        assert_eq!(buffer.as_wide(), &[98, 121, 101, 0]);
        Ok(())
    }

    #[test]
    fn default_is_nul_terminated() {
        let buffer = PWSTRBuffer::default();
        assert_eq!(buffer.as_wide(), &[0]);
    }
}
