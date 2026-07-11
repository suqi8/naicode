use crate::color::is_light;
use crate::terminal_palette::StdoutColorLevel;
use crate::terminal_palette::best_color_for_level;
use crate::terminal_palette::stdout_color_level;
use ratatui::style::Color;
use std::sync::OnceLock;

const DEFAULT_ACCENT: (u8, u8, u8) = (0x27, 0x9c, 0xff);
const DEFAULT_ACCENT_BRIGHT: (u8, u8, u8) = (0x86, 0xca, 0xff);
const DEFAULT_SELECTION_BACKGROUND: (u8, u8, u8) = (0x0b, 0x20, 0x32);
const DEFAULT_DARK_BACKGROUND: (u8, u8, u8) = (0x07, 0x12, 0x1d);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProductPalette {
    pub(crate) accent: Color,
    pub(crate) accent_bright: Color,
    pub(crate) selection_background: Color,
    pub(crate) selection_foreground: Color,
    pub(crate) border_focused: Color,
    pub(crate) border_muted: Color,
    pub(crate) status_success: Color,
    pub(crate) status_warning: Color,
    pub(crate) status_error: Color,
    pub(crate) dark_background: Color,
}

static PRODUCT_PALETTE: OnceLock<ProductPalette> = OnceLock::new();

pub(crate) fn configure(accent: Option<&str>) -> Option<String> {
    let (accent_rgb, warning) = match accent {
        Some(raw) => match parse_hex_color(raw) {
            Some(rgb) => (rgb, None),
            None => (
                DEFAULT_ACCENT,
                Some(format!(
                    "[tui].product_accent 必须是 #RRGGBB；已回退到默认深空蔚蓝 #279CFF（收到 {raw:?}）"
                )),
            ),
        },
        None => (DEFAULT_ACCENT, None),
    };
    let _ = PRODUCT_PALETTE.set(build_palette(accent_rgb, stdout_color_level()));
    warning
}

pub(crate) fn current() -> ProductPalette {
    PRODUCT_PALETTE
        .get()
        .copied()
        .unwrap_or_else(|| build_palette(DEFAULT_ACCENT, stdout_color_level()))
}

fn parse_hex_color(value: &str) -> Option<(u8, u8, u8)> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some((
        u8::from_str_radix(&hex[0..2], 16).ok()?,
        u8::from_str_radix(&hex[2..4], 16).ok()?,
        u8::from_str_radix(&hex[4..6], 16).ok()?,
    ))
}

fn build_palette(accent: (u8, u8, u8), level: StdoutColorLevel) -> ProductPalette {
    let custom = accent != DEFAULT_ACCENT;
    let accent_bright = if custom {
        let (h, s, l) = rgb_to_hsl(accent);
        hsl_to_rgb(h, s, (l + 0.18).min(1.0))
    } else {
        DEFAULT_ACCENT_BRIGHT
    };
    let selection_background = if custom {
        let (h, s, _) = rgb_to_hsl(accent);
        hsl_to_rgb(h, s.max(0.45), 0.14)
    } else {
        DEFAULT_SELECTION_BACKGROUND
    };
    let selection_foreground = contrast_foreground(selection_background, 4.5);

    ProductPalette {
        accent: color_for_level(accent, level, Color::Blue),
        accent_bright: color_for_level(accent_bright, level, Color::Cyan),
        selection_background: color_for_level(selection_background, level, Color::DarkGray),
        selection_foreground: color_for_level(selection_foreground, level, Color::White),
        border_focused: color_for_level(accent, level, Color::Blue),
        border_muted: Color::DarkGray,
        status_success: color_for_level((0x3f, 0xc5, 0x6b), level, Color::Green),
        status_warning: color_for_level((0xff, 0xb0, 0x20), level, Color::Yellow),
        status_error: color_for_level((0xff, 0x5f, 0x68), level, Color::Red),
        dark_background: color_for_level(DEFAULT_DARK_BACKGROUND, level, Color::Black),
    }
}

fn color_for_level(rgb: (u8, u8, u8), level: StdoutColorLevel, ansi: Color) -> Color {
    match level {
        StdoutColorLevel::TrueColor | StdoutColorLevel::Ansi256 => best_color_for_level(rgb, level),
        StdoutColorLevel::Ansi16 | StdoutColorLevel::Unknown => ansi,
    }
}

fn contrast_foreground(background: (u8, u8, u8), minimum_ratio: f32) -> (u8, u8, u8) {
    let white = contrast_ratio((255, 255, 255), background);
    let black = contrast_ratio((0, 0, 0), background);
    let preferred = if white >= black {
        (255, 255, 255)
    } else {
        (0, 0, 0)
    };
    if white.max(black) >= minimum_ratio {
        preferred
    } else if is_light(background) {
        (0, 0, 0)
    } else {
        (255, 255, 255)
    }
}

fn contrast_ratio(a: (u8, u8, u8), b: (u8, u8, u8)) -> f32 {
    let lighter = relative_luminance(a).max(relative_luminance(b));
    let darker = relative_luminance(a).min(relative_luminance(b));
    (lighter + 0.05) / (darker + 0.05)
}

fn relative_luminance((r, g, b): (u8, u8, u8)) -> f32 {
    fn channel(value: u8) -> f32 {
        let value = f32::from(value) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

fn rgb_to_hsl((r, g, b): (u8, u8, u8)) -> (f32, f32, f32) {
    let r = f32::from(r) / 255.0;
    let g = f32::from(g) / 255.0;
    let b = f32::from(b) / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let delta = max - min;
    if delta == 0.0 {
        return (0.0, 0.0, l);
    }
    let s = delta / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    (h, s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match h {
        h if h < 60.0 => (c, x, 0.0),
        h if h < 120.0 => (x, c, 0.0),
        h if h < 180.0 => (0.0, c, x),
        h if h < 240.0 => (0.0, x, c),
        h if h < 300.0 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_rrggbb() {
        assert_eq!(parse_hex_color("#279CFF"), Some(DEFAULT_ACCENT));
        assert_eq!(parse_hex_color("279CFF"), None);
        assert_eq!(parse_hex_color("#abc"), None);
        assert_eq!(parse_hex_color("#zzzzzz"), None);
    }

    #[test]
    fn default_palette_has_required_ansi_fallbacks() {
        let palette = build_palette(DEFAULT_ACCENT, StdoutColorLevel::Ansi16);
        assert_eq!(palette.accent, Color::Blue);
        assert_eq!(palette.accent_bright, Color::Cyan);
        assert_eq!(palette.selection_background, Color::DarkGray);
    }

    #[test]
    fn custom_selection_foreground_meets_text_contrast() {
        let (_, _, selection) = {
            let (h, s, _) = rgb_to_hsl((0xee, 0x33, 0x99));
            (h, s, hsl_to_rgb(h, s.max(0.45), 0.14))
        };
        let foreground = contrast_foreground(selection, 4.5);
        assert!(contrast_ratio(foreground, selection) >= 4.5);
    }
}
