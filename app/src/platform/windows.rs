//! Windows: tray-icon, WinRT toasts, Focus Assist / full-screen detection.

use std::cell::RefCell;

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::Shell::{
    QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_QUIET_TIME, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
    SetCurrentProcessExplicitAppUserModelID,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowRect, SPI_GETWORKAREA, SystemParametersInfoW,
};

use super::{APP_ID, MenuAction, Notice, NotificationAction, PlatformEvent, emit, icon};

struct Tray {
    icon: TrayIcon,
    dnd_item: CheckMenuItem,
    day: u32,
    dnd: bool,
    problem: bool,
}

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn early_init() {
    unsafe {
        SetCurrentProcessExplicitAppUserModelID(wide(APP_ID).as_ptr());
    }
    if let Err(e) = register_aumid() {
        log::warn!("couldn't register notification identity: {e}");
    }
}

/// Lets toasts show "Calen" with our icon without an installer/shortcut.
fn register_aumid() -> std::io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let dir = std::env::temp_dir().join("calen");
    std::fs::create_dir_all(&dir)?;
    let ico = dir.join("calen.ico");
    std::fs::write(&ico, ico_bytes(64))?;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(format!(r"Software\Classes\AppUserModelId\{APP_ID}"))?;
    key.set_value("DisplayName", &"Calen")?;
    key.set_value("IconUri", &ico.to_string_lossy().to_string())?;
    key.set_value("IconBackgroundColor", &"FFE5484D")?;
    Ok(())
}

/// A single-image .ico (32-bit BMP).
fn ico_bytes(size: u32) -> Vec<u8> {
    let rgba = icon::render(size, Some(0), false, false);
    let mut bmp = Vec::new();
    let mask_row = size.div_ceil(32) * 4;
    // BITMAPINFOHEADER
    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&(size as i32).to_le_bytes());
    bmp.extend_from_slice(&((size * 2) as i32).to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&32u16.to_le_bytes());
    bmp.extend_from_slice(&[0u8; 24]);
    for y in (0..size).rev() {
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            bmp.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
        }
    }
    bmp.extend(std::iter::repeat_n(0u8, (mask_row * size) as usize));
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    let dim = if size >= 256 { 0 } else { size as u8 };
    out.extend_from_slice(&[dim, dim, 0, 0]);
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&(bmp.len() as u32).to_le_bytes());
    out.extend_from_slice(&22u32.to_le_bytes());
    out.extend(bmp);
    out
}

fn tray_icon(dnd: bool, problem: bool) -> Option<Icon> {
    let size = 32;
    Icon::from_rgba(icon::render(size, None, dnd, problem), size, size).ok()
}

pub fn init_tray(dnd: bool) -> bool {
    let menu = Menu::new();
    let open = MenuItem::with_id("open", "Open Calen", true, None);
    let new = MenuItem::with_id("new", "New Event…", true, None);
    let dnd_item = CheckMenuItem::with_id("dnd", "Do Not Disturb", true, dnd, None);
    let sync = MenuItem::with_id("sync", "Sync Now", true, None);
    let quit = MenuItem::with_id("quit", "Quit Calen", true, None);
    let sep = PredefinedMenuItem::separator();
    let sep2 = PredefinedMenuItem::separator();
    if menu.append_items(&[&open, &new, &sep, &dnd_item, &sync, &sep2, &quit]).is_err() {
        return false;
    }

    MenuEvent::set_event_handler(Some(|e: MenuEvent| {
        let action = match e.id.as_ref() {
            "open" => MenuAction::Open,
            "new" => MenuAction::NewEvent,
            "dnd" => MenuAction::ToggleDnd,
            "sync" => MenuAction::SyncNow,
            "quit" => MenuAction::Quit,
            _ => return,
        };
        emit(PlatformEvent::Menu(action));
    }));
    TrayIconEvent::set_event_handler(Some(|e: TrayIconEvent| {
        if let TrayIconEvent::Click { position, rect, button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
            emit(PlatformEvent::TrayClick {
                x: position.x,
                y: position.y,
                rect: Some((rect.position.x, rect.position.y, rect.size.width as f64, rect.size.height as f64)),
            });
        }
    }));

    let built = TrayIconBuilder::new()
        .with_tooltip("Calen")
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_icon(match tray_icon(dnd, false) {
            Some(i) => i,
            None => return false,
        })
        .build();
    match built {
        Ok(icon) => {
            TRAY.with(|t| {
                *t.borrow_mut() = Some(Tray { icon, dnd_item, day: today(), dnd, problem: false });
            });
            true
        }
        Err(e) => {
            log::error!("tray: {e}");
            false
        }
    }
}

fn today() -> u32 {
    chrono::Datelike::day(&chrono::Local::now())
}

pub fn update_tray(dnd: bool, problem: bool) {
    TRAY.with(|t| {
        if let Some(t) = t.borrow_mut().as_mut() {
            let day = today();
            if t.day != day || t.dnd != dnd || t.problem != problem {
                let _ = t.icon.set_icon(tray_icon(dnd, problem));
                t.day = day;
                t.dnd = dnd;
                t.problem = problem;
            }
            if t.dnd_item.is_checked() != dnd {
                t.dnd_item.set_checked(dnd);
            }
            let tip = if dnd { "Calen — Do Not Disturb" } else { "Calen" };
            let _ = t.icon.set_tooltip(Some(tip));
        }
    });
}

pub fn main_window_platform() -> iced::window::settings::PlatformSpecific {
    iced::window::settings::PlatformSpecific { drag_and_drop: true, ..Default::default() }
}

pub fn flyout_platform() -> iced::window::settings::PlatformSpecific {
    iced::window::settings::PlatformSpecific {
        drag_and_drop: false,
        skip_taskbar: true,
        undecorated_shadow: true,
        corner_preference: iced::window::settings::platform::CornerPreference::Round,
    }
}

fn scale() -> f64 {
    let dpi = unsafe { GetDpiForSystem() };
    if dpi == 0 { 1.0 } else { dpi as f64 / 96.0 }
}

fn work_area() -> Option<(f64, f64, f64, f64)> {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let ok = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut r as *mut RECT as *mut _, 0) };
    (ok != 0).then_some((r.left as f64, r.top as f64, r.right as f64, r.bottom as f64))
}

pub fn flyout_position(x: f64, y: f64, rect: Option<(f64, f64, f64, f64)>, size: iced::Size) -> iced::Point {
    let s = scale();
    let (w, h) = (size.width as f64 * s, size.height as f64 * s);
    let (ax, ay, aw, ah) = rect.unwrap_or((x, y, 1.0, 1.0));
    let (l, t, r, b) = work_area().unwrap_or((0.0, 0.0, 1920.0 * s, 1040.0 * s));
    let gap = 10.0 * s;
    // Taskbar edge: whichever side of the work area the icon sits outside of.
    let (mut px, mut py) = if ay >= b - 2.0 {
        (ax + aw / 2.0 - w / 2.0, b - h - gap)
    } else if ay + ah <= t + 2.0 {
        (ax + aw / 2.0 - w / 2.0, t + gap)
    } else if ax >= r - 2.0 {
        (r - w - gap, ay + ah / 2.0 - h / 2.0)
    } else if ax + aw <= l + 2.0 {
        (l + gap, ay + ah / 2.0 - h / 2.0)
    } else {
        (ax - w / 2.0, ay - h - gap)
    };
    px = px.clamp(l + gap, (r - w - gap).max(l + gap));
    py = py.clamp(t + gap, (b - h - gap).max(t + gap));
    iced::Point::new((px / s) as f32, (py / s) as f32)
}

pub fn system_quiet_reason() -> Option<&'static str> {
    let mut state = 0;
    let hr = unsafe { SHQueryUserNotificationState(&mut state) };
    if hr != 0 {
        return None;
    }
    match state {
        QUNS_RUNNING_D3D_FULL_SCREEN => Some("a full-screen game is running"),
        // "Busy" is also reported for live-wallpaper apps; only trust it when the
        // focused window really covers its monitor (borderless games, video players).
        QUNS_BUSY if foreground_is_fullscreen() => Some("a full-screen app or game is running"),
        QUNS_PRESENTATION_MODE => Some("presentation mode is on"),
        QUNS_QUIET_TIME => Some("Windows quiet time is on"),
        _ => None,
    }
}

fn foreground_is_fullscreen() -> bool {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return false;
        }
        let mut class = [0u16; 64];
        let len = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
        let class = String::from_utf16_lossy(&class[..len.max(0) as usize]);
        if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd") {
            return false;
        }
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if GetWindowRect(hwnd, &mut r) == 0 {
            return false;
        }
        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(mon, &mut mi) == 0 {
            return false;
        }
        let m = mi.rcMonitor;
        r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
    }
}

pub fn notify(n: &Notice) -> Result<(), String> {
    use tauri_winrt_notification::{Duration, Sound, Toast};
    let key = n.key.clone();
    let join = n.join_url.clone();
    let mut toast = Toast::new(APP_ID).title(&n.title).text1(&n.body).sound(Some(Sound::Reminder)).duration(Duration::Long);
    if let Some(url) = &n.join_url {
        toast = toast.add_button("Join", &format!("join|{url}"));
    }
    toast = toast.add_button("Snooze 5 min", &format!("snooze|{key}"));
    let key2 = key.clone();
    toast = toast.on_activated(move |action| {
        let ev = match action.as_deref() {
            Some(a) if a.starts_with("join|") => NotificationAction::Join(a[5..].to_string()),
            Some(a) if a.starts_with("snooze|") => NotificationAction::Snooze(a[7..].to_string()),
            _ => match &join {
                Some(_) | None => NotificationAction::Open(key2.clone()),
            },
        };
        emit(PlatformEvent::Notification(ev));
        Ok(())
    });
    toast.show().map_err(|e| e.to_string())?;
    // Keep the WinRT object (and its callbacks) alive while the toast can be clicked.
    std::thread::sleep(std::time::Duration::from_secs(15 * 60));
    drop(toast);
    Ok(())
}
