#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod app;
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

fn main() -> iced::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,wgpu=warn,naga=warn,iced_wgpu=warn,cosmic_text=warn"))
        .init();
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

    if !platform::instance::acquire(&data_dir) {
        log::info!("Calen is already running; asked it to show itself");
        return Ok(());
    }
    platform::early_init();
    if updater::apply_staged(&data_dir) {
        return Ok(());
    }

    let flags = std::sync::Mutex::new(Some(Flags { start_minimized, data_dir }));
    iced::daemon(move || App::boot(flags.lock().unwrap().take().expect("boot once")), App::update, views::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .antialiasing(true)
        .run()
}
