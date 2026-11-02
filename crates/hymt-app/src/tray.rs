//! Notification-area icon and its context menu.
//!
//! The icon is created on the UI thread during application startup, and its events are drained
//! by [`Root::tick`](crate::root::Root). Left click restores the window; the menu also offers
//! "열기" and "종료".

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

const MENU_SHOW: &str = "hymt.show";
const MENU_QUIT: &str = "hymt.quit";
const ICON_SIZE: u32 = 32;
const ICON_SUPERSAMPLE: u32 = 4;

/// What the user asked for from the tray icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCommand {
    /// Bring the main window back.
    Show,
    /// Stop the engine and exit the application.
    Quit,
}

/// Owns the native tray icon for as long as the setting is enabled.
pub struct Tray {
    _icon: TrayIcon,
}

impl Tray {
    /// Creates the icon and its menu.
    ///
    /// Must be called on the UI thread, after the application has started.
    pub fn new() -> Result<Self, String> {
        // Drop events left over from a previous icon (for example after toggling the setting).
        drain_events();

        let menu = Menu::new();
        let show = MenuItem::with_id(MENU_SHOW, "열기", true, None);
        let separator = PredefinedMenuItem::separator();
        let quit = MenuItem::with_id(MENU_QUIT, "종료", true, None);
        menu.append_items(&[&show, &separator, &quit])
            .map_err(|err| err.to_string())?;

        let icon =
            Icon::from_rgba(icon_rgba(), ICON_SIZE, ICON_SIZE).map_err(|err| err.to_string())?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("HyMT — 오프라인 번역")
            .with_icon(icon)
            .with_menu_on_left_click(false)
            .build()
            .map_err(|err| err.to_string())?;

        Ok(Self { _icon: icon })
    }

    /// Returns the next command, if the user clicked the icon or a menu item.
    pub fn poll(&self) -> Option<TrayCommand> {
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                return Some(TrayCommand::Show);
            }
        }

        while let Ok(event) = MenuEvent::receiver().try_recv() {
            match event.id.0.as_str() {
                MENU_SHOW => return Some(TrayCommand::Show),
                MENU_QUIT => return Some(TrayCommand::Quit),
                _ => {}
            }
        }

        None
    }
}

fn drain_events() {
    while TrayIconEvent::receiver().try_recv().is_ok() {}
    while MenuEvent::receiver().try_recv().is_ok() {}
}

/// Draws a rounded blue tile with a white "H", supersampled for clean edges.
fn icon_rgba() -> Vec<u8> {
    let side = ICON_SIZE as f32;
    let samples = (ICON_SUPERSAMPLE * ICON_SUPERSAMPLE) as f32;
    let mut rgba = vec![0u8; (ICON_SIZE * ICON_SIZE * 4) as usize];

    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let mut coverage = 0.0;
            let mut glyph = 0.0;
            for sy in 0..ICON_SUPERSAMPLE {
                for sx in 0..ICON_SUPERSAMPLE {
                    let px = x as f32 + (sx as f32 + 0.5) / ICON_SUPERSAMPLE as f32;
                    let py = y as f32 + (sy as f32 + 0.5) / ICON_SUPERSAMPLE as f32;
                    if inside_rounded_square(px, py, 1.5, 1.5, side - 3.0, 7.0) {
                        coverage += 1.0;
                        if inside_h(px, py) {
                            glyph += 1.0;
                        }
                    }
                }
            }

            if coverage == 0.0 {
                continue;
            }
            let mix = glyph / coverage;
            let index = ((y * ICON_SIZE + x) * 4) as usize;
            rgba[index] = blend(37, 255, mix);
            rgba[index + 1] = blend(99, 255, mix);
            rgba[index + 2] = blend(235, 255, mix);
            rgba[index + 3] = (coverage / samples * 255.0).round() as u8;
        }
    }
    rgba
}

fn blend(from: u8, to: u8, t: f32) -> u8 {
    (from as f32 + (to as f32 - from as f32) * t).round() as u8
}

fn inside_rounded_square(x: f32, y: f32, left: f32, top: f32, size: f32, radius: f32) -> bool {
    let right = left + size;
    let bottom = top + size;
    if x < left || x > right || y < top || y > bottom {
        return false;
    }
    let cx = x.clamp(left + radius, right - radius);
    let cy = y.clamp(top + radius, bottom - radius);
    let dx = x - cx;
    let dy = y - cy;
    dx * dx + dy * dy <= radius * radius
}

fn inside_h(x: f32, y: f32) -> bool {
    let (left, right) = (9.0, 23.0);
    let (top, bottom) = (8.0, 24.0);
    let bar = 3.5;
    (x >= left && x <= left + bar && y >= top && y <= bottom)
        || (x >= right - bar && x <= right && y >= top && y <= bottom)
        || (x >= left && x <= right && (14.5..=17.5).contains(&y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_is_opaque_in_the_middle_and_clear_at_the_edges() {
        let pixels = icon_rgba();
        assert_eq!(pixels.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        let alpha = |x: u32, y: u32| pixels[((y * ICON_SIZE + x) * 4 + 3) as usize];
        assert_eq!(alpha(16, 16), 255);
        assert_eq!(alpha(0, 0), 0);
    }
}
