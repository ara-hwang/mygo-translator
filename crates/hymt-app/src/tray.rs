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

/// Draws the app icon at 32 px: a blue→cyan rounded tile with a white card carrying "가"
/// and a light card carrying "A". Same design as `assets/icon-512.png`, redrawn here so the
/// tray bitmap stays pixel-exact without pulling an image decoder into the binary.
fn icon_rgba() -> Vec<u8> {
    let side = ICON_SIZE as f32;
    let samples = (ICON_SUPERSAMPLE * ICON_SUPERSAMPLE) as f32;
    let mut rgba = vec![0u8; (ICON_SIZE * ICON_SIZE * 4) as usize];

    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let mut coverage = 0.0;
            let mut sum = [0.0f32; 3];
            for sy in 0..ICON_SUPERSAMPLE {
                for sx in 0..ICON_SUPERSAMPLE {
                    let px = x as f32 + (sx as f32 + 0.5) / ICON_SUPERSAMPLE as f32;
                    let py = y as f32 + (sy as f32 + 0.5) / ICON_SUPERSAMPLE as f32;
                    let color = match sample_color(px, py, side) {
                        Some(color) => color,
                        None => continue,
                    };
                    coverage += 1.0;
                    for channel in 0..3 {
                        sum[channel] += color[channel];
                    }
                }
            }

            if coverage == 0.0 {
                continue;
            }
            let index = ((y * ICON_SIZE + x) * 4) as usize;
            for channel in 0..3 {
                rgba[index + channel] = blend_channel(sum[channel] / coverage);
            }
            rgba[index + 3] = (coverage / samples * 255.0).round() as u8;
        }
    }
    rgba
}

/// Colors one point of the icon, or nothing outside the rounded tile. Layers are applied in
/// bottom-up order: tile gradient, back card (with the "A" glyph), front card, "가" glyph.
fn sample_color(px: f32, py: f32, side: f32) -> Option<[f32; 3]> {
    let margin = TILE_MARGIN;
    if !inside_rounded_square(px, py, margin, margin, side - 2.0 * margin, TILE_RADIUS) {
        return None;
    }
    let t = (px + py) / (2.0 * side - 2.0);
    let mut color = mix(TILE_FROM, TILE_TO, t);

    let bx = px - CARD_BACK_OFFSET[0];
    let by = py - CARD_BACK_OFFSET[1];
    if inside_rounded_square(bx, by, 0.0, 0.0, CARD, CARD_CORNER) {
        color = mix(color, CARD_BACK, CARD_BACK_ALPHA);
        if inside_glyph_a(bx, by) {
            color = LETTER_CYAN;
        }
    }

    let fx = px - CARD_FRONT_OFFSET[0];
    let fy = py - CARD_FRONT_OFFSET[1];
    if inside_rounded_square(fx, fy, 0.0, 0.0, CARD, CARD_CORNER) {
        color = CARD_WHITE;
        if inside_glyph_ga(fx, fy) {
            color = LETTER_BLUE;
        }
    }
    Some(color)
}

/// "A": two diagonal legs plus a crossbar, in back-card coordinates.
fn inside_glyph_a(x: f32, y: f32) -> bool {
    inside_thick_line(x, y, A_LEGS.0, A_LEGS.1, STROKE_THIN)
        || inside_thick_line(x, y, A_LEGS.2, A_LEGS.3, STROKE_THIN)
        || inside_thick_line(x, y, A_BAR.0, A_BAR.1, STROKE_BAR)
}

/// "가": ㄱ (bar and diagonal descender) plus ㅏ (vertical and branch), in front-card coords.
fn inside_glyph_ga(x: f32, y: f32) -> bool {
    inside_rect(x, y, GA_BAR)
        || inside_rect(x, y, GA_VERTICAL)
        || inside_rect(x, y, GA_BRANCH)
        || inside_thick_line(x, y, GA_SLOPE.0, GA_SLOPE.1, STROKE_SLOPE)
}

/// Mirrors the icon at 1/16 scale of the 512 px design, so every number below has an
/// obvious counterpart in `scripts/make-icons.py`.
const TILE_MARGIN: f32 = 1.625;
const TILE_RADIUS: f32 = 7.5;
const CARD: f32 = 11.875;
const CARD_CORNER: f32 = 2.75;
const CARD_BACK_OFFSET: [f32; 2] = [18.875, 7.375];
const CARD_FRONT_OFFSET: [f32; 2] = [6.875, 14.0];
const CARD_BACK_ALPHA: f32 = 0.9725;

const TILE_FROM: [f32; 3] = [37.0, 99.0, 235.0];
const TILE_TO: [f32; 3] = [14.0, 165.0, 233.0];
const CARD_BACK: [f32; 3] = [236.0, 247.0, 255.0];
const CARD_WHITE: [f32; 3] = [255.0, 255.0, 255.0];
const LETTER_BLUE: [f32; 3] = [29.0, 78.0, 216.0];
const LETTER_CYAN: [f32; 3] = [12.0, 134.0, 184.0];

const STROKE_THIN: f32 = 0.9;
const STROKE_SLOPE: f32 = 1.2;

/// Rectangles are `x0, y0, x1, y1`.
const GA_BAR: [f32; 4] = [2.375, 2.4375, 6.625, 4.375];
const GA_VERTICAL: [f32; 4] = [7.625, 2.4375, 8.5625, 10.0625];
const GA_BRANCH: [f32; 4] = [7.625, 4.875, 9.75, 5.875];

/// Segments are `(from, to)` with the chosen stroke width.
const GA_SLOPE: ([f32; 2], [f32; 2]) = ([5.9, 3.5], [3.0, 7.9]);
const A_LEGS: ([f32; 2], [f32; 2], [f32; 2], [f32; 2]) =
    ([5.875, 3.35], [3.75, 8.3125], [5.875, 3.35], [8.0, 8.3125]);
const A_BAR: ([f32; 2], [f32; 2]) = ([4.375, 6.5625], [7.4375, 6.5625]);
/// The crossbar reads slightly lighter than the legs in the source font, so it is drawn on
/// its own thickness.
const STROKE_BAR: f32 = 0.8125;

fn blend_channel(value: f32) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

/// Componentwise mix between two colors; `t = 0.0` yields `from`, `t = 1.0` yields `to`.
fn mix(from: [f32; 3], to: [f32; 3], t: f32) -> [f32; 3] {
    [
        from[0] + (to[0] - from[0]) * t,
        from[1] + (to[1] - from[1]) * t,
        from[2] + (to[2] - from[2]) * t,
    ]
}

fn inside_rect(x: f32, y: f32, rect: [f32; 4]) -> bool {
    x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3]
}

/// Point inside the rectangle swept by a stroke segment of the given half width.
fn inside_thick_line(x: f32, y: f32, from: [f32; 2], to: [f32; 2], half_width: f32) -> bool {
    let dx = to[0] - from[0];
    let dy = to[1] - from[1];
    let length = (dx * dx + dy * dy).sqrt();
    if length <= f32::EPSILON {
        // A zero-length segment is a dot; approximate it with the stroke's disk.
        let ox = x - from[0];
        let oy = y - from[1];
        return ox * ox + oy * oy <= half_width * half_width;
    }
    let nx = -dy / length * half_width;
    let ny = dx / length * half_width;
    // Walk the corners in order; the swept rectangle is convex, so the point is inside
    // when every turn keeps the same orientation.
    let corners = [
        [from[0] + nx, from[1] + ny],
        [to[0] + nx, to[1] + ny],
        [to[0] - nx, to[1] - ny],
        [from[0] - nx, from[1] - ny],
    ];
    // Convex quad in cyclic order: the point is inside when it sits on the same side of
    // every edge.
    let mut orientation = 0.0f32;
    for edge in 0..4 {
        let a = corners[edge];
        let b = corners[(edge + 1) % 4];
        let cross = (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
        if cross == 0.0 {
            return false;
        }
        if orientation == 0.0 {
            orientation = cross.signum();
        } else if cross.signum() != orientation {
            return false;
        }
    }
    true
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

    fn pixel(x: u32, y: u32) -> [u8; 3] {
        let rgba = icon_rgba();
        let index = ((y * ICON_SIZE + x) * 4) as usize;
        [rgba[index], rgba[index + 1], rgba[index + 2]]
    }

    #[test]
    fn tile_is_a_blue_to_cyan_gradient() {
        // Diagonal gradient: near the top-left corner the original blue, by the bottom-right
        // corner close to the cyan end.
        let top_left = pixel(3, 3);
        let bottom_right = pixel(27, 27);
        assert!(top_left[0] >= 30 && top_left[0] <= 40, "{top_left:?}");
        assert!(
            bottom_right[0] >= 14 && bottom_right[0] <= 26,
            "{bottom_right:?}"
        );
        assert!(top_left[0] > bottom_right[0] + 5);
    }

    #[test]
    fn front_card_stays_white_away_from_the_glyphs() {
        let white = pixel(9, 24);
        assert!(white.iter().all(|&c| c >= 250), "{white:?}");
    }

    #[test]
    fn glyphs_keep_their_accent_colors() {
        // Center of the ㄱ top bar, blended slightly lighter by tile antialiasing.
        let blue = pixel(11, 17);
        assert!(blue[0] < 70 && blue[1] < 110 && blue[2] > 180, "{blue:?}");

        // Center of the A crossbar.
        let cyan = pixel(24, 13);
        assert!(cyan[0] < 90 && cyan[2] > 150, "{cyan:?}");
    }

    #[test]
    fn thick_line_covers_the_whole_swept_rectangle() {
        // Far from any drawn stroke, the helper must stay off; on the stroke it must hold.
        assert!(!inside_thick_line(
            7.0,
            6.0,
            GA_SLOPE.0,
            GA_SLOPE.1,
            STROKE_SLOPE
        ));
        let on_slope = [(5.35, 4.4), (4.0, 6.5), (3.1, 7.8)];
        for (x, y) in on_slope {
            assert!(
                inside_thick_line(x, y, GA_SLOPE.0, GA_SLOPE.1, STROKE_SLOPE),
                "({x},{y})"
            );
        }
    }
}
