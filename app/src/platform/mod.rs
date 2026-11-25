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

#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
pub const APP_ID: &str = "com.felixadr.Calen";

/// Running inside a Flatpak sandbox. The package then provides the desktop
/// entry, icons and updates, and autostart goes through the Background portal.
pub fn sandboxed() -> bool {
    static SANDBOXED: OnceLock<bool> = OnceLock::new();
    *SANDBOXED.get_or_init(|| cfg!(target_os = "linux") && std::path::Path::new("/.flatpak-info").exists())
}

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
    /// Calendar files to import (from the command line or another launch).
    OpenFiles(Vec<std::path::PathBuf>),
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
        log::warn!("system tray not available");
    }
    update_background_status(dnd, false);
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

/// Whether hiding the main window keeps Calen reachable: through the tray, or
/// on Linux (e.g. GNOME without the AppIndicator extension), by launching Calen
/// again, which brings the running copy's window back.
pub fn can_run_in_background() -> bool {
    tray_available() || cfg!(target_os = "linux")
}

/// The main window was just hidden. Without a tray (Linux), show a resident
/// "Calen is running" notification so it's clear Calen didn't quit.
pub fn entered_background() {
    #[cfg(target_os = "linux")]
    if !tray_available() && !shows_background_apps() {
        os::show_running_notice();
    }
}

/// The main window is back: remove the "Calen is running" notification.
pub fn left_background() {
    #[cfg(target_os = "linux")]
    os::hide_running_notice();
}

/// How the Windows tray tooltip's clock writes the time and date.
pub fn set_clock_prefs(time_24h: bool, month_first: bool) {
    #[cfg(windows)]
    os::set_clock_prefs(time_24h, month_first);
    #[cfg(not(windows))]
    let _ = (time_24h, month_first);
}

/// Refresh tray icon (date number / DND badge) and menu state.
pub fn update_tray(dnd: bool, problem: bool) {
    if tray_available() {
        os::update_tray(dnd, problem);
    }
    update_background_status(dnd, problem);
}

/// GNOME (44+) lists a sandboxed app that runs without a window under
/// Quick Settings → Background Apps, with a button to stop it. There the
/// "Calen is running" notification would only repeat that.
#[cfg(target_os = "linux")]
fn shows_background_apps() -> bool {
    sandboxed() && std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.split(':').any(|d| d.eq_ignore_ascii_case("GNOME")))
}

/// The line shown under Calen in GNOME's Background Apps (Flatpak only).
fn update_background_status(dnd: bool, problem: bool) {
    #[cfg(target_os = "linux")]
    if sandboxed() {
        os::set_background_status(background_status(dnd, problem));
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (dnd, problem);
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn background_status(dnd: bool, problem: bool) -> &'static str {
    match (dnd, problem) {
        (false, false) => "Reminders are on",
        (false, true) => "Reminders are on · Sync problem",
        (true, false) => "Do Not Disturb: reminders are held",
        (true, true) => "Do Not Disturb · Sync problem",
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

/// Make the window the focused foreground window; true once it is (or when the
/// platform has nothing to do).
pub fn try_focus(raw: u64) -> bool {
    os::try_focus(raw)
}

/// Return idle memory pages to the OS (Windows: empty the working set).
pub fn trim_memory() {
    os::trim_memory();
}

/// Hide a window with a short fade where the platform supports it.
pub fn fade_out(raw: u64) {
    os::fade_out(raw);
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
    #[cfg(target_os = "linux")]
    if sandboxed() {
        return os::request_background(enabled);
    }
    let al = autolaunch().ok_or("can't determine the app path")?;
    let r = if enabled { al.enable() } else { al.disable() };
    r.map_err(|e| e.to_string())
}

pub fn autostart_enabled() -> bool {
    // The Background portal can't be asked for the current state; the setting
    // is the source of truth, and the portal is told whenever it changes.
    if sandboxed() {
        return true;
    }
    autolaunch().and_then(|a| a.is_enabled().ok()).unwrap_or(false)
}
