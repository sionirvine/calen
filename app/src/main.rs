#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod alert_popup;
mod app;
mod bench;
mod datefmt;
mod edit;
mod flyout;
mod icons;
mod image_tasks;
mod images;
mod model;
mod notify;
mod palette;
mod platform;
mod seed;
mod settings;
mod storage;
mod sync_hook;
mod theme;
mod updater;
mod viewer;
mod views;

use app::{App, Flags};

/// Log sink: stderr plus `calen.log` in the data folder once it's opened
/// (release builds on Windows have no console, so the file is the only record).
struct LogTee;

static LOG_FILE: std::sync::OnceLock<std::sync::Mutex<(std::fs::File, u64)>> = std::sync::OnceLock::new();
const LOG_LIMIT: u64 = 2 * 1024 * 1024;

impl std::io::Write for LogTee {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        if let Some(f) = LOG_FILE.get()
            && let Ok(mut g) = f.lock()
            && g.1 < LOG_LIMIT
        {
            g.1 += buf.len() as u64;
            let _ = g.0.write_all(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if let Some(f) = LOG_FILE.get()
            && let Ok(mut g) = f.lock()
        {
            let _ = g.0.flush();
        }
        Ok(())
    }
}

/// Start a fresh `calen.log` (only the running instance does this).
fn open_log_file(dir: &std::path::Path) {
    if let Ok(f) = std::fs::File::create(dir.join("calen.log")) {
        let _ = LOG_FILE.set(std::sync::Mutex::new((f, 0)));
        log::info!("Calen {} starting", env!("CARGO_PKG_VERSION"));
    }
}

/// Pick iced's renderer from the saved setting (an explicit ICED_BACKEND wins).
/// Must run before any other thread starts, since it sets an environment variable.
fn choose_renderer(data_dir: &std::path::Path) {
    if std::env::var_os("ICED_BACKEND").is_none() {
        let renderer = calen_store::Store::open(&data_dir.join("calen.db"))
            .ok()
            .and_then(|s| s.setting::<settings::Settings>(settings::Settings::KEY).ok().flatten())
            .map(|s| s.renderer)
            .unwrap_or_default();
        // SAFETY: called at the top of main, before any threads are spawned.
        unsafe { std::env::set_var("ICED_BACKEND", renderer.iced_backend()) };
    }
    let backend = std::env::var("ICED_BACKEND").unwrap_or_default();
    theme::set_cpu_rendering(backend.trim().starts_with("tiny-skia"));
}

fn update(app: &mut App, message: app::Message) -> iced::Task<app::Message> {
    bench::timed("update", || app.update(message))
}

fn view(app: &App, window: iced::window::Id) -> iced::Element<'_, app::Message> {
    bench::timed("view", || views::view(app, window))
}

fn main() -> iced::Result {
    env_logger::Builder::from_env(
        env_logger::Env::default()
            .default_filter_or("info,wgpu=warn,naga=warn,iced_wgpu=warn,iced_winit=warn,cosmic_text=warn,zbus=warn,tracing=warn"),
    )
    .target(env_logger::Target::Pipe(Box::new(LogTee)))
    .init();
    // Panics (also in background tasks) go to calen.log, not only to stderr.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}");
        default_hook(info);
    }));
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--export-icons") {
        let dir = args.get(i + 1).map(std::path::PathBuf::from).unwrap_or_else(|| "assets".into());
        platform::icon::export(&dir).expect("export icons");
        return Ok(());
    }
    let start_minimized = std::env::args().any(|a| a == "--minimized");
    let data_dir = std::env::var_os("CALEN_DATA_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| directories::ProjectDirs::from("com", "felixadr", "Calen").map(|d| d.data_dir().to_path_buf()))
        .unwrap_or_else(|| std::env::temp_dir().join("calen"));
    let _ = std::fs::create_dir_all(&data_dir);
    choose_renderer(&data_dir);

    if !platform::instance::acquire(&data_dir) {
        log::info!("Calen is already running; asked it to show itself");
        return Ok(());
    }
    open_log_file(&data_dir);
    platform::early_init();
    if updater::apply_staged(&data_dir) {
        return Ok(());
    }

    let flags = std::sync::Mutex::new(Some(Flags { start_minimized, data_dir }));
    iced::daemon(move || App::boot(flags.lock().unwrap().take().expect("boot once")), update, view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .antialiasing(true)
        .run()
}
