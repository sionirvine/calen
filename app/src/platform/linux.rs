//! Linux (GNOME + KDE): StatusNotifierItem tray via ksni, freedesktop
//! notifications, DND via KDE `Inhibited` / GNOME `show-banners`, Feral GameMode.

use std::sync::{Mutex, OnceLock};

use ksni::blocking::TrayMethods;
use ksni::menu::{CheckmarkItem, StandardItem};

use super::{MenuAction, Notice, NotificationAction, PlatformEvent, emit, icon};

struct CalenTray {
    dnd: bool,
    problem: bool,
    day: u32,
}

impl ksni::Tray for CalenTray {
    fn id(&self) -> String {
        "calen".into()
    }

    fn title(&self) -> String {
        "Calen".into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip { title: super::tray_tooltip(self.dnd), ..Default::default() }
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        [22u32, 32, 48]
            .into_iter()
            .map(|size| {
                let rgba = icon::render(size, Some(self.day), self.dnd, self.problem);
                // RGBA → ARGB (network byte order)
                let mut data = Vec::with_capacity(rgba.len());
                for px in rgba.chunks(4) {
                    data.extend_from_slice(&[px[3], px[0], px[1], px[2]]);
                }
                ksni::Icon { width: size as i32, height: size as i32, data }
            })
            .collect()
    }

    /// Left click.
    fn activate(&mut self, x: i32, y: i32) {
        emit(PlatformEvent::TrayClick { x: x as f64, y: y as f64, rect: None });
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Open Calen".into(),
                activate: Box::new(|_| emit(PlatformEvent::Menu(MenuAction::Open))),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "New Event…".into(),
                activate: Box::new(|_| emit(PlatformEvent::Menu(MenuAction::NewEvent))),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            CheckmarkItem {
                label: "Do Not Disturb".into(),
                checked: self.dnd,
                activate: Box::new(|_| emit(PlatformEvent::Menu(MenuAction::ToggleDnd))),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Sync Now".into(),
                activate: Box::new(|_| emit(PlatformEvent::Menu(MenuAction::SyncNow))),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Quit Calen".into(),
                activate: Box::new(|_| emit(PlatformEvent::Menu(MenuAction::Quit))),
                ..Default::default()
            }
            .into(),
        ]
    }
}

static HANDLE: OnceLock<Mutex<ksni::blocking::Handle<CalenTray>>> = OnceLock::new();

pub fn early_init() {}

pub fn init_tray(dnd: bool) -> bool {
    let tray = CalenTray { dnd, problem: false, day: chrono::Datelike::day(&chrono::Local::now()) };
    match tray.spawn() {
        Ok(h) => {
            let _ = HANDLE.set(Mutex::new(h));
            true
        }
        Err(e) => {
            log::warn!("StatusNotifierItem tray unavailable: {e} (on GNOME, enable the AppIndicator extension)");
            false
        }
    }
}

pub fn update_tray(dnd: bool, problem: bool) {
    if let Some(h) = HANDLE.get() {
        let day = chrono::Datelike::day(&chrono::Local::now());
        let h = h.lock().unwrap();
        h.update(|t| {
            t.dnd = dnd;
            t.problem = problem;
            t.day = day;
        });
    }
}

pub fn main_window_platform() -> iced::window::settings::PlatformSpecific {
    iced::window::settings::PlatformSpecific { application_id: "calen".into(), ..Default::default() }
}

pub fn flyout_platform() -> iced::window::settings::PlatformSpecific {
    iced::window::settings::PlatformSpecific { application_id: "calen".into(), ..Default::default() }
}

/// SNI gives us the click point (logical on most desktops). Place the panel
/// next to it, flipping toward the screen centre.
pub fn flyout_position(x: f64, y: f64, _rect: Option<(f64, f64, f64, f64)>, size: iced::Size) -> iced::Point {
    let (w, h) = (size.width as f64, size.height as f64);
    let gap = 8.0;
    let px = (x - w / 2.0).max(gap);
    // Panel at the top (GNOME/KDE default top/bottom): open below or above the click.
    let py = if y < 300.0 { y + gap + 16.0 } else { (y - h - gap).max(gap) };
    iced::Point::new(px as f32, py as f32)
}

fn dbus() -> Option<zbus::blocking::Connection> {
    zbus::blocking::Connection::session().ok()
}

pub fn system_quiet_reason() -> Option<&'static str> {
    let conn = dbus()?;
    // KDE Plasma (and other servers implementing the property)
    if let Ok(p) = zbus::blocking::Proxy::new(
        &conn,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    ) && let Ok(true) = p.get_property::<bool>("Inhibited")
    {
        return Some("Do Not Disturb is on");
    }
    // GNOME: org.gnome.desktop.notifications show-banners
    if let Ok(out) = std::process::Command::new("gsettings").args(["get", "org.gnome.desktop.notifications", "show-banners"]).output()
        && out.status.success()
        && String::from_utf8_lossy(&out.stdout).trim() == "false"
    {
        return Some("Do Not Disturb is on");
    }
    // Feral GameMode
    if let Ok(p) = zbus::blocking::Proxy::new(
        &conn,
        "com.feralinteractive.GameMode",
        "/com/feralinteractive/GameMode",
        "com.feralinteractive.GameMode",
    ) && let Ok(n) = p.get_property::<i32>("ClientCount")
        && n > 0
    {
        return Some("a game is running (GameMode)");
    }
    None
}

pub fn notify(n: &Notice) -> Result<(), String> {
    let mut note = notify_rust::Notification::new();
    note.appname("Calen").summary(&n.title).body(&n.body).icon("x-office-calendar").timeout(notify_rust::Timeout::Never);
    note.action("default", "Open");
    if n.join_url.is_some() {
        note.action("join", "Join");
    }
    note.action("snooze", "Snooze 5 min");
    let handle = note.show().map_err(|e| e.to_string())?;
    let key = n.key.clone();
    let join = n.join_url.clone();
    handle.wait_for_action(|action| {
        let ev = match action {
            "join" => join.clone().map(NotificationAction::Join),
            "snooze" => Some(NotificationAction::Snooze(key.clone())),
            "default" => Some(NotificationAction::Open(key.clone())),
            _ => None,
        };
        if let Some(ev) = ev {
            emit(PlatformEvent::Notification(ev));
        }
    });
    Ok(())
}

// Focus loss closes the flyout on GNOME/KDE; there is no global click hook on Wayland.
pub fn try_focus(_raw: u64) -> bool {
    true
}
pub fn fade_out(_raw: u64) {}
pub fn watch_outside_clicks(_raw: u64) {}
pub fn unwatch_outside_clicks() {}

/// notify-rust reports a missing notification server as an error, which
/// triggers the in-app reminder popup instead.
pub fn toasts_blocked() -> Option<&'static str> {
    None
}

pub const NOTIFICATION_SETTINGS: Option<&str> = None;

/// Top-right corner (where GNOME/KDE show notifications), stacked downwards.
pub fn popup_position(index: usize, _size: iced::Size) -> iced::window::Position {
    fn at(i: usize, win: iced::Size, monitor: iced::Size) -> iced::Point {
        iced::Point::new((monitor.width - win.width - 16.0).max(0.0), 48.0 + i as f32 * (win.height + 8.0))
    }
    iced::window::Position::SpecificWith(match index {
        0 => |w, m| at(0, w, m),
        1 => |w, m| at(1, w, m),
        _ => |w, m| at(2, w, m),
    })
}
