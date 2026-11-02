//! Reads the text the user had selected in the application that held focus.
//!
//! UI Automation comes first: most modern apps (browsers, terminals, Office) expose their
//! text selection through the UIA `TextPattern`, so nothing is synthesized at all.
//! Applications without an exposed selection get the classic approach — a synthetic
//! `Ctrl+C` copies the selection, the clipboard is read, and the user's previous clipboard
//! text is written back. Captures always run on their own thread, so a slow application can
//! never stall the UI.

use std::thread::{self, sleep};
use std::time::{Duration, Instant};

use crate::hotkey;
use hymt_core::{prompt, settings::AcquisitionMode};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber, OpenClipboard,
    SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern,
    IUIAutomationTreeWalker, UIA_TextPatternId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};

/// Virtual-key code of the `C` key used for the synthetic copy chord.
const VK_C: u16 = 0x43;
/// How many times `OpenClipboard` is retried while another process owns it.
const CLIPBOARD_OPEN_ATTEMPTS: usize = 5;
/// Delay between clipboard-open retries.
const CLIPBOARD_OPEN_RETRY_MS: u64 = 10;
/// How far up the UIA element tree the bridge walks looking for a text selection.
const UIA_PARENT_DEPTH: usize = 3;

/// Reads the selected text of whatever application held focus when the trigger fired.
///
/// `clipboard_min_seq` is the hint from a double-press trigger: when the clipboard sequence
/// number has moved past it, the source application just copied the selection itself and the
/// clipboard can be read directly without synthesizing another copy.
pub fn capture_selected(
    mode: AcquisitionMode,
    clipboard_min_seq: Option<u32>,
) -> Result<String, String> {
    if let Some(min_seq) = clipboard_min_seq {
        unsafe {
            if GetClipboardSequenceNumber() >= min_seq {
                if let Some(text) = read_clipboard_text() {
                    return clamp_capture(&text);
                }
            }
        }
    }
    match mode {
        AcquisitionMode::Auto => {
            if let Some(text) = unsafe { capture_uia_text() } {
                return clamp_capture(&text);
            }
            unsafe { capture_clipboard_fallback() }
        }
        AcquisitionMode::ClipboardOnly => unsafe { capture_clipboard_fallback() },
    }
}

/// Applies the same non-empty / length rules the manual translation path enforces.
fn clamp_capture(text: &str) -> Result<String, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("선택한 텍스트가 없습니다.".to_string());
    }
    prompt::validate_source(trimmed)
        .map(|()| trimmed.to_string())
        .map_err(|message| message.to_string())
}

/// Reads the UI Automation selection of the focused element, walking a shallow parent
/// chain because the element with text patterns is often above the focused control.
unsafe fn capture_uia_text() -> Option<String> {
    // UIA needs a COM apartment on the capture thread; leftover errors degrade into None,
    // which the caller treats as "no UIA selection available".
    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    let automation: IUIAutomation =
        CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
    let walker: IUIAutomationTreeWalker = automation.ControlViewWalker().ok()?;
    let mut element: Option<IUIAutomationElement> = automation.GetFocusedElement().ok();
    for _ in 0..=UIA_PARENT_DEPTH {
        let current = element?;
        if let Some(text) = selection_text_of(&current) {
            return Some(text);
        }
        element = walker.GetParentElement(&current).ok();
    }
    None
}

/// Extraction of one element's selected text; `None` also means "no usable pattern".
unsafe fn selection_text_of(element: &IUIAutomationElement) -> Option<String> {
    let pattern: IUIAutomationTextPattern = element
        .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
        .ok()?;
    let ranges = pattern.GetSelection().ok()?;
    let count = ranges.Length().ok()?.max(0);
    if count == 0 {
        return None;
    }
    let range = ranges.GetElement(0).ok()?;
    let text = range.GetText(-1).ok()?;
    let text = text.to_string();
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

/// The clipboard fallback: backs the clipboard up, injects `Ctrl+C` into the foreground
/// window, waits for the copy to land, reads the selection and puts the backup back.
unsafe fn capture_clipboard_fallback() -> Result<String, String> {
    let previous = read_clipboard_text();
    let baseline = GetClipboardSequenceNumber();
    inject_copy_chord();

    let deadline = Instant::now() + Duration::from_millis(hotkey::CLIPBOARD_COPY_TIMEOUT_MS);
    loop {
        if GetClipboardSequenceNumber() != baseline {
            if let Some(text) = read_clipboard_text() {
                restore_clipboard(previous);
                return clamp_capture(&text);
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        sleep(Duration::from_millis(hotkey::CLIPBOARD_POLL_INTERVAL_MS));
    }
    restore_clipboard(previous);
    Err(
        "선택한 텍스트를 복사하지 못했습니다. 텍스트를 선택한 상태에서 다시 시도해 주세요."
            .to_string(),
    )
}

/// Sends the key events of one logical key through `SendInput`.
fn send_key(vk: VIRTUAL_KEY, up: bool) {
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

/// Injects the synthetic copy chord, after resetting the logical modifiers that may still
/// be physically held from the hotkey press itself.
fn inject_copy_chord() {
    send_key(VK_SHIFT, true);
    send_key(VK_CONTROL, true);
    send_key(VK_MENU, true);
    send_key(VK_LWIN, true);
    send_key(VK_RWIN, true);
    send_key(VK_CONTROL, false);
    send_key(VIRTUAL_KEY(VK_C), false);
    send_key(VIRTUAL_KEY(VK_C), true);
    send_key(VK_CONTROL, true);
}

/// Retries `OpenClipboard` briefly; the clipboard is a shared global resource.
unsafe fn open_clipboard() -> bool {
    for _ in 0..CLIPBOARD_OPEN_ATTEMPTS {
        if OpenClipboard(None).is_ok() {
            return true;
        }
        thread::sleep(Duration::from_millis(CLIPBOARD_OPEN_RETRY_MS));
    }
    false
}

/// Reads `CF_UNICODETEXT` off the clipboard, or `None` when empty or unreadable.
unsafe fn read_clipboard_text() -> Option<String> {
    if !open_clipboard() {
        return None;
    }
    // The clipboard stays open up to the close below, whichever path returns.
    let result = unsafe { read_open_clipboard_text() };
    let _ = CloseClipboard();
    result
}

/// Body of [`read_clipboard_text`] with the clipboard already open.
unsafe fn read_open_clipboard_text() -> Option<String> {
    let handle = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
    let hglobal = HGLOBAL(handle.0);
    let pointer = GlobalLock(hglobal);
    if pointer.is_null() {
        return None;
    }
    let words = core::slice::from_raw_parts(pointer as *const u16, GlobalSize(hglobal) / 2);
    let length = words
        .iter()
        .position(|word| *word == 0)
        .unwrap_or(words.len());
    let text = String::from_utf16_lossy(&words[..length]);
    GlobalUnlock(hglobal).ok();
    Some(text)
}

/// Writes `text` back over the clipboard (text only; richer payloads cannot be restored).
unsafe fn restore_clipboard(text: Option<String>) {
    let Some(text) = text else {
        return;
    };
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    if !open_clipboard() {
        return;
    }
    let restored = match EmptyClipboard() {
        Err(_) => false,
        Ok(()) => match GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
            Err(_) => false,
            Ok(hglobal) => {
                let pointer = GlobalLock(hglobal);
                if pointer.is_null() {
                    let _ = GlobalFree(Some(hglobal));
                    false
                } else {
                    core::ptr::copy_nonoverlapping(wide.as_ptr(), pointer as *mut u16, wide.len());
                    let _ = GlobalUnlock(hglobal);
                    match SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(hglobal.0))) {
                        // Ownership of the memory moved to the clipboard.
                        Ok(_) => true,
                        Err(_) => {
                            let _ = GlobalFree(Some(hglobal));
                            false
                        }
                    }
                }
            }
        },
    };
    let _ = CloseClipboard();
    if !restored {
        // Emptying the clipboard failed or the write was rejected; nothing to undo that is
        // accessible from here, and the copy the user just made remains available.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_and_rejects_captures() {
        assert_eq!(clamp_capture("  안녕  \n").unwrap(), "안녕");
        assert_eq!(
            clamp_capture("   ").unwrap_err(),
            "선택한 텍스트가 없습니다."
        );
        let long = "a".repeat(20_001);
        assert_eq!(
            clamp_capture(&long).unwrap_err(),
            "텍스트가 너무 깁니다. 20,000자 이하로 나눠서 번역하세요."
        );
    }
}
