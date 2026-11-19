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

pub fn early_init() {
    if let Err(e) = install_desktop_entry() {
        log::warn!("couldn't install the desktop entry: {e}");
    }
}

/// GNOME on Wayland shows a window's icon from the `.desktop` file that matches
/// its app id, and lists apps in the grid from those files. An AppImage (or a
/// bare binary) has none, so write one, plus icons, to `~/.local/share`.
/// Files are only rewritten when their contents change.
fn install_desktop_entry() -> std::io::Result<()> {
    let id = super::APP_ID;
    if std::path::Path::new(&format!("/usr/share/applications/{id}.desktop")).exists() {
        return Ok(()); // installed by a system package
    }
    let Some(exe) = std::env::var_os("APPIMAGE").map(std::path::PathBuf::from).or_else(|| std::env::current_exe().ok()) else {
        return Ok(());
    };
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")))
        .ok_or_else(|| std::io::Error::other("no home directory"))?;

    for size in [32u32, 48, 64, 128, 256] {
        let path = data.join(format!("icons/hicolor/{size}x{size}/apps/{id}.png"));
        write_if_changed(&path, &icon::png(size))?;
    }
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Calen\nGenericName=Calendar\nComment=A fast calendar that lives in your tray\n\
         Exec={}\nIcon={id}\nTerminal=false\nCategories=Office;Calendar;\nStartupWMClass={id}\n",
        desktop_quote(&exe.to_string_lossy())
    );
    write_if_changed(&data.join(format!("applications/{id}.desktop")), entry.as_bytes())
}

fn write_if_changed(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    if std::fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, bytes)
}

/// Quote a path for a desktop file's `Exec=` key.
fn desktop_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '%' => out.push_str("%%"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

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

/// Without a tray, GNOME can still list Calen under "Background Apps" if the
/// XDG Background portal accepts it. Best effort: failures are only logged.
struct RunningNotice {
    wanted: bool,
    id: Option<u32>,
}

static RUNNING_NOTICE: Mutex<RunningNotice> = Mutex::new(RunningNotice { wanted: false, id: None });

/// With no tray icon, this resident notification is what shows that Calen is
/// still running: it stays in GNOME's notification list until the window is
/// back, and offers Open / Quit.
pub fn show_running_notice() {
    {
        let mut st = RUNNING_NOTICE.lock().unwrap();
        if st.wanted {
            return;
        }
        st.wanted = true;
    }
    std::thread::spawn(|| {
        let mut note = notify_rust::Notification::new();
        note.appname("Calen")
            .summary("Calen is running in the background")
            .body("Reminders stay on. For a tray icon, install the AppIndicator extension.")
            .icon(super::APP_ID)
            .hint(notify_rust::Hint::DesktopEntry(super::APP_ID.into()))
            .hint(notify_rust::Hint::Resident(true))
            .urgency(notify_rust::Urgency::Low)
            .timeout(notify_rust::Timeout::Never);
        note.action("default", "Open");
        note.action("quit", "Quit Calen");
        let handle = match note.show() {
            Ok(h) => h,
            Err(e) => {
                RUNNING_NOTICE.lock().unwrap().wanted = false;
                return log::info!("running notice: {e}");
            }
        };
        {
            let mut st = RUNNING_NOTICE.lock().unwrap();
            if !st.wanted {
                // The window came back while we were showing it.
                drop(st);
                return handle.close();
            }
            st.id = Some(handle.id());
        }
        handle.wait_for_action(|action| match action {
            "default" => emit(PlatformEvent::Notification(NotificationAction::Open(String::new()))),
            "quit" => emit(PlatformEvent::Menu(MenuAction::Quit)),
            _ => {}
        });
    });
}

pub fn hide_running_notice() {
    let id = {
        let mut st = RUNNING_NOTICE.lock().unwrap();
        st.wanted = false;
        st.id.take()
    };
    if let (Some(id), Some(conn)) = (id, dbus())
        && let Ok(p) = zbus::blocking::Proxy::new(
            &conn,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
        )
    {
        let _ = p.call::<_, _, ()>("CloseNotification", &(id,));
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
    iced::window::settings::PlatformSpecific { application_id: super::APP_ID.into(), ..Default::default() }
}

pub fn flyout_platform() -> iced::window::settings::PlatformSpecific {
    iced::window::settings::PlatformSpecific { application_id: super::APP_ID.into(), ..Default::default() }
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

/// The session bus, connected once and reused (the DND check runs every 20 s).
fn dbus() -> Option<zbus::blocking::Connection> {
    static CONN: OnceLock<Option<zbus::blocking::Connection>> = OnceLock::new();
    CONN.get_or_init(|| zbus::blocking::Connection::session().ok()).clone()
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
    note.appname("Calen")
        .summary(&n.title)
        .body(&n.body)
        .icon(super::APP_ID)
        .hint(notify_rust::Hint::DesktopEntry(super::APP_ID.into()))
        .timeout(notify_rust::Timeout::Never);
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
pub fn trim_memory() {}
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
