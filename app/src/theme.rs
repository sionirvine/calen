//! Colours and widget styles. Two palettes (dark/light); dark sidebar in both,
//! like the inspiration screenshots.

use iced::widget::{button, container, scrollable, text_input};
use iced::{Background, Border, Color, Shadow, Theme, Vector, color};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color,
    pub surface: Color,
    pub surface_alt: Color,
    pub grid: Color,
    pub grid_strong: Color,
    pub text: Color,
    pub muted: Color,
    pub faint: Color,
    pub accent: Color,
    pub accent_text: Color,
    pub today: Color,
    pub now_line: Color,
    pub danger: Color,
    pub success: Color,
    pub sidebar: Color,
    pub sidebar_text: Color,
    pub sidebar_muted: Color,
    pub sidebar_hover: Color,
    pub weekend: Color,
    pub selection: Color,
}

pub const DARK: Palette = Palette {
    dark: true,
    bg: color!(0x161618),
    surface: color!(0x1e1e21),
    surface_alt: color!(0x26262a),
    grid: color!(0x2a2a2f),
    grid_strong: color!(0x3a3a40),
    text: color!(0xececf0),
    muted: color!(0x9a9aa3),
    faint: color!(0x5d5d66),
    accent: color!(0xe5484d),
    accent_text: color!(0xffffff),
    today: color!(0x3b82f6),
    now_line: color!(0xe5484d),
    danger: color!(0xe5484d),
    success: color!(0x30a46c),
    sidebar: color!(0x1b1b1d),
    sidebar_text: color!(0xf2f2f5),
    sidebar_muted: color!(0x8b8b94),
    sidebar_hover: color!(0x2b2b30),
    weekend: color!(0x1a1a1d),
    selection: color!(0x24324a),
};

pub const LIGHT: Palette = Palette {
    dark: false,
    bg: color!(0xffffff),
    surface: color!(0xf7f7f8),
    surface_alt: color!(0xefeff1),
    grid: color!(0xe8e8eb),
    grid_strong: color!(0xd4d4d9),
    text: color!(0x1c1c1f),
    muted: color!(0x6b6b74),
    faint: color!(0xa8a8b0),
    accent: color!(0xd93a3f),
    accent_text: color!(0xffffff),
    today: color!(0x2f74e8),
    now_line: color!(0xd93a3f),
    danger: color!(0xd93a3f),
    success: color!(0x2b9a66),
    sidebar: color!(0x1f1f22),
    sidebar_text: color!(0xf2f2f5),
    sidebar_muted: color!(0x8b8b94),
    sidebar_hover: color!(0x2e2e33),
    weekend: color!(0xfafafb),
    selection: color!(0xe3edfd),
};

pub fn palette(theme: &Theme) -> Palette {
    if theme.extended_palette().is_dark { DARK } else { LIGHT }
}

pub fn iced_theme(p: &Palette) -> Theme {
    Theme::custom(
        if p.dark { "Calen Dark" } else { "Calen Light" },
        iced::theme::Palette {
            background: p.bg,
            text: p.text,
            primary: p.today,
            success: p.success,
            warning: color!(0xf5a524),
            danger: p.danger,
        },
    )
}

pub fn parse_hex(s: &str) -> Option<Color> {
    let h = s.trim().trim_start_matches('#');
    let v = u32::from_str_radix(h.get(..6)?, 16).ok()?;
    Some(Color::from_rgb8((v >> 16) as u8, (v >> 8 & 0xff) as u8, (v & 0xff) as u8))
}

pub fn with_alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

/// Mix `c` toward `toward` by `t` (0..1).
pub fn mix(c: Color, toward: Color, t: f32) -> Color {
    Color { r: c.r + (toward.r - c.r) * t, g: c.g + (toward.g - c.g) * t, b: c.b + (toward.b - c.b) * t, a: c.a + (toward.a - c.a) * t }
}

/// Event chip colours: soft fill + strong text, readable in both themes.
pub fn event_fill(p: &Palette, c: Color) -> Color {
    if p.dark { mix(c, p.bg, 0.62) } else { mix(c, Color::WHITE, 0.78) }
}

pub fn event_text(p: &Palette, c: Color) -> Color {
    if p.dark { mix(c, Color::WHITE, 0.55) } else { mix(c, Color::BLACK, 0.45) }
}

// ---------- widget styles ----------

pub fn sidebar(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style { background: Some(p.sidebar.into()), text_color: Some(p.sidebar_text), ..Default::default() }
}

pub fn main_area(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style { background: Some(p.bg.into()), text_color: Some(p.text), ..Default::default() }
}

/// Set at start when the CPU renderer (tiny-skia) is used.
static CPU_RENDERING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_cpu_rendering(on: bool) {
    CPU_RENDERING.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn popover(theme: &Theme) -> container::Style {
    let p = palette(theme);
    // No drop shadow on the CPU renderer: it paints shadows outside the area it is
    // repainting, so when only part of the window was redrawn (the today column
    // each minute, say) the shadow piled up over the dialog and turned it black.
    let shadow = if CPU_RENDERING.load(std::sync::atomic::Ordering::Relaxed) {
        Shadow::default()
    } else {
        Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.35), offset: Vector::new(0.0, 8.0), blur_radius: 28.0 }
    };
    container::Style {
        background: Some(p.surface.into()),
        text_color: Some(p.text),
        border: Border { color: p.grid_strong, width: 1.0, radius: 12.0.into() },
        shadow,
        snap: true,
    }
}

pub fn backdrop(_theme: &Theme) -> container::Style {
    container::Style { background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.45).into()), ..Default::default() }
}

pub fn pill(bg: Color, fg: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(bg.into()),
        text_color: Some(fg),
        border: Border { radius: 999.0.into(), ..Default::default() },
        ..Default::default()
    }
}

pub fn dot(c: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(c.into()),
        border: Border { radius: 999.0.into(), ..Default::default() },
        ..Default::default()
    }
}

pub fn divider(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style { background: Some(p.grid.into()), ..Default::default() }
}

/// Flat icon/text button for the light/dark main area.
pub fn ghost(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let bg = match status {
        button::Status::Hovered => Some(Background::Color(p.surface_alt)),
        button::Status::Pressed => Some(Background::Color(p.grid_strong)),
        _ => None,
    };
    button::Style { background: bg, text_color: p.text, border: Border { radius: 6.0.into(), ..Default::default() }, ..Default::default() }
}

/// Window caption buttons (minimize / maximize / close) in the toolbar.
pub fn caption_button(close: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        let red = Color::from_rgb8(0xC4, 0x2B, 0x1C);
        let bg = match (status, close) {
            (button::Status::Hovered, true) => Some(Background::Color(red)),
            (button::Status::Pressed, true) => Some(Background::Color(with_alpha(red, 0.8))),
            (button::Status::Hovered, false) => Some(Background::Color(p.surface_alt)),
            (button::Status::Pressed, false) => Some(Background::Color(p.grid_strong)),
            _ => None,
        };
        button::Style { background: bg, text_color: p.text, ..Default::default() }
    }
}

/// Flat button on the dark sidebar.
pub fn side_ghost(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let bg = match status {
        button::Status::Hovered | button::Status::Pressed => Some(Background::Color(p.sidebar_hover)),
        _ => None,
    };
    button::Style {
        background: bg,
        text_color: p.sidebar_text,
        border: Border { radius: 6.0.into(), ..Default::default() },
        ..Default::default()
    }
}

pub fn segment(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        if active {
            button::Style {
                background: Some(p.accent.into()),
                text_color: p.accent_text,
                border: Border { radius: 6.0.into(), ..Default::default() },
                ..Default::default()
            }
        } else {
            ghost(theme, status)
        }
    }
}

pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let bg = match status {
        button::Status::Hovered => mix(p.today, Color::WHITE, 0.12),
        button::Status::Pressed => mix(p.today, Color::BLACK, 0.12),
        button::Status::Disabled => with_alpha(p.today, 0.4),
        _ => p.today,
    };
    button::Style {
        background: Some(bg.into()),
        text_color: Color::WHITE,
        border: Border { radius: 8.0.into(), ..Default::default() },
        ..Default::default()
    }
}

pub fn join(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let bg = match status {
        button::Status::Hovered => mix(p.today, Color::WHITE, 0.15),
        _ => p.today,
    };
    button::Style {
        background: Some(bg.into()),
        text_color: Color::WHITE,
        border: Border { radius: 999.0.into(), ..Default::default() },
        ..Default::default()
    }
}

/// Quieter Join for meetings that aren't about to start.
pub fn join_outline(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    button::Style {
        background: Some(with_alpha(p.today, if matches!(status, button::Status::Hovered) { 0.3 } else { 0.16 }).into()),
        text_color: mix(p.today, Color::WHITE, 0.35),
        border: Border { radius: 999.0.into(), ..Default::default() },
        ..Default::default()
    }
}

pub fn danger_ghost(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let mut s = ghost(theme, status);
    s.text_color = p.danger;
    s
}

pub fn link(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    button::Style {
        background: None,
        text_color: if matches!(status, button::Status::Hovered) { mix(p.today, Color::WHITE, 0.2) } else { p.today },
        ..Default::default()
    }
}

/// A day cell of the sidebar mini-month.
pub fn mini_day(selected: bool, today: bool, in_month: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        let bg = if today {
            Some(p.today.into())
        } else if matches!(status, button::Status::Hovered) {
            Some(with_alpha(p.sidebar_hover, 0.7).into())
        } else {
            None
        };
        let text_color = if today {
            Color::WHITE
        } else if in_month {
            p.sidebar_text
        } else {
            p.sidebar_muted
        };
        // The picked day gets a red ring; today keeps its blue fill.
        let border = Border { radius: 999.0.into(), width: if selected { 1.5 } else { 0.0 }, color: p.accent };
        button::Style { background: bg, text_color, border, ..Default::default() }
    }
}

pub fn input(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let p = palette(theme);
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: p.bg.into(),
        border: Border { color: if focused { p.today } else { p.grid_strong }, width: 1.0, radius: 8.0.into() },
        icon: p.muted,
        placeholder: p.faint,
        value: p.text,
        selection: with_alpha(p.today, 0.35),
    }
}

pub fn bare_input(theme: &Theme, _status: text_input::Status) -> text_input::Style {
    let p = palette(theme);
    text_input::Style {
        background: Color::TRANSPARENT.into(),
        border: Border::default(),
        icon: p.muted,
        placeholder: p.faint,
        value: p.text,
        selection: with_alpha(p.today, 0.35),
    }
}

pub fn side_scroll(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let p = palette(theme);
    let mut s = scrollable::default(theme, status);
    s.vertical_rail.background = None;
    s.vertical_rail.scroller.background = with_alpha(p.sidebar_muted, 0.35).into();
    s.vertical_rail.border = Border::default();
    s
}

pub fn main_scroll(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let p = palette(theme);
    let mut s = scrollable::default(theme, status);
    s.vertical_rail.background = None;
    s.vertical_rail.scroller.background = with_alpha(p.muted, 0.35).into();
    s.vertical_rail.border = Border::default();
    s
}
