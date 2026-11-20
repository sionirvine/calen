//! Small inline SVG icon set (24×24, stroked, tinted at draw time).

use iced::widget::svg::{self, Svg};
use iced::{Color, Length, Theme};

macro_rules! icon {
    ($name:ident, $body:literal) => {
        pub fn $name() -> svg::Handle {
            static H: std::sync::OnceLock<svg::Handle> = std::sync::OnceLock::new();
            H.get_or_init(|| {
                svg::Handle::from_memory(
                    concat!(
                        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
                        $body,
                        "</svg>"
                    )
                    .as_bytes(),
                )
            })
            .clone()
        }
    };
}

// Window caption glyphs (thin, Windows 11 style).
icon!(win_min, r#"<path d="M5 12h14" stroke-width="1.4"/>"#);
icon!(win_max, r#"<rect x="5" y="5" width="14" height="14" rx="1.5" stroke-width="1.4"/>"#);
icon!(
    win_restore,
    r#"<rect x="5" y="8" width="11" height="11" rx="1.5" stroke-width="1.4"/><path d="M8.5 8V6.5A1.5 1.5 0 0 1 10 5h7.5A1.5 1.5 0 0 1 19 6.5V14a1.5 1.5 0 0 1-1.5 1.5H16" stroke-width="1.4"/>"#
);
icon!(win_close, r#"<path d="M6 6l12 12M18 6L6 18" stroke-width="1.4"/>"#);
icon!(chevron_left, r#"<path d="M15 6l-6 6 6 6"/>"#);
icon!(chevron_right, r#"<path d="M9 6l6 6-6 6"/>"#);
icon!(chevron_down, r#"<path d="M6 9l6 6 6-6"/>"#);
icon!(search, r#"<circle cx="11" cy="11" r="7"/><path d="M20 20l-3.5-3.5"/>"#);
icon!(plus, r#"<path d="M12 5v14M5 12h14"/>"#);
icon!(bell, r#"<path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9"/><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0"/>"#);
icon!(
    bell_off,
    r#"<path d="M8.7 3A6 6 0 0 1 18 8a21.3 21.3 0 0 0 .6 5"/><path d="M17 17H3s3-2 3-9a4.67 4.67 0 0 1 .3-1.7"/><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0"/><path d="M2 2l20 20"/>"#
);
icon!(video, r#"<path d="M15 10l5-3v10l-5-3"/><rect x="2" y="6" width="13" height="12" rx="2"/>"#);
icon!(map_pin, r#"<path d="M20 10c0 6-8 12-8 12s-8-6-8-12a8 8 0 0 1 16 0z"/><circle cx="12" cy="10" r="3"/>"#);
icon!(cloud, r#"<path d="M17.5 19H9a7 7 0 1 1 6.7-9h1.8a4.5 4.5 0 1 1 0 9z"/>"#);
icon!(
    cloud_off,
    r#"<path d="M2 2l20 20"/><path d="M5.8 5.8A7 7 0 0 0 9 19h8.5a4.5 4.5 0 0 0 1.3-.2"/><path d="M21.5 15.5A4.5 4.5 0 0 0 17.5 10h-1.8A7 7 0 0 0 10 5.1"/>"#
);
icon!(check, r#"<path d="M20 6L9 17l-5-5"/>"#);
icon!(alert, r#"<path d="M12 9v4M12 17h.01"/><path d="M10.3 3.9L1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z"/>"#);
icon!(
    settings,
    r#"<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>"#
);
icon!(
    image,
    r#"<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="2"/><path d="M21 15l-3.1-3.1a2 2 0 0 0-2.8 0L6 21"/>"#
);
icon!(close, r#"<path d="M18 6L6 18M6 6l12 12"/>"#);
icon!(pencil, r#"<path d="M17 3a2.83 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5z"/>"#);
icon!(trash, r#"<path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6"/>"#);
icon!(calendar, r#"<rect x="3" y="4" width="18" height="18" rx="2"/><path d="M16 2v4M8 2v4M3 10h18"/>"#);
icon!(download, r#"<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><path d="M7 10l5 5 5-5M12 15V3"/>"#);
icon!(sidebar, r#"<rect x="3" y="3" width="18" height="18" rx="2"/><path d="M9 3v18"/>"#);

/// An icon widget tinted with `color`.
pub fn icon<'a>(handle: svg::Handle, size: f32, color: Color) -> Svg<'a, Theme> {
    Svg::new(handle).width(Length::Fixed(size)).height(Length::Fixed(size)).style(move |_, _| svg::Style { color: Some(color) })
}
