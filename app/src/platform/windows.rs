//! Windows: tray-icon, WinRT toasts, Focus Assist / full-screen detection.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::Shell::{
    QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_QUIET_TIME, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
    SetCurrentProcessExplicitAppUserModelID,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CallNextHookEx, GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId, HHOOK, IsWindowVisible,
    MSLLHOOKSTRUCT, SPI_GETWORKAREA, SW_HIDE, SetForegroundWindow, SetTimer, SetWindowsHookExW, ShowWindow, SystemParametersInfoW,
    UnhookWindowsHookEx, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_RBUTTONDOWN,
};

use super::{APP_ID, MenuAction, Notice, NotificationAction, PlatformEvent, emit, icon};

struct Tray {
    icon: TrayIcon,
    dnd_item: CheckMenuItem,
    day: u32,
    dnd: bool,
    problem: bool,
    tip: String,
}

static CLOCK_24H: AtomicBool = AtomicBool::new(true);
static MONTH_FIRST: AtomicBool = AtomicBool::new(false);

/// How the tooltip writes the time and date.
pub fn set_clock_prefs(time_24h: bool, month_first: bool) {
    CLOCK_24H.store(time_24h, Ordering::Relaxed);
    MONTH_FIRST.store(month_first, Ordering::Relaxed);
}

/// "20 November 2026 / 14:06:00 / (blank) / Calen v0.1.8".
fn tooltip(dnd: bool) -> String {
    let now = chrono::Local::now();
    let fmt = if MONTH_FIRST.load(Ordering::Relaxed) { crate::datefmt::DateFormat::MmDd } else { crate::datefmt::DateFormat::DdMm };
    let date = crate::datefmt::long(now.date_naive(), fmt);
    let time = if CLOCK_24H.load(Ordering::Relaxed) { now.format("%H:%M:%S") } else { now.format("%-I:%M:%S %p") };
    format!("{date}\n{time}\n\n{}", super::tray_tooltip(dnd))
}

/// Refresh the tooltip's clock. Runs once a second from a thread timer on the
/// event-loop thread, so it never wakes iced or redraws a window.
unsafe extern "system" fn clock_tick(_: windows_sys::Win32::Foundation::HWND, _: u32, _: usize, _: u32) {
    TRAY.with(|t| {
        if let Ok(mut t) = t.try_borrow_mut()
            && let Some(t) = t.as_mut()
        {
            let tip = tooltip(t.dnd);
            if tip != t.tip {
                let _ = t.icon.set_tooltip(Some(&tip));
                t.tip = tip;
            }
        }
    });
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
        match e {
            TrayIconEvent::Click { position, rect, button: MouseButton::Left, button_state: MouseButtonState::Up, .. } => {
                emit(PlatformEvent::TrayClick {
                    x: position.x,
                    y: position.y,
                    rect: Some((rect.position.x, rect.position.y, rect.size.width as f64, rect.size.height as f64)),
                });
            }
            // The first click of the pair has opened the flyout; opening the
            // main window closes it again.
            TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => emit(PlatformEvent::Menu(MenuAction::Open)),
            _ => {}
        }
    }));

    let built = TrayIconBuilder::new()
        .with_tooltip(tooltip(dnd))
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
                *t.borrow_mut() = Some(Tray { icon, dnd_item, day: today(), dnd, problem: false, tip: String::new() });
            });
            unsafe {
                SetTimer(std::ptr::null_mut(), 0, 1000, Some(clock_tick));
            }
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
            let tip = tooltip(dnd);
            if tip != t.tip {
                let _ = t.icon.set_tooltip(Some(&tip));
                t.tip = tip;
            }
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

/// Try to make `raw` (an HWND) the focused foreground window. Returns true once
/// it is. Does nothing until the window is visible: iced shows new windows only
/// after their first frame, and Windows ignores focus requests for hidden ones.
pub fn try_focus(raw: u64) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VK_MENU,
    };
    let hwnd = raw as usize as *mut core::ffi::c_void;
    unsafe {
        if IsWindowVisible(hwnd) == 0 {
            return false;
        }
        if GetForegroundWindow() == hwnd {
            return true;
        }
        if SetForegroundWindow(hwnd) != 0 && GetForegroundWindow() == hwnd {
            return true;
        }
        // A tray click doesn't give us the right to take the foreground. A synthetic
        // Alt press does (this process then sent the last input); winit uses the same trick.
        let key = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_MENU, wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
        };
        let inputs = [key(0), key(KEYEVENTF_KEYUP)];
        SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
        BringWindowToTop(hwnd);
        SetForegroundWindow(hwnd);
        if GetForegroundWindow() == hwnd {
            return true;
        }
        // Last resort: borrow the foreground thread's input state.
        let fg = GetForegroundWindow();
        let fg_thread = if fg.is_null() { 0 } else { GetWindowThreadProcessId(fg, std::ptr::null_mut()) };
        let me = GetCurrentThreadId();
        if fg_thread != 0 && fg_thread != me {
            AttachThreadInput(me, fg_thread, 1);
            BringWindowToTop(hwnd);
            SetForegroundWindow(hwnd);
            AttachThreadInput(me, fg_thread, 0);
        }
        GetForegroundWindow() == hwnd
    }
}

pub fn trim_memory() {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
    unsafe {
        SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
    log::debug!("trimmed working set");
}

/// Fade a window out (~120 ms) instead of letting it vanish abruptly.
///
/// `AnimateWindow(AW_BLEND)` can't be used: it snapshots the window with
/// `WM_PRINT`, which iced never draws for, so an unthemed blank window faded
/// out. Instead the window becomes layered for the fade, and its alpha steps
/// down over the frame already on screen. The old style comes back once it's
/// hidden, so winit's idea of the window stays right. GPU swapchains (flip
/// model) don't draw in layered windows, so there the window just hides.
pub fn fade_out(raw: u64) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, LWA_ALPHA, SetLayeredWindowAttributes, SetWindowLongPtrW, WS_EX_LAYERED,
    };
    let hwnd = raw as usize as *mut core::ffi::c_void;
    unsafe {
        if IsWindowVisible(hwnd) == 0 {
            return;
        }
        let cpu = std::env::var("ICED_BACKEND").is_ok_and(|b| b.starts_with("tiny-skia"));
        if cpu {
            let old = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            if old & WS_EX_LAYERED as isize == 0 {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, old | WS_EX_LAYERED as isize);
            }
            if SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA) != 0 {
                const STEPS: u32 = 8;
                for i in 1..=STEPS {
                    SetLayeredWindowAttributes(hwnd, 0, (255 * (STEPS - i) / STEPS) as u8, LWA_ALPHA);
                    std::thread::sleep(std::time::Duration::from_millis(15));
                }
            }
            ShowWindow(hwnd, SW_HIDE);
            SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, old);
        } else {
            ShowWindow(hwnd, SW_HIDE);
        }
    }
}

thread_local! {
    static OUTSIDE_HOOK: RefCell<Option<HHOOK>> = const { RefCell::new(None) };
}
static WATCHED: AtomicU64 = AtomicU64::new(0);

unsafe extern "system" fn mouse_hook(code: i32, wparam: usize, lparam: isize) -> isize {
    if code >= 0 && matches!(wparam as u32, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN) {
        let raw = WATCHED.load(Ordering::Relaxed);
        if raw != 0 {
            let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
            let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            let hwnd = raw as usize as *mut core::ffi::c_void;
            if unsafe { GetWindowRect(hwnd, &mut r) } != 0 {
                let (x, y) = (info.pt.x, info.pt.y);
                if x < r.left || x >= r.right || y < r.top || y >= r.bottom {
                    emit(PlatformEvent::OutsideClick(raw));
                }
            } else {
                log::warn!("flyout: can't read its window rect");
            }
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Report mouse presses outside `raw` (an HWND) as `PlatformEvent::OutsideClick`
/// until `unwatch_outside_clicks`. Must run on the event-loop thread (the hook
/// is serviced by that thread's message loop).
pub fn watch_outside_clicks(raw: u64) {
    unwatch_outside_clicks();
    let hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), GetModuleHandleW(std::ptr::null()), 0) };
    if hook.is_null() {
        log::warn!("couldn't watch clicks outside the flyout");
        return;
    }
    WATCHED.store(raw, Ordering::Relaxed);
    OUTSIDE_HOOK.with(|h| *h.borrow_mut() = Some(hook));
}

pub fn unwatch_outside_clicks() {
    WATCHED.store(0, Ordering::Relaxed);
    OUTSIDE_HOOK.with(|h| {
        if let Some(hook) = h.borrow_mut().take() {
            unsafe { UnhookWindowsHookEx(hook) };
        }
    });
}

/// Why Windows won't show our toasts, if it won't.
pub fn toasts_blocked() -> Option<&'static str> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let read = |path: &str, name: &str| hkcu.open_subkey(path).ok().and_then(|k| k.get_value::<u32, _>(name).ok());
    if read(r"Software\Microsoft\Windows\CurrentVersion\PushNotifications", "ToastEnabled") == Some(0) {
        return Some("Windows notifications are turned off");
    }
    if read(&format!(r"Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\{APP_ID}"), "Enabled") == Some(0) {
        return Some("Calen's notifications are turned off in Windows settings");
    }
    None
}

/// Logical position for the `index`-th reminder popup, stacked up from the
/// bottom-right corner of the work area.
pub fn popup_position(index: usize, size: iced::Size) -> iced::window::Position {
    let s = scale();
    let (_, t, r, b) = work_area().unwrap_or((0.0, 0.0, 1920.0 * s, 1040.0 * s));
    let gap = 12.0;
    let x = r / s - size.width as f64 - gap;
    let y = (b / s - (size.height as f64 + gap) * (index as f64 + 1.0)).max(t / s + gap);
    iced::window::Position::Specific(iced::Point::new(x as f32, y as f32))
}

pub const NOTIFICATION_SETTINGS: Option<&str> = Some("ms-settings:notifications");

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
