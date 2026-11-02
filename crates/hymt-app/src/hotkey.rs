//! Global hotkey triggers for quick translation.
//!
//! Two mechanisms live on one dedicated thread, drained by [`Root::tick`](crate::root::Root)
//! like the tray events:
//!
//! * A modifier chord (`Ctrl+Shift+C`) through `RegisterHotKey`, which delivers `WM_HOTKEY`.
//! * A "double press" trigger (`Ctrl+C` twice) observed with a passive low-level keyboard
//!   hook (`WH_KEYBOARD_LL`), because `RegisterHotKey` cannot express repeated keys. The
//!   hook never swallows input, so an ordinary `Ctrl+C` still updates the clipboard and the
//!   double press picks the fresh text up without synthesizing another copy.
//!
//! Bindings are re-applied through [`Hotkey::update`] whenever the user edits the settings;
//! registration failures come back as [`HotkeyEvent::Registration`] events that the UI
//! shows as toasts and a status line.

use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    MOD_SHIFT, MOD_WIN, VK_CONTROL, VK_LWIN, VK_MENU, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostMessageW,
    RegisterClassW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HHOOK, HWND_MESSAGE,
    KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_DESTROY,
    WM_HOTKEY, WNDCLASSW,
};

use hymt_core::settings::{is_supported_key, HotkeyCombo, HotkeySettings};

/// Time between the two presses of a double-press trigger, in milliseconds.
pub const DOUBLE_TAP_GAP_MS: u64 = 300;
/// Sampling interval for the clipboard fallback while waiting for the copy to land.
pub const CLIPBOARD_POLL_INTERVAL_MS: u64 = 16;
/// How long the clipboard fallback waits for the application to copy the selection.
pub const CLIPBOARD_COPY_TIMEOUT_MS: u64 = 250;
/// Extra room above [`DOUBLE_TAP_GAP_MS`] given in the recording UI, so a second press the
/// hook would already have missed can still be recorded.
pub const DOUBLE_TAP_RECORD_SLACK_MS: u64 = 100;

/// Which side of the recorder the settings UI captures next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecorderSlot {
    /// Modifier chord.
    Chord,
    /// Double press.
    DoubleTap,
}

/// One trigger configuration sent to the hotkey thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotkeySpecs {
    /// Chord registered through `RegisterHotKey`.
    pub chord: Option<ChordSpec>,
    /// Double press observed through the keyboard hook.
    pub tap: Option<TapSpec>,
}

impl HotkeySpecs {
    /// Both triggers cleared.
    pub const NONE: Self = Self {
        chord: None,
        tap: None,
    };
}

/// A `RegisterHotKey` chord in Win32 terms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChordSpec {
    /// Windows virtual-key code of the bound key.
    pub vk: u32,
    /// `MOD_*` flags, including `MOD_NOREPEAT`.
    pub mods: u32,
}

/// A double-press trigger in Win32 terms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TapSpec {
    /// Windows virtual-key code pressed twice.
    pub vk: u32,
    /// Windows virtual-key code of the single required modifier (`Ctrl` by default).
    pub mod_vk: u32,
}

/// What the hotkey thread reports to the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HotkeyEvent {
    /// A trigger fired.
    ///
    /// `clipboard_seq` is set only when the double press found that the clipboard sequence
    /// number grew between the two presses, meaning the source application copied the
    /// selection itself and [`capture_selected`](crate::capture) may read the clipboard
    /// directly.
    Triggered { clipboard_seq: Option<u32> },
    /// Outcome of the latest registration attempt. `Err` carries a user-facing message.
    Registration(Result<(), String>),
}

enum HotkeyCommand {
    Replace(HotkeySpecs),
}

/// Owns the hotkey thread's channels on the UI side.
pub struct Hotkey {
    cmd_tx: Sender<HotkeyCommand>,
    events: Receiver<HotkeyEvent>,
}

impl Hotkey {
    /// Spawns the thread and applies `specs` once its message loop is up.
    ///
    /// The supplied configuration is applied even when both triggers are `None` (feature
    /// disabled), so the thread always starts with a coherent picture.
    pub fn start(specs: HotkeySpecs) -> Result<Self, String> {
        let (event_tx, events) = mpsc::channel();
        let (cmd_tx, cmd_rx) = mpsc::channel();
        cmd_tx
            .send(HotkeyCommand::Replace(specs))
            .map_err(|err| err.to_string())?;
        thread::Builder::new()
            .name("hymt-hotkey".to_string())
            .spawn(move || unsafe { run_hotkey_thread(cmd_rx, event_tx) })
            .map_err(|err| err.to_string())?;
        Ok(Self { cmd_tx, events })
    }

    /// Applies a new trigger configuration.
    pub fn update(&self, specs: HotkeySpecs) {
        if self.cmd_tx.send(HotkeyCommand::Replace(specs)).is_ok() {
            wake_hotkey_thread();
        }
    }

    /// Returns the next event, if any. Called from `Root::tick`.
    pub fn poll(&self) -> Option<HotkeyEvent> {
        self.events.try_recv().ok()
    }
}

/// Win32 window handle of the hotkey thread's message window, published by the thread
/// itself so the UI can wake it after queueing commands.
static HOTKEY_WINDOW: AtomicUsize = AtomicUsize::new(0);

/// Private message that makes the thread's loop drain queued commands.
const WM_APP_REPLACE: u32 = WM_APP + 42;

/// Posts [`WM_APP_REPLACE`] so pending `update` calls are applied promptly.
fn wake_hotkey_thread() {
    let raw = HOTKEY_WINDOW.load(Ordering::Acquire);
    if raw != 0 {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(raw as *mut _)),
                WM_APP_REPLACE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

thread_local! {
    static STATE: RefCell<ThreadState> = const { RefCell::new(ThreadState::EMPTY) };
}

struct ThreadState {
    hwnd: HWND,
    events_tx: Option<Sender<HotkeyEvent>>,
    cmd_rx: Option<Receiver<HotkeyCommand>>,
    hook: Option<HHOOK>,
    chord: Option<ChordSpec>,
    tap: Option<TapSpec>,
    tracker: TapTracker,
    /// Clipboard sequence number sampled at the first press of a double press.
    first_press_seq: Option<u32>,
}

impl Default for ThreadState {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl ThreadState {
    const EMPTY: Self = Self {
        hwnd: HWND(std::ptr::null_mut()),
        events_tx: None,
        cmd_rx: None,
        hook: None,
        chord: None,
        tap: None,
        tracker: TapTracker::IDLE,
        first_press_seq: None,
    };
}

unsafe fn run_hotkey_thread(cmd_rx: Receiver<HotkeyCommand>, events_tx: Sender<HotkeyEvent>) {
    STATE.with_borrow_mut(|state| {
        state.cmd_rx = Some(cmd_rx);
        state.events_tx = Some(events_tx);
    });

    match create_message_window() {
        Ok(()) => {}
        Err(message) => {
            STATE.with_borrow_mut(|state| {
                if let Some(events_tx) = &state.events_tx {
                    let _ = events_tx.send(HotkeyEvent::Registration(Err(message)));
                }
            });
            return;
        }
    }

    // Drain whatever the UI queued while the message window was being created, then run.
    drain_replace_commands();

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}

/// Registers the window class and creates a message-only window for the hotkey ids.
unsafe fn create_message_window() -> Result<(), String> {
    let class_name = HSTRING::from("HymtHotkeyWindow");
    let class_ptr = PCWSTR(class_name.as_ptr());
    let hinstance =
        GetModuleHandleW(None).map_err(|err| format!("핫키 창 모듈을 찾을 수 없습니다: {err}"))?;

    let class = WNDCLASSW {
        style: Default::default(),
        lpfnWndProc: Some(hotkey_wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinstance.into(),
        hIcon: Default::default(),
        hCursor: Default::default(),
        hbrBackground: Default::default(),
        lpszMenuName: PCWSTR::null(),
        lpszClassName: class_ptr,
    };
    if RegisterClassW(&class) == 0 {
        return Err("핫키 창 클래스를 등록할 수 없습니다.".to_string());
    }

    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class_ptr,
        PCWSTR::null(),
        WINDOW_STYLE(0),
        0,
        0,
        0,
        0,
        Some(HWND_MESSAGE),
        None,
        Some::<HINSTANCE>(hinstance.into()),
        None,
    )
    .map_err(|err| format!("핫키 창을 만들 수 없습니다: {err}"))?;

    STATE.with_borrow_mut(|state| state.hwnd = hwnd);
    HOTKEY_WINDOW.store(hwnd.0 as usize, Ordering::Release);
    Ok(())
}

/// Pulls queued `Replace` commands out of the thread-local and applies them one by one.
///
/// Commands are collected first so the application never runs while the thread-local is
/// borrowed.
unsafe fn drain_replace_commands() {
    let mut queued: Vec<HotkeySpecs> = Vec::new();
    STATE.with_borrow_mut(|state| {
        while let Some(cmd_rx) = state.cmd_rx.as_ref() {
            match cmd_rx.try_recv() {
                Ok(HotkeyCommand::Replace(specs)) => queued.push(specs),
                Err(_) => break,
            }
        }
    });
    for specs in queued {
        replace_specs(specs);
    }
}

/// Applies one set of trigger bindings: chord through `RegisterHotKey`, double press
/// through the keyboard hook. Reports the combined outcome once.
unsafe fn replace_specs(specs: HotkeySpecs) {
    let mut errors: Vec<String> = Vec::new();
    STATE.with_borrow_mut(|state| {
        // Chord: always unregister the old id first; registering a fresh one replaces it.
        let _ = UnregisterHotKey(Some(state.hwnd), 1);
        state.chord = None;
        if let Some(chord) = specs.chord {
            match RegisterHotKey(Some(state.hwnd), 1, HOT_KEY_MODIFIERS(chord.mods), chord.vk) {
                Ok(()) => state.chord = Some(chord),
                Err(err) => errors.push(format!(
                    "조합 단축키를 등록할 수 없습니다 (다른 프로그램이 이미 사용 중일 수 있습니다): {err}"
                )),
            }
        }

        state.tap = specs.tap;
        state.tracker.set_spec(specs.tap);
        match (specs.tap, state.hook) {
            (Some(_), None) => {
                match GetModuleHandleW(None) {
                    Ok(hinstance) => match SetWindowsHookExW(
                        WH_KEYBOARD_LL,
                        Some(hotkey_hookproc),
                        Some::<HINSTANCE>(hinstance.into()),
                        0,
                    ) {
                        Ok(hook) => state.hook = Some(hook),
                        Err(err) => {
                            state.tap = None;
                            state.tracker.set_spec(None);
                            errors
                                .push(format!("두 번 누르기 감지를 설치할 수 없습니다: {err}"));
                        }
                    },
                    Err(err) => {
                        state.tap = None;
                        state.tracker.set_spec(None);
                        errors.push(format!("두 번 누르기 감지 프로그램을 찾을 수 없습니다: {err}"));
                    }
                };
            }
            (None, Some(hook)) => {
                let _ = UnhookWindowsHookEx(hook);
                state.hook = None;
            }
            _ => {}
        }
        state.first_press_seq = None;
    });

    STATE.with_borrow_mut(|state| {
        if let Some(events_tx) = state.events_tx.as_ref() {
            let outcome = if errors.is_empty() {
                Ok(())
            } else {
                Err(errors.join("\n"))
            };
            let _ = events_tx.send(HotkeyEvent::Registration(outcome));
        }
    });
}

/// Handles the hotkey thread's window messages.
unsafe extern "system" fn hotkey_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_HOTKEY if wparam.0 == 1 => {
            emit_triggered(None);
        }
        WM_APP_REPLACE => {
            drain_replace_commands();
        }
        WM_DESTROY => {
            cleanup_thread_state(hwnd);
        }
        _ => return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
    LRESULT(0)
}

/// Reports a plain trigger (chord path) through the shared channel.
fn emit_triggered(clipboard_seq: Option<u32>) {
    STATE.with_borrow_mut(|state| {
        if let Some(events_tx) = state.events_tx.as_ref() {
            let _ = events_tx.send(HotkeyEvent::Triggered { clipboard_seq });
        }
    });
}

/// Removes the hook and the chord registration while the thread window goes away.
unsafe fn cleanup_thread_state(hwnd: HWND) {
    STATE.with_borrow_mut(|state| {
        if let Some(hook) = state.hook.take() {
            let _ = UnhookWindowsHookEx(hook);
        }
        let _ = UnregisterHotKey(Some(hwnd), 1);
        state.chord = None;
        HOTKEY_WINDOW.store(0, Ordering::Release);
    });
}

const WM_KEYDOWN: u32 = 0x0100;
const WM_KEYUP: u32 = 0x0101;
const WM_SYSKEYDOWN: u32 = 0x0104;
const WM_SYSKEYUP: u32 = 0x0105;

/// True for the four key messages the tracker consumes.
fn is_key_message(wparam: WPARAM) -> bool {
    matches!(
        wparam.0 as u32,
        WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP
    )
}

/// Passive low-level keyboard hook: observes `Ctrl+C C` patterns without ever touching
/// the keys applications receive.
unsafe extern "system" fn hotkey_hookproc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    if is_key_message(wparam) {
        let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let vk = info.vkCode;
        let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        let now = GetTickCount64();
        STATE.with_borrow_mut(|state| {
            let step = state.tracker.feed(vk, down, now);
            match step {
                TapStep::FirstDown => {
                    state.first_press_seq = Some(GetClipboardSequenceNumber());
                }
                TapStep::Triggered => {
                    let first_seq = state.first_press_seq.take();
                    let now_seq = GetClipboardSequenceNumber();
                    let clipboard_seq =
                        first_seq.filter(|first| *first != now_seq).map(|_| now_seq);
                    if let Some(events_tx) = state.events_tx.as_ref() {
                        let _ = events_tx.send(HotkeyEvent::Triggered { clipboard_seq });
                    }
                }
                TapStep::Nothing => {}
            }
        });
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// Result of feeding one key event into [`TapTracker`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapStep {
    /// No progress of note.
    Nothing,
    /// The first press of the tracked key just landed.
    FirstDown,
    /// The second press completed the pattern.
    Triggered,
}

/// Modifier kinds merged from their left/right virtual keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModifierKind {
    Ctrl,
    Shift,
    Alt,
    Win,
}

/// Maps a virtual-key code onto its logical modifier when it is one.
pub fn modifier_kind(vk: u32) -> Option<ModifierKind> {
    match vk {
        0x11 | 0xA2 | 0xA3 => Some(ModifierKind::Ctrl), // VK_CONTROL/LCONTROL/RCONTROL
        0x10 | 0xA0 | 0xA1 => Some(ModifierKind::Shift), // VK_SHIFT/LSHIFT/RSHIFT
        0x12 | 0xA4 | 0xA5 => Some(ModifierKind::Alt),  // VK_MENU/LMENU/RMENU
        0x5B | 0x5C => Some(ModifierKind::Win),         // VK_LWIN/VK_RWIN
        _ => None,
    }
}

/// Passive double-press state machine.
///
/// The tracker sees the whole key stream, so it keeps its own record of which modifiers
/// are down: the pattern matches only when the required modifier is held, no other
/// modifier is held, the same key goes down-up-down within [`DOUBLE_TAP_GAP_MS`], and no
/// other key is pressed in between.
pub struct TapTracker {
    spec: Option<TapSpec>,
    held_ctrl: bool,
    held_shift: bool,
    held_alt: bool,
    held_win: bool,
    first_down_at: Option<u64>,
    tracked_key_down: bool,
}

impl TapTracker {
    /// State with the pattern unset.
    pub const IDLE: Self = Self {
        spec: None,
        held_ctrl: false,
        held_shift: false,
        held_alt: false,
        held_win: false,
        first_down_at: None,
        tracked_key_down: false,
    };

    /// Swaps the observed pattern and forgets any half-completed press.
    pub fn set_spec(&mut self, spec: Option<TapSpec>) {
        *self = Self { spec, ..Self::IDLE };
    }

    /// True when exactly the required modifier (and no other) is held.
    fn modifiers_match(&self, spec: &TapSpec) -> bool {
        let required = modifier_kind(spec.mod_vk);
        let states = [
            (ModifierKind::Ctrl, self.held_ctrl),
            (ModifierKind::Shift, self.held_shift),
            (ModifierKind::Alt, self.held_alt),
            (ModifierKind::Win, self.held_win),
        ];
        states.iter().all(|(kind, held)| {
            if Some(*kind) == required {
                *held
            } else {
                !*held
            }
        })
    }

    /// Invalidates the pending first press.
    fn reset(&mut self) {
        self.first_down_at = None;
    }

    /// Feeds one key event, returning how far the pattern progressed.
    pub fn feed(&mut self, vk: u32, down: bool, now_ms: u64) -> TapStep {
        let Some(spec) = self.spec else {
            return TapStep::Nothing;
        };
        match modifier_kind(vk) {
            Some(ModifierKind::Ctrl) => self.held_ctrl = down,
            Some(ModifierKind::Shift) => self.held_shift = down,
            Some(ModifierKind::Alt) => self.held_alt = down,
            Some(ModifierKind::Win) => self.held_win = down,
            None => {
                if down {
                    if vk == spec.vk {
                        if self.tracked_key_down {
                            // Auto-repeat keydown without a release can never be the
                            // second press; the pattern requires down-up-down.
                            return TapStep::Nothing;
                        }
                        self.tracked_key_down = true;
                        if !self.modifiers_match(&spec) {
                            self.reset();
                            return TapStep::Nothing;
                        }
                        if let Some(first_at) = self.first_down_at {
                            if now_ms.saturating_sub(first_at) <= DOUBLE_TAP_GAP_MS {
                                self.first_down_at = None;
                                return TapStep::Triggered;
                            }
                        }
                        self.first_down_at = Some(now_ms);
                        return TapStep::FirstDown;
                    }
                    self.reset();
                } else if vk == spec.vk {
                    self.tracked_key_down = false;
                }
            }
        }
        // A modifier change can invalidate the pending first press.
        if self.first_down_at.is_some() && !self.modifiers_match(&spec) {
            self.reset();
        }
        TapStep::Nothing
    }
}

/// Maps one stored key name onto its Windows virtual-key code.
pub fn vk_for_key(key: &str) -> Option<u32> {
    match key {
        "space" => Some(0x20),
        "tab" => Some(0x09),
        "enter" => Some(0x0D),
        _ => {
            let mut chars = key.chars();
            match (chars.next(), chars.next()) {
                (Some(single), None) => match single {
                    'a'..='z' => Some(0x41 + (single as u32 - 'a' as u32)),
                    '0'..='9' => Some(0x30 + (single as u32 - '0' as u32)),
                    other => oem_vkey(other),
                },
                _ => key
                    .strip_prefix('f')
                    .and_then(|digits| digits.parse::<u32>().ok())
                    .filter(|number| (1..=24).contains(number))
                    .map(|number| 0x70 + number - 1),
            }
        }
    }
}

/// `VK_OEM_*` codes for the punctuation keys we support.
fn oem_vkey(ch: char) -> Option<u32> {
    match ch {
        '-' => Some(0xBD),
        '=' => Some(0xBB),
        ',' => Some(0xBC),
        '.' => Some(0xBE),
        '/' => Some(0xBF),
        '`' => Some(0xC0),
        ';' => Some(0xBA),
        '\'' => Some(0xDE),
        '[' => Some(0xDB),
        '\\' => Some(0xDC),
        ']' => Some(0xDD),
        _ => None,
    }
}

/// Parses the persisted chord into a `RegisterHotKey` spec.
pub fn parse_chord(combo: &HotkeyCombo) -> Option<ChordSpec> {
    if !is_supported_key(&combo.key) || !combo.has_modifier() {
        return None;
    }
    let vk = vk_for_key(&combo.key)?;
    let mut mods = MOD_NOREPEAT.0;
    if combo.ctrl {
        mods |= MOD_CONTROL.0;
    }
    if combo.shift {
        mods |= MOD_SHIFT.0;
    }
    if combo.alt {
        mods |= MOD_ALT.0;
    }
    if combo.win {
        mods |= MOD_WIN.0;
    }
    Some(ChordSpec { vk, mods })
}

/// Parses the persisted double press into its two keys.
pub fn parse_tap(combo: &HotkeyCombo) -> Option<TapSpec> {
    if !is_supported_key(&combo.key) {
        return None;
    }
    let modifier_count =
        combo.ctrl as u32 + combo.shift as u32 + combo.alt as u32 + combo.win as u32;
    if modifier_count != 1 {
        return None;
    }
    let mod_vk = if combo.ctrl {
        VK_CONTROL.0
    } else if combo.shift {
        VK_SHIFT.0
    } else if combo.alt {
        VK_MENU.0
    } else {
        VK_LWIN.0
    };
    Some(TapSpec {
        vk: vk_for_key(&combo.key)?,
        mod_vk: u32::from(mod_vk),
    })
}

/// Reads the persisted settings into the specs the thread applies.
pub fn parse_specs(settings: &HotkeySettings) -> HotkeySpecs {
    if !settings.enabled {
        return HotkeySpecs::NONE;
    }
    let chord = settings.combo.as_ref().and_then(parse_chord);
    let tap = settings.double_tap.as_ref().and_then(parse_tap);
    HotkeySpecs { chord, tap }
}

/// Display name of one stored key (`"c"` -> `C`, `"space"` -> `Space`, …).
pub fn display_key(key: &str) -> String {
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(single), None) => single.to_ascii_uppercase().to_string(),
        _ => match key {
            "space" => "Space".to_string(),
            "tab" => "Tab".to_string(),
            "enter" => "Enter".to_string(),
            fkey if key.starts_with('f') && key[1..].parse::<u32>().is_ok() => fkey.to_uppercase(),
            other => other.to_string(),
        },
    }
}

/// Korean display for a chord, e.g. `Ctrl+Shift+C`.
pub fn format_combo(combo: &HotkeyCombo) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if combo.ctrl {
        parts.push("Ctrl");
    }
    if combo.shift {
        parts.push("Shift");
    }
    if combo.alt {
        parts.push("Alt");
    }
    if combo.win {
        parts.push("Win");
    }
    let key = display_key(&combo.key);
    parts.push(&key);
    parts.join("+")
}

/// Korean display for the double-press trigger, e.g. `Ctrl+C 두 번`.
pub fn format_double_tap(combo: &HotkeyCombo) -> String {
    format!("{} 두 번", format_combo(combo))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combo(ctrl: bool, shift: bool, alt: bool, win: bool, key: &str) -> HotkeyCombo {
        HotkeyCombo {
            ctrl,
            shift,
            alt,
            win,
            key: key.to_string(),
        }
    }

    #[test]
    fn maps_letters_digits_and_named_keys() {
        assert_eq!(vk_for_key("a"), Some(0x41));
        assert_eq!(vk_for_key("c"), Some(0x43));
        assert_eq!(vk_for_key("0"), Some(0x30));
        assert_eq!(vk_for_key("-"), Some(0xBD));
        assert_eq!(vk_for_key("["), Some(0xDB));
        assert_eq!(vk_for_key("space"), Some(0x20));
        assert_eq!(vk_for_key("f12"), Some(0x70 + 11));
        assert!(vk_for_key("f25").is_none());
        assert!(vk_for_key("").is_none());
        assert!(vk_for_key("hello").is_none());
    }

    #[test]
    fn parses_only_usable_chords() {
        let usable = parse_chord(&combo(true, true, false, false, "c")).unwrap();
        assert_eq!(usable.vk, 0x43);
        assert_ne!(usable.mods & MOD_CONTROL.0, 0);
        assert_ne!(usable.mods & MOD_NOREPEAT.0, 0);
        assert!(parse_chord(&combo(true, false, false, false, "ctrl")).is_none());
        assert!(parse_chord(&combo(false, false, false, false, "c")).is_none());
    }

    #[test]
    fn parses_only_single_modifier_double_press() {
        let tap = parse_tap(&combo(true, false, false, false, "c")).unwrap();
        assert_eq!(tap.vk, 0x43);
        assert_eq!(tap.mod_vk, u32::from(VK_CONTROL.0));
        assert!(parse_tap(&combo(true, true, false, false, "c")).is_none());
        assert!(parse_tap(&combo(false, false, false, false, "c")).is_none());
    }

    #[test]
    fn parse_specs_honours_the_master_switch() {
        let disabled = HotkeySettings {
            enabled: false,
            ..HotkeySettings::default()
        };
        assert_eq!(parse_specs(&disabled), HotkeySpecs::NONE);
        let enabled = HotkeySettings {
            enabled: true,
            ..HotkeySettings::default()
        };
        let specs = parse_specs(&enabled);
        assert!(specs.chord.is_some() && specs.tap.is_some());
    }

    #[test]
    fn renders_korean_labels() {
        assert_eq!(
            format_combo(&combo(true, true, false, false, "c")),
            "Ctrl+Shift+C"
        );
        assert_eq!(
            format_combo(&combo(false, false, true, false, "f4")),
            "Alt+F4"
        );
        assert_eq!(
            format_double_tap(&combo(true, false, false, false, "c")),
            "Ctrl+C 두 번"
        );
        assert_eq!(display_key("space"), "Space");
    }

    fn tap_tracker() -> TapTracker {
        let mut tracker = TapTracker::IDLE;
        tracker.set_spec(Some(TapSpec {
            vk: 0x43,
            mod_vk: u32::from(VK_CONTROL.0),
        }));
        tracker
    }

    #[test]
    fn double_press_within_gap_triggers() {
        let mut tracker = tap_tracker(); // Ctrl+C C
        assert_eq!(tracker.feed(0xA2, true, 100), TapStep::Nothing); // left ctrl
        assert_eq!(tracker.feed(0x43, true, 110), TapStep::FirstDown);
        assert_eq!(tracker.feed(0x43, false, 140), TapStep::Nothing);
        assert_eq!(tracker.feed(0x43, true, 240), TapStep::Triggered);
        // The pattern re-arms from scratch after firing.
        assert_eq!(tracker.feed(0x43, false, 260), TapStep::Nothing);
        assert_eq!(tracker.feed(0x43, true, 300), TapStep::FirstDown);
    }

    #[test]
    fn gap_too_long_never_triggers() {
        let mut tracker = tap_tracker();
        tracker.feed(0xA2, true, 0);
        assert_eq!(tracker.feed(0x43, true, 0), TapStep::FirstDown);
        tracker.feed(0x43, false, 50);
        assert_eq!(
            tracker.feed(0x43, true, DOUBLE_TAP_GAP_MS + 100),
            TapStep::FirstDown
        );
    }

    #[test]
    fn another_key_resets_the_first_press() {
        let mut tracker = tap_tracker();
        tracker.feed(0xA2, true, 0);
        assert_eq!(tracker.feed(0x43, true, 0), TapStep::FirstDown);
        assert_eq!(tracker.feed(0x43, false, 5), TapStep::Nothing);
        assert_eq!(tracker.feed(0x44, true, 10), TapStep::Nothing);
        assert_eq!(tracker.feed(0x44, false, 12), TapStep::Nothing);
        // A fresh press of the tracked key starts a new pattern.
        assert_eq!(tracker.feed(0x43, true, 20), TapStep::FirstDown);
    }

    #[test]
    fn wrong_modifier_state_does_not_trigger() {
        let mut tracker = tap_tracker();
        tracker.feed(0xA2, true, 0);
        // Shift alongside Ctrl never completes the pattern.
        assert_eq!(tracker.feed(0xA0, true, 5), TapStep::Nothing);
        assert_eq!(tracker.feed(0x43, true, 10), TapStep::Nothing);
        // Ctrl-only again: the first press can be re-taken after release.
        assert_eq!(tracker.feed(0xA0, false, 15), TapStep::Nothing);
        assert_eq!(tracker.feed(0x43, false, 20), TapStep::Nothing);
        assert_eq!(tracker.feed(0x43, true, 25), TapStep::FirstDown);
        assert_eq!(tracker.feed(0x43, false, 40), TapStep::Nothing);
        assert_eq!(tracker.feed(0x43, true, 60), TapStep::Triggered);
    }

    #[test]
    fn releasing_the_modifier_cancels_the_pending_press() {
        let mut tracker = tap_tracker();
        tracker.feed(0xA2, true, 0);
        assert_eq!(tracker.feed(0x43, true, 0), TapStep::FirstDown);
        assert_eq!(tracker.feed(0x43, false, 20), TapStep::Nothing);
        assert_eq!(tracker.feed(0xA2, false, 25), TapStep::Nothing);
        // Press the key again without Ctrl: no first press is recorded.
        assert_eq!(tracker.feed(0x43, true, 30), TapStep::Nothing);
        assert_eq!(tracker.feed(0xA2, true, 35), TapStep::Nothing);
        assert_eq!(tracker.feed(0x43, false, 40), TapStep::Nothing);
        assert_eq!(tracker.feed(0x43, true, 50), TapStep::FirstDown);
    }

    #[test]
    fn auto_repeat_without_release_does_not_retrigger() {
        let mut tracker = tap_tracker();
        tracker.feed(0xA2, true, 0);
        assert_eq!(tracker.feed(0x43, true, 0), TapStep::FirstDown);
        // Auto-repeat keydowns arrive without a key up in between.
        assert_eq!(tracker.feed(0x43, true, 10), TapStep::Nothing);
        assert_eq!(tracker.feed(0x43, true, 20), TapStep::Nothing);
    }

    #[test]
    fn idle_tracker_never_fires() {
        let mut tracker = TapTracker::IDLE;
        assert_eq!(tracker.feed(0x43, true, 0), TapStep::Nothing);
        assert_eq!(tracker.feed(0x43, false, 10), TapStep::Nothing);
    }
}
