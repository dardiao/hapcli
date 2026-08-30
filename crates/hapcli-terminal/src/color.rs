use alacritty_terminal::{
    term::cell::Flags,
    vte::ansi::{Color, NamedColor, Rgb},
};

use crate::{TerminalAttrs, TerminalColor};

#[derive(Clone, Copy, Debug)]
pub(crate) struct HapcliTheme {
    pub(crate) foreground: TerminalColor,
    pub(crate) ansi_background: TerminalColor,
    pub(crate) bright_foreground: TerminalColor,
    pub(crate) dim_foreground: TerminalColor,
    pub(crate) cursor: TerminalColor,
    pub(crate) ansi: [TerminalColor; 16],
    pub(crate) dim_ansi: [TerminalColor; 8],
}

pub(crate) const HAPCLI_DARK_THEME: HapcliTheme = HapcliTheme {
    // Dracula 主题：背景 / 前景 / ANSI 16 色采用官方规范值。
    foreground: TerminalColor::rgb(0xf8, 0xf8, 0xf2),
    ansi_background: TerminalColor::rgb(0x28, 0x2a, 0x36),
    bright_foreground: TerminalColor::rgb(0xff, 0xff, 0xff),
    dim_foreground: TerminalColor::rgb(0xad, 0xad, 0xa9),
    cursor: TerminalColor::rgb(0xbd, 0x93, 0xf9),
    ansi: [
        // 常规 8 色
        TerminalColor::rgb(0x21, 0x22, 0x2c), // black
        TerminalColor::rgb(0xff, 0x55, 0x55), // red
        TerminalColor::rgb(0x50, 0xfa, 0x7b), // green
        TerminalColor::rgb(0xf1, 0xfa, 0x8c), // yellow
        TerminalColor::rgb(0xbd, 0x93, 0xf9), // blue (purple)
        TerminalColor::rgb(0xff, 0x79, 0xc6), // magenta (pink)
        TerminalColor::rgb(0x8b, 0xe9, 0xfd), // cyan
        TerminalColor::rgb(0xf8, 0xf8, 0xf2), // white
        // 明亮 8 色
        TerminalColor::rgb(0x62, 0x72, 0xa4), // bright black (comment)
        TerminalColor::rgb(0xff, 0x6e, 0x6e), // bright red
        TerminalColor::rgb(0x69, 0xff, 0x94), // bright green
        TerminalColor::rgb(0xff, 0xff, 0xa5), // bright yellow
        TerminalColor::rgb(0xd6, 0xac, 0xff), // bright blue
        TerminalColor::rgb(0xff, 0x92, 0xdf), // bright magenta
        TerminalColor::rgb(0xa4, 0xff, 0xff), // bright cyan
        TerminalColor::rgb(0xff, 0xff, 0xff), // bright white
    ],
    dim_ansi: [
        TerminalColor::rgb(0x17, 0x18, 0x1f),
        TerminalColor::rgb(0xb2, 0x3b, 0x3b),
        TerminalColor::rgb(0x38, 0xaf, 0x56),
        TerminalColor::rgb(0xa8, 0xaf, 0x62),
        TerminalColor::rgb(0x84, 0x67, 0xae),
        TerminalColor::rgb(0xb2, 0x54, 0x8a),
        TerminalColor::rgb(0x61, 0xa3, 0xb1),
        TerminalColor::rgb(0xad, 0xad, 0xa9),
    ],
};

pub(crate) fn default_theme() -> HapcliTheme {
    HapcliTheme {
        foreground: TerminalColor::rgb(0xd4, 0xd4, 0xd4),
        ansi_background: TerminalColor::rgb(0x0a, 0x0a, 0x0a),
        bright_foreground: TerminalColor::rgb(0xff, 0xff, 0xff),
        dim_foreground: TerminalColor::rgb(0x8a, 0x8a, 0x8a),
        cursor: TerminalColor::rgb(0xd0, 0xd0, 0xd0),
        ansi: [
            TerminalColor::rgb(0x00, 0x00, 0x00),
            TerminalColor::rgb(0xcd, 0x00, 0x00),
            TerminalColor::rgb(0x00, 0xcd, 0x00),
            TerminalColor::rgb(0xcd, 0xcd, 0x00),
            TerminalColor::rgb(0x00, 0x00, 0xee),
            TerminalColor::rgb(0xcd, 0x00, 0xcd),
            TerminalColor::rgb(0x00, 0xcd, 0xcd),
            TerminalColor::rgb(0xe5, 0xe5, 0xe5),
            TerminalColor::rgb(0x7f, 0x7f, 0x7f),
            TerminalColor::rgb(0xff, 0x00, 0x00),
            TerminalColor::rgb(0x00, 0xff, 0x00),
            TerminalColor::rgb(0xff, 0xff, 0x00),
            TerminalColor::rgb(0x5c, 0x5c, 0xff),
            TerminalColor::rgb(0xff, 0x00, 0xff),
            TerminalColor::rgb(0x00, 0xff, 0xff),
            TerminalColor::rgb(0xff, 0xff, 0xff),
        ],
        dim_ansi: [
            TerminalColor::rgb(0x00, 0x00, 0x00),
            TerminalColor::rgb(0x8f, 0x00, 0x00),
            TerminalColor::rgb(0x00, 0x8f, 0x00),
            TerminalColor::rgb(0x8f, 0x8f, 0x00),
            TerminalColor::rgb(0x00, 0x00, 0xa6),
            TerminalColor::rgb(0x8f, 0x00, 0x8f),
            TerminalColor::rgb(0x00, 0x8f, 0x8f),
            TerminalColor::rgb(0xa0, 0xa0, 0xa0),
        ],
    }
}

pub(crate) fn high_contrast_theme() -> HapcliTheme {
    HapcliTheme {
        foreground: TerminalColor::rgb(0xff, 0xff, 0xff),
        ansi_background: TerminalColor::rgb(0x00, 0x00, 0x00),
        bright_foreground: TerminalColor::rgb(0xff, 0xff, 0xff),
        dim_foreground: TerminalColor::rgb(0xc0, 0xc0, 0xc0),
        cursor: TerminalColor::rgb(0xff, 0xff, 0xff),
        ansi: [
            TerminalColor::rgb(0x00, 0x00, 0x00),
            TerminalColor::rgb(0xff, 0x00, 0x00),
            TerminalColor::rgb(0x00, 0xff, 0x00),
            TerminalColor::rgb(0xff, 0xff, 0x00),
            TerminalColor::rgb(0x00, 0x00, 0xff),
            TerminalColor::rgb(0xff, 0x00, 0xff),
            TerminalColor::rgb(0x00, 0xff, 0xff),
            TerminalColor::rgb(0xff, 0xff, 0xff),
            TerminalColor::rgb(0x55, 0x55, 0x55),
            TerminalColor::rgb(0xff, 0x55, 0x55),
            TerminalColor::rgb(0x55, 0xff, 0x55),
            TerminalColor::rgb(0xff, 0xff, 0x55),
            TerminalColor::rgb(0x55, 0x55, 0xff),
            TerminalColor::rgb(0xff, 0x55, 0xff),
            TerminalColor::rgb(0x55, 0xff, 0xff),
            TerminalColor::rgb(0xff, 0xff, 0xff),
        ],
        dim_ansi: [
            TerminalColor::rgb(0x00, 0x00, 0x00),
            TerminalColor::rgb(0xb2, 0x00, 0x00),
            TerminalColor::rgb(0x00, 0xb2, 0x00),
            TerminalColor::rgb(0xb2, 0xb2, 0x00),
            TerminalColor::rgb(0x00, 0x00, 0xb2),
            TerminalColor::rgb(0xb2, 0x00, 0xb2),
            TerminalColor::rgb(0x00, 0xb2, 0xb2),
            TerminalColor::rgb(0xb2, 0xb2, 0xb2),
        ],
    }
}

/// 终端 ANSI 配色预设（可运行时切换，作用于会话画面）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TerminalThemePreset {
    #[default]
    Default,
    Dracula,
    HighContrast,
}

impl TerminalThemePreset {
    pub const ALL: [TerminalThemePreset; 3] = [
        TerminalThemePreset::Default,
        TerminalThemePreset::Dracula,
        TerminalThemePreset::HighContrast,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "默认",
            Self::Dracula => "Dracula",
            Self::HighContrast => "高对比",
        }
    }

    pub(crate) fn theme(self) -> HapcliTheme {
        match self {
            Self::Default => default_theme(),
            Self::Dracula => HAPCLI_DARK_THEME,
            Self::HighContrast => high_contrast_theme(),
        }
    }
}

/// 是否为某个配色预设的“默认终端背景色”。
pub fn is_terminal_default_bg(color: TerminalColor) -> bool {
    [
        HAPCLI_DARK_THEME.ansi_background,
        default_theme().ansi_background,
        high_contrast_theme().ansi_background,
    ]
    .contains(&color)
}

/// 是否为某个配色预设的“默认终端前景色”。
pub fn is_terminal_default_fg(color: TerminalColor) -> bool {
    [
        HAPCLI_DARK_THEME.foreground,
        default_theme().foreground,
        high_contrast_theme().foreground,
    ]
    .contains(&color)
}

pub(crate) const DEFAULT_MINIMUM_CONTRAST_SCORE: f32 = 45.0;
pub(crate) fn attrs_from_flags(flags: Flags) -> TerminalAttrs {
    TerminalAttrs {
        bold: flags.contains(Flags::BOLD),
        dim: flags.contains(Flags::DIM),
        italic: flags.contains(Flags::ITALIC),
        underline: flags.intersects(Flags::ALL_UNDERLINES),
        strikeout: flags.contains(Flags::STRIKEOUT),
        inverse: flags.contains(Flags::INVERSE),
    }
}

fn color_to_rgb(color: Color, theme: &HapcliTheme) -> TerminalColor {
    match color {
        Color::Named(named) => named_color_to_rgb(named, theme),
        Color::Spec(rgb) => TerminalColor::rgb(rgb.r, rgb.g, rgb.b),
        Color::Indexed(index) => indexed_color_to_rgb(index, theme),
    }
}

pub(crate) fn color_for_alacritty_request_with_override(
    index: usize,
    override_color: Option<Rgb>,
    theme: &HapcliTheme,
) -> Rgb {
    if let Some(color) = override_color {
        return color;
    }

    let color = match index {
        0..=15 => theme.ansi[index],
        16..=255 => indexed_color_to_rgb(index as u8, theme),
        256 => theme.foreground,
        257 => theme.ansi_background,
        258 => theme.cursor,
        259..=266 => theme.dim_ansi[(index - 259).min(7)],
        267 => theme.bright_foreground,
        268 => theme.ansi[0],
        _ => TerminalColor::rgb(0, 0, 0),
    };

    Rgb {
        r: color.r,
        g: color.g,
        b: color.b,
    }
}

fn named_color_to_rgb(color: NamedColor, theme: &HapcliTheme) -> TerminalColor {
    match color {
        NamedColor::Black => theme.ansi[0],
        NamedColor::Red => theme.ansi[1],
        NamedColor::Green => theme.ansi[2],
        NamedColor::Yellow => theme.ansi[3],
        NamedColor::Blue => theme.ansi[4],
        NamedColor::Magenta => theme.ansi[5],
        NamedColor::Cyan => theme.ansi[6],
        NamedColor::White => theme.ansi[7],
        NamedColor::BrightBlack => theme.ansi[8],
        NamedColor::BrightRed => theme.ansi[9],
        NamedColor::BrightGreen => theme.ansi[10],
        NamedColor::BrightYellow => theme.ansi[11],
        NamedColor::BrightBlue => theme.ansi[12],
        NamedColor::BrightMagenta => theme.ansi[13],
        NamedColor::BrightCyan => theme.ansi[14],
        NamedColor::BrightWhite => theme.ansi[15],
        NamedColor::Foreground => theme.foreground,
        NamedColor::Background => theme.ansi_background,
        NamedColor::Cursor => theme.cursor,
        NamedColor::DimBlack => theme.dim_ansi[0],
        NamedColor::DimRed => theme.dim_ansi[1],
        NamedColor::DimGreen => theme.dim_ansi[2],
        NamedColor::DimYellow => theme.dim_ansi[3],
        NamedColor::DimBlue => theme.dim_ansi[4],
        NamedColor::DimMagenta => theme.dim_ansi[5],
        NamedColor::DimCyan => theme.dim_ansi[6],
        NamedColor::DimWhite => theme.dim_ansi[7],
        NamedColor::BrightForeground => theme.bright_foreground,
        NamedColor::DimForeground => theme.dim_foreground,
    }
}

pub(crate) fn indexed_color_to_rgb(index: u8, theme: &HapcliTheme) -> TerminalColor {
    match index {
        0..=15 => theme.ansi[index as usize],
        16..=231 => {
            let index = index - 16;
            let r = index / 36;
            let g = (index % 36) / 6;
            let b = index % 6;
            TerminalColor::rgb(
                if r == 0 { 0 } else { r * 40 + 55 },
                if g == 0 { 0 } else { g * 40 + 55 },
                if b == 0 { 0 } else { b * 40 + 55 },
            )
        }
        232..=255 => {
            let value = (index - 232) * 10 + 8;
            TerminalColor::rgb(value, value, value)
        }
    }
}

fn dim_color(color: TerminalColor) -> TerminalColor {
    TerminalColor::rgb(
        (color.r as f32 * 0.7) as u8,
        (color.g as f32 * 0.7) as u8,
        (color.b as f32 * 0.7) as u8,
    )
}

fn is_app_chosen_exact_color(color: &Color) -> bool {
    matches!(color, Color::Spec(_) | Color::Indexed(16..=255))
}

fn is_terminal_decoration_glyph(ch: char) -> bool {
    const DECORATIVE_RANGES: &[(u32, u32)] = &[
        (0x2500, 0x257f),
        (0x2580, 0x259f),
        (0x25a0, 0x25ff),
        (0xe0b0, 0xe0b7),
        (0xe0b8, 0xe0bf),
        (0xe0c0, 0xe0ca),
        (0xe0cc, 0xe0d1),
        (0xe0d2, 0xe0d7),
    ];

    let codepoint = ch as u32;
    DECORATIVE_RANGES
        .iter()
        .any(|&(start, end)| (start..=end).contains(&codepoint))
}

pub(crate) fn style_colors_for_cell(
    fg: Color,
    bg: Color,
    ch: char,
    attrs: TerminalAttrs,
    theme: &HapcliTheme,
) -> (TerminalColor, TerminalColor) {
    let mut fg_color = fg;
    let mut bg_color = bg;
    if attrs.inverse {
        std::mem::swap(&mut fg_color, &mut bg_color);
    }

    let mut fg = color_to_rgb(fg_color, theme);
    let bg = color_to_rgb(bg_color, theme);

    // 16 色命名色直接按配色预设渲染：不做 APCA 对比度改写，
    // 否则红/蓝等低亮颜色会被统一提亮、在三个预设下趋同，无法体现预设差异。
    let fg_is_named = matches!(fg_color, Color::Named(_));
    if !fg_is_named && !is_app_chosen_exact_color(&fg_color) && !is_terminal_decoration_glyph(ch) {
        fg = ensure_minimum_contrast(fg, bg, DEFAULT_MINIMUM_CONTRAST_SCORE);
    }

    if attrs.dim {
        fg = dim_color(fg);
    }

    (fg, bg)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct HslaColor {
    h: f32,
    s: f32,
    l: f32,
    a: f32,
}

impl HslaColor {
    fn to_rgb(self) -> (f32, f32, f32) {
        if self.s == 0.0 {
            return (self.l, self.l, self.l);
        }

        let q = if self.l < 0.5 {
            self.l * (1.0 + self.s)
        } else {
            self.l + self.s - self.l * self.s
        };
        let p = 2.0 * self.l - q;

        (
            hue_to_rgb(p, q, self.h + 1.0 / 3.0),
            hue_to_rgb(p, q, self.h),
            hue_to_rgb(p, q, self.h - 1.0 / 3.0),
        )
    }

    fn to_terminal_color(self) -> TerminalColor {
        let (r, g, b) = self.to_rgb();
        TerminalColor::rgb(
            (r.clamp(0.0, 1.0) * 255.0).round() as u8,
            (g.clamp(0.0, 1.0) * 255.0).round() as u8,
            (b.clamp(0.0, 1.0) * 255.0).round() as u8,
        )
    }
}

impl From<TerminalColor> for HslaColor {
    fn from(color: TerminalColor) -> Self {
        let r = color.r as f32 / 255.0;
        let g = color.g as f32 / 255.0;
        let b = color.b as f32 / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;

        if max == min {
            return Self {
                h: 0.0,
                s: 0.0,
                l,
                a: 1.0,
            };
        }

        let d = max - min;
        let s = if l > 0.5 {
            d / (2.0 - max - min)
        } else {
            d / (max + min)
        };
        let h = if max == r {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        } / 6.0;

        Self { h, s, l, a: 1.0 }
    }
}

fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 1.0 / 2.0 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

pub(crate) fn perceptual_contrast_score(
    text_color: TerminalColor,
    background_color: TerminalColor,
) -> f32 {
    let text_luminance = apca_luminance(text_color);
    let background_luminance = apca_luminance(background_color);
    if (background_luminance - text_luminance).abs() < APCA_MIN_LUMINANCE_DELTA {
        return 0.0;
    }

    let raw_contrast = if background_luminance > text_luminance {
        (background_luminance.powf(APCA_DARK_TEXT_BACKGROUND_EXPONENT)
            - text_luminance.powf(APCA_DARK_TEXT_FOREGROUND_EXPONENT))
            * APCA_SCALE
    } else {
        (background_luminance.powf(APCA_LIGHT_TEXT_BACKGROUND_EXPONENT)
            - text_luminance.powf(APCA_LIGHT_TEXT_FOREGROUND_EXPONENT))
            * APCA_SCALE
    };

    let low_contrast = raw_contrast.abs() < APCA_LOW_CONTRAST_CLIP;
    if low_contrast {
        0.0
    } else if raw_contrast > 0.0 {
        (raw_contrast - APCA_LOW_CONTRAST_OFFSET) * 100.0
    } else {
        (raw_contrast + APCA_LOW_CONTRAST_OFFSET) * 100.0
    }
}

// APCA-W3 algorithm constants from the public specification:
// https://github.com/Myndex/apca-w3 (W3 licensed, not derived from any third-party product)
const APCA_MAIN_TRANSFER_EXPONENT: f32 = 2.4;
const APCA_DARK_TEXT_BACKGROUND_EXPONENT: f32 = 0.56;
const APCA_DARK_TEXT_FOREGROUND_EXPONENT: f32 = 0.57;
const APCA_LIGHT_TEXT_BACKGROUND_EXPONENT: f32 = 0.65;
const APCA_LIGHT_TEXT_FOREGROUND_EXPONENT: f32 = 0.62;
const APCA_RED_COEFFICIENT: f32 = 0.2126729;
const APCA_GREEN_COEFFICIENT: f32 = 0.7151522;
const APCA_BLUE_COEFFICIENT: f32 = 0.0721750;
const APCA_BLACK_THRESHOLD: f32 = 0.022;
const APCA_BLACK_CLAMP_EXPONENT: f32 = 1.414;
const APCA_LOW_CONTRAST_CLIP: f32 = 0.1;
const APCA_MIN_LUMINANCE_DELTA: f32 = 0.0005;
const APCA_LOW_CONTRAST_OFFSET: f32 = 0.027;
const APCA_SCALE: f32 = 1.14;

fn apca_luminance(color: TerminalColor) -> f32 {
    let red = apca_channel(color.r);
    let green = apca_channel(color.g);
    let blue = apca_channel(color.b);
    let luminance =
        red * APCA_RED_COEFFICIENT + green * APCA_GREEN_COEFFICIENT + blue * APCA_BLUE_COEFFICIENT;

    if luminance >= APCA_BLACK_THRESHOLD {
        luminance
    } else {
        luminance + (APCA_BLACK_THRESHOLD - luminance).powf(APCA_BLACK_CLAMP_EXPONENT)
    }
}

fn apca_channel(channel: u8) -> f32 {
    (channel as f32 / 255.0).powf(APCA_MAIN_TRANSFER_EXPONENT)
}

fn relative_luminance(color: TerminalColor) -> f32 {
    let r = srgb_channel_to_linear(color.r);
    let g = srgb_channel_to_linear(color.g);
    let b = srgb_channel_to_linear(color.b);

    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn srgb_channel_to_linear(channel: u8) -> f32 {
    let value = channel as f32 / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn ensure_minimum_contrast(
    foreground: TerminalColor,
    background: TerminalColor,
    minimum_perceptual_contrast_score: f32,
) -> TerminalColor {
    if minimum_perceptual_contrast_score <= 0.0
        || perceptual_contrast_score(foreground, background).abs()
            >= minimum_perceptual_contrast_score
    {
        return foreground;
    }

    let foreground_hsla = HslaColor::from(foreground);
    for saturation in saturation_search_order(foreground_hsla.s) {
        if let Some(adjusted) = find_nearest_contrasting_lightness(
            foreground_hsla,
            saturation,
            background,
            minimum_perceptual_contrast_score,
        ) {
            return adjusted.to_terminal_color();
        }
    }

    let black = HslaColor {
        h: 0.0,
        s: 0.0,
        l: 0.0,
        a: foreground_hsla.a,
    }
    .to_terminal_color();
    let white = HslaColor {
        h: 0.0,
        s: 0.0,
        l: 1.0,
        a: foreground_hsla.a,
    }
    .to_terminal_color();

    if perceptual_contrast_score(white, background).abs()
        > perceptual_contrast_score(black, background).abs()
    {
        white
    } else {
        black
    }
}

fn saturation_search_order(original_saturation: f32) -> [f32; 6] {
    [
        original_saturation,
        original_saturation * 0.85,
        original_saturation * 0.65,
        original_saturation * 0.45,
        original_saturation * 0.25,
        0.0,
    ]
}

fn find_nearest_contrasting_lightness(
    foreground: HslaColor,
    saturation: f32,
    background: TerminalColor,
    minimum_perceptual_contrast_score: f32,
) -> Option<HslaColor> {
    let target_lightness = if relative_luminance(background) > 0.45 {
        0.0
    } else {
        1.0
    };

    let terminal_candidate = HslaColor {
        h: foreground.h,
        s: saturation,
        l: target_lightness,
        a: foreground.a,
    };
    if perceptual_contrast_score(terminal_candidate.to_terminal_color(), background).abs()
        < minimum_perceptual_contrast_score
    {
        return None;
    }

    let mut low = 0.0;
    let mut high = 1.0;
    let mut best = terminal_candidate;

    for _ in 0..24 {
        let amount = (low + high) / 2.0;
        let lightness = foreground.l + (target_lightness - foreground.l) * amount;
        let candidate = HslaColor {
            h: foreground.h,
            s: saturation,
            l: lightness,
            a: foreground.a,
        };

        if perceptual_contrast_score(candidate.to_terminal_color(), background).abs()
            >= minimum_perceptual_contrast_score
        {
            best = candidate;
            high = amount;
        } else {
            low = amount;
        }
    }

    Some(best)
}
