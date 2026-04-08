use teamy_windows::string::EasyPCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Controls::{
    TASKDIALOG_BUTTON, TASKDIALOG_COMMON_BUTTON_FLAGS, TASKDIALOG_FLAGS, TASKDIALOGCONFIG,
    TDF_ALLOW_DIALOG_CANCELLATION, TDF_POSITION_RELATIVE_TO_WINDOW, TDF_SIZE_TO_CONTENT,
    TaskDialogIndirect,
};
use windows::Win32::UI::WindowsAndMessaging::{
    IDOK, MB_ICONWARNING, MB_OKCANCEL, MESSAGEBOX_STYLE, MessageBoxW,
};

const BTN_PASTE_ANYWAY: i32 = 100;
const BTN_CANCEL: i32 = 101;
const MULTILINE_PASTE_PREVIEW_LIMIT: usize = 280;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteConfirmationChoice {
    Paste,
    Cancel,
}

// os[impl window.interaction.clipboard.multiline-paste-confirmation.native-dialog]
pub fn show_multiline_paste_confirmation_dialog(
    owner: Option<HWND>,
    clipboard_text: &str,
) -> eyre::Result<PasteConfirmationChoice> {
    let owner_hwnd = owner.unwrap_or_default();
    let window_title = "Confirm Multiline Paste".easy_pcwstr()?;
    let instruction = "The clipboard text contains multiple lines.".easy_pcwstr()?;
    let preview = multiline_paste_dialog_preview(clipboard_text);
    let content = format!(
        "Pasting this text may execute multiple commands in the shell. Do you want to continue?\n\nClipboard preview:\n\n{preview}"
    )
    .easy_pcwstr()?;
    let paste_label = "Paste anyway".easy_pcwstr()?;
    let cancel_label = "Cancel".easy_pcwstr()?;

    let buttons = [
        TASKDIALOG_BUTTON {
            nButtonID: BTN_PASTE_ANYWAY,
            // Safety: the UTF-16 button label storage lives until TaskDialogIndirect returns.
            pszButtonText: unsafe { paste_label.as_ptr() },
        },
        TASKDIALOG_BUTTON {
            nButtonID: BTN_CANCEL,
            // Safety: the UTF-16 button label storage lives until TaskDialogIndirect returns.
            pszButtonText: unsafe { cancel_label.as_ptr() },
        },
    ];

    let config = TASKDIALOGCONFIG {
        cbSize: u32::try_from(std::mem::size_of::<TASKDIALOGCONFIG>())
            .expect("TASKDIALOGCONFIG size fits in u32"),
        hwndParent: owner_hwnd,
        dwFlags: TASKDIALOG_FLAGS(
            TDF_ALLOW_DIALOG_CANCELLATION.0
                | TDF_POSITION_RELATIVE_TO_WINDOW.0
                | TDF_SIZE_TO_CONTENT.0,
        ),
        dwCommonButtons: TASKDIALOG_COMMON_BUTTON_FLAGS(0),
        // Safety: these UTF-16 strings outlive the dialog invocation below.
        pszWindowTitle: unsafe { window_title.as_ptr() },
        // Safety: these UTF-16 strings outlive the dialog invocation below.
        pszMainInstruction: unsafe { instruction.as_ptr() },
        // Safety: these UTF-16 strings outlive the dialog invocation below.
        pszContent: unsafe { content.as_ptr() },
        cButtons: u32::try_from(buttons.len()).expect("button count fits in u32"),
        pButtons: buttons.as_ptr(),
        ..Default::default()
    };

    let mut pressed_button = BTN_CANCEL;
    // Safety: the config and backing strings/buttons remain valid for the duration of the call.
    unsafe {
        if TaskDialogIndirect(&raw const config, Some(&raw mut pressed_button), None, None).is_ok()
        {
            return Ok(match pressed_button {
                BTN_PASTE_ANYWAY => PasteConfirmationChoice::Paste,
                _ => PasteConfirmationChoice::Cancel,
            });
        }
    }

    // Safety: the owner handle is either null or a valid top-level window, and the strings remain valid for the call.
    let message_box_result = unsafe {
        MessageBoxW(
            Some(owner_hwnd),
            content.as_ref(),
            window_title.as_ref(),
            MESSAGEBOX_STYLE(MB_OKCANCEL.0 | MB_ICONWARNING.0),
        )
    };

    Ok(if message_box_result == IDOK {
        PasteConfirmationChoice::Paste
    } else {
        PasteConfirmationChoice::Cancel
    })
}

#[must_use]
pub fn paste_confirmation_required(clipboard_text: &str) -> bool {
    clipboard_text.contains(['\r', '\n'])
}

fn multiline_paste_dialog_preview(clipboard_text: &str) -> String {
    let preview: String = clipboard_text
        .chars()
        .take(MULTILINE_PASTE_PREVIEW_LIMIT)
        .collect();
    if clipboard_text.chars().count() > MULTILINE_PASTE_PREVIEW_LIMIT {
        format!("{preview}\n\n...")
    } else {
        preview
    }
}

#[cfg(test)]
mod tests {
    use super::{multiline_paste_dialog_preview, paste_confirmation_required};

    // behavior[verify window.interaction.clipboard.right-click-paste.confirm-multiline]
    #[test]
    fn paste_confirmation_is_required_for_multiline_text() {
        assert!(paste_confirmation_required("first\nsecond"));
        assert!(paste_confirmation_required("first\rsecond"));
        assert!(!paste_confirmation_required("single line"));
    }

    #[test]
    fn multiline_paste_preview_is_truncated() {
        let text = "x".repeat(400);
        let preview = multiline_paste_dialog_preview(&text);

        assert!(preview.ends_with("\n\n..."));
        assert!(preview.len() < text.len());
    }
}
