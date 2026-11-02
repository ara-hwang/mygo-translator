//! Small Win32 helpers for showing and hiding the main window.
//!
//! GPUI has no public "hide window" API, but its window exposes a raw Win32 handle. The tray
//! feature needs to hide the window on close and bring it back from the notification area.

use gpui_kit::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, IsIconic, SetForegroundWindow, ShowWindow, SW_HIDE, SW_RESTORE, SW_SHOW,
};

/// Hides the window without destroying it, keeping GPUI's state intact.
///
/// Returns false when the native handle is unavailable, so callers can fall back to closing.
pub fn hide(window: &Window) -> bool {
    if let Some(hwnd) = hwnd(window) {
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        true
    } else {
        false
    }
}

/// Restores and focuses the window after it was hidden or minimized.
pub fn show(window: &mut Window) {
    if let Some(hwnd) = hwnd(window) {
        unsafe {
            // SW_RESTORE would also undo a maximized layout, so only use it when the window
            // is actually minimized.
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            } else {
                let _ = ShowWindow(hwnd, SW_SHOW);
            }
            let _ = SetForegroundWindow(hwnd);
        }
    }
    window.activate_window();
}

fn hwnd(window: &Window) -> Option<HWND> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Some(HWND(handle.hwnd.get() as *mut std::ffi::c_void)),
        _ => None,
    }
}

/// Win32 handle of the window that currently holds keyboard focus.
unsafe fn foreground_hwnd() -> Option<HWND> {
    let hwnd = GetForegroundWindow();
    if hwnd.is_invalid() {
        None
    } else {
        Some(hwnd)
    }
}

/// True when the app's own window is the foreground window.
///
/// Firing the hotkey while our window holds focus would capture our editor instead of
/// another application's selection, so callers re-translate the current text directly.
pub fn foreground_is_own(window: &Window) -> bool {
    match (hwnd(window), unsafe { foreground_hwnd() }) {
        (Some(ours), Some(current)) => ours == current,
        _ => false,
    }
}
