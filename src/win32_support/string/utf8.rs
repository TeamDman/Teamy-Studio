use windows::Win32::Globalization::GetACP;

pub const UTF8_CODEPAGE: u32 = 65001;

#[must_use]
pub fn is_system_utf8() -> bool {
    // Safety: `GetACP` is a process-global query with no pointer arguments.
    unsafe { GetACP() == UTF8_CODEPAGE }
}

pub fn warn_if_utf8_not_enabled() {
    if !is_system_utf8() {
        tracing::warn!("The current system codepage is not UTF-8. This may cause '?' problems.");
        tracing::warn!(
            "See https://github.com/Azure/azure-cli/issues/22616#issuecomment-1147061949"
        );
        tracing::warn!(
            "Control panel -> Clock and Region -> Region -> Administrative -> Change system locale -> Check Beta: Use Unicode UTF-8 for worldwide language support."
        );
    }
}
