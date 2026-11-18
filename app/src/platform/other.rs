//! Fallback for platforms not implemented yet (macOS is planned).

use super::Notice;

pub fn early_init() {}
pub fn init_tray(_dnd: bool) -> bool {
    false
}
pub fn update_tray(_dnd: bool, _problem: bool) {}
pub fn main_window_platform() -> iced::window::settings::PlatformSpecific {
    Default::default()
}
pub fn flyout_platform() -> iced::window::settings::PlatformSpecific {
    Default::default()
}
pub fn flyout_position(x: f64, y: f64, _rect: Option<(f64, f64, f64, f64)>, _size: iced::Size) -> iced::Point {
    iced::Point::new(x as f32, y as f32)
}
pub fn system_quiet_reason() -> Option<&'static str> {
    None
}
pub fn notify(_n: &Notice) -> Result<(), String> {
    Err("notifications not implemented on this platform".into())
}
