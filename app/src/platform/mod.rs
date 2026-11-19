//! OS integration: tray icon + menu, single instance, autostart, notifications,
//! do-not-disturb / game-mode detection. Per-OS code lives in submodules.

use futures::channel::mpsc;
use iced::Subscription;
use std::sync::{Mutex, OnceLock};

use crate::app::Message;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use self::windows as os;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use self::linux as os;

#[cfg(not(any(windows, target_os = "linux")))]
mod other;
#[cfg(not(any(windows, target_os = "linux")))]
use self::other as os;

pub mod icon;
pub mod instance;

#[cfg_attr(not(windows), allow(dead_code))]
pub const APP_ID: &str = "com.felixadr.Calen";

/// Events coming from OS threads/callbacks into the iced app.
#[derive(Debug, Clone)]
pub enum PlatformEvent {
    /// Left click on the tray icon. Physical pixel anchor (x, y, w, h) of the icon if known.
    TrayClick {
        x: f64,
        y: f64,
        rect: Option<(f64, f64, f64, f64)>,
    },
    Menu(MenuAction),
    /// Another launch of the app asked us to show ourselves.
    SecondInstance,
    /// A notification button/body was activated.
    Notification(NotificationAction),
    /// The OS refused to show a notification; show it in-app instead.
    NotifyFailed(Notice),
    /// A mouse press outside the watched window (raw window id).
    #[cfg_attr(not(windows), allow(dead_code))]
    OutsideClick(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    Open,
    NewEvent,
    ToggleDnd,
    SyncNow,
    Quit,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NotificationAction {
    Open(String),
    Join(String),
    Snooze(String),
}

struct Channel {
    tx: mpsc::UnboundedSender<PlatformEvent>,
    rx: Mutex<Option<mpsc::UnboundedReceiver<PlatformEvent>>>,
}

fn channel() -> &'static Channel {
    static C: OnceLock<Channel> = OnceLock::new();
    C.get_or_init(|| {
        let (tx, rx) = mpsc::unbounded();
        Channel { tx, rx: Mutex::new(Some(rx)) }
    })
}

/// Send an event to the app from any thread.
pub fn emit(e: PlatformEvent) {
    let _ = channel().tx.unbounded_send(e);
}

pub fn subscription() -> Subscription<Message> {
    Subscription::run(|| {
        let rx = channel().rx.lock().unwrap().take();
        match rx {
            Some(rx) => futures::stream::StreamExt::boxed(futures::stream::StreamExt::map(rx, Message::Platform)),
            None => futures::stream::StreamExt::boxed(futures::stream::pending()),
        }
    })
}

static TRAY_OK: OnceLock<bool> = OnceLock::new();

/// Create the tray icon. Must be called on the main (event loop) thread.
pub fn init_tray(dnd: bool) {
    let ok = os::init_tray(dnd);
    let _ = TRAY_OK.set(ok);
    if !ok {
        log::warn!("system tray not available; closing the window will quit");
    }
}

/// Tray hover text, e.g. "Calen v0.1.2" or "Calen v0.1.2 — Do Not Disturb".
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
pub fn tray_tooltip(dnd: bool) -> String {
    let base = concat!("Calen v", env!("CARGO_PKG_VERSION"));
    if dnd { format!("{base} — Do Not Disturb") } else { base.to_string() }
}

pub fn tray_available() -> bool {
    TRAY_OK.get().copied().unwrap_or(false)
}

/// Refresh tray icon (date number / DND badge) and menu state.
pub fn update_tray(dnd: bool, problem: bool) {
    if tray_available() {
        os::update_tray(dnd, problem);
    }
}

pub fn window_icon() -> Option<iced::window::Icon> {
    let size = 64;
    iced::window::icon::from_rgba(icon::render(size, Some(0), false, false), size, size).ok()
}

pub fn main_window_platform() -> iced::window::settings::PlatformSpecific {
    os::main_window_platform()
}

pub fn flyout_platform() -> iced::window::settings::PlatformSpecific {
    os::flyout_platform()
}

/// Logical-pixel position for the flyout given the tray anchor.
pub fn flyout_position(x: f64, y: f64, rect: Option<(f64, f64, f64, f64)>, size: iced::Size) -> iced::Point {
    os::flyout_position(x, y, rect, size)
}

pub fn early_init() {
    os::early_init();
}

// ---------------------------------------------------------------------------
// Do-not-disturb

/// Why notifications should be held back right now (None = OK to notify).
pub fn system_quiet_reason() -> Option<&'static str> {
    os::system_quiet_reason()
}

// ---------------------------------------------------------------------------
// Notifications

#[derive(Debug, Clone)]
pub struct Notice {
    pub key: String,
    pub title: String,
    pub body: String,
    pub join_url: Option<String>,
}

pub fn notify(n: Notice) {
    std::thread::spawn(move || {
        if let Err(e) = os::notify(&n) {
            log::error!("notification failed: {e}");
            emit(PlatformEvent::NotifyFailed(n));
        }
    });
}

/// Why the OS won't show our notifications right now, if it won't.
pub fn toasts_blocked() -> Option<&'static str> {
    os::toasts_blocked()
}

/// Where the OS lets the user turn notifications on (a URL for `open`), if known.
pub fn notification_settings_url() -> Option<&'static str> {
    os::NOTIFICATION_SETTINGS
}

pub fn popup_position(index: usize, size: iced::Size) -> iced::window::Position {
    os::popup_position(index, size)
}

pub fn force_foreground(raw: u64) {
    os::force_foreground(raw);
}

/// Emit `PlatformEvent::OutsideClick` for presses outside this window (Windows).
pub fn watch_outside_clicks(raw: u64) {
    os::watch_outside_clicks(raw);
}

pub fn unwatch_outside_clicks() {
    os::unwatch_outside_clicks();
}

// ---------------------------------------------------------------------------
// Autostart

fn autolaunch() -> Option<auto_launch::AutoLaunch> {
    let exe = std::env::var("APPIMAGE").ok().or_else(|| std::env::current_exe().ok().map(|p| p.to_string_lossy().into_owned()))?;
    auto_launch::AutoLaunchBuilder::new().set_app_name("Calen").set_app_path(&exe).set_args(&["--minimized"]).build().ok()
}

pub fn set_autostart(enabled: bool) -> Result<(), String> {
    let al = autolaunch().ok_or("can't determine the app path")?;
    let r = if enabled { al.enable() } else { al.disable() };
    r.map_err(|e| e.to_string())
}

pub fn autostart_enabled() -> bool {
    autolaunch().and_then(|a| a.is_enabled().ok()).unwrap_or(false)
}
