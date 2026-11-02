//! A small, calm dark theme. Colors are plain `u32` values so the palette can be reviewed at
//! a glance; widgets convert them with [`gpui::rgb`].

/// Window background.
pub const BG: u32 = 0x0e1116;
/// Raised panels and cards.
pub const PANEL: u32 = 0x161a21;
/// Slightly lighter panel, used for inputs and rows.
pub const PANEL_ALT: u32 = 0x1c212a;
/// Hover state.
pub const PANEL_HOVER: u32 = 0x232a35;
/// Hairline borders.
pub const BORDER: u32 = 0x2b3341;
/// Primary text.
pub const TEXT: u32 = 0xe7ebf3;
/// Secondary text.
pub const MUTED: u32 = 0x8f9aab;
/// Disabled text.
pub const DISABLED: u32 = 0x5a6474;
/// Accent (buttons, selection, focus).
pub const ACCENT: u32 = 0x5b8cff;
/// Translucent-looking accent background for selected chips.
pub const ACCENT_SOFT: u32 = 0x1e2f52;
/// Success.
pub const OK: u32 = 0x3ddc97;
/// Warning.
pub const WARN: u32 = 0xf0a83c;
/// Failure.
pub const ERROR: u32 = 0xff6f6f;
/// Selection highlight in text areas.
pub const SELECTION: u32 = 0x2f4a7d;
/// Placeholder text.
pub const PLACEHOLDER: u32 = 0x5d6777;
/// Caret color.
pub const CARET: u32 = 0x8fb2ff;
