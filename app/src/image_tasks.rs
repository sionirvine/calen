//! Image attachments: picking / drag-and-drop → upload → notes block; thumbnails.

use iced::Task;
use iced::widget::image::Handle;
use std::path::PathBuf;

use crate::app::{App, Message};

const KEYRING_SERVICE: &str = "calen-image-api";
const KEYRING_USER: &str = "default";

#[derive(Debug, Clone)]
pub enum ImageMsg {
    Picked(Vec<PathBuf>),
    Uploaded { count: usize, result: Result<Vec<String>, String> },
    Thumb(String, Result<calen_images::Rgba, String>),
}

/// API key from the OS keyring; in debug builds also `IMG_API_KEY` / `.env` (never compiled in).
pub fn api_key() -> Option<String> {
    if let Ok(k) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).and_then(|e| e.get_password())
        && !k.is_empty()
    {
        return Some(k);
    }
    if cfg!(debug_assertions) {
        if let Ok(k) = std::env::var("IMG_API_KEY") {
            return Some(k);
        }
        if let Ok(env) = std::fs::read_to_string(".env") {
            for line in env.lines() {
                if let Some(v) = line.trim().strip_prefix("IMG_API_KEY=") {
                    return Some(v.trim().trim_matches('"').to_string());
                }
            }
        }
    }
    None
}

pub fn has_api_key() -> bool {
    api_key().is_some()
}

pub fn save_api_key(key: &str) -> Result<(), String> {
    let e = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|e| e.to_string())?;
    if key.is_empty() {
        let _ = e.delete_credential();
        Ok(())
    } else {
        e.set_password(key).map_err(|e| e.to_string())
    }
}

fn cache_dir(app: &App) -> PathBuf {
    app.data_dir.join("thumbs")
}

pub fn load_thumbs(app: &mut App, urls: Vec<String>) -> Task<Message> {
    let dir = cache_dir(app);
    let tasks: Vec<Task<Message>> = urls
        .into_iter()
        .map(|url| {
            app.images.pending.insert(url.clone());
            let dir = dir.clone();
            Task::perform(
                async move {
                    let r = calen_images::thumbnail(&url, &dir).await.map_err(|e| e.to_string());
                    (url, r)
                },
                |(u, r)| Message::Images(ImageMsg::Thumb(u, r)),
            )
        })
        .collect();
    Task::batch(tasks)
}

pub fn files_dropped(app: &mut App, paths: Vec<PathBuf>) -> Task<Message> {
    let images: Vec<PathBuf> = paths.into_iter().filter(|p| calen_images::is_image_path(p)).collect();
    if images.is_empty() {
        app.flash("Only image files can be attached");
        return Task::none();
    }
    // Drop onto the open editor, or onto the selected event (opens its editor).
    if app.editor.is_none() {
        let Some(key) = app.selected.clone() else {
            app.flash("Select or open an event, then drop images on it");
            return Task::none();
        };
        if app.occ(&key).is_some_and(|o| o.read_only) {
            app.flash("This calendar is read-only");
            return Task::none();
        }
        let open = app.update(Message::OpenSelected);
        return Task::batch([open, Task::done(Message::Images(ImageMsg::Picked(images)))]);
    }
    update(app, ImageMsg::Picked(images))
}

pub fn pick_images(_app: &mut App) -> Task<Message> {
    Task::perform(
        async {
            rfd::AsyncFileDialog::new()
                .set_title("Attach images")
                .add_filter("Images", &calen_images::IMAGE_EXTENSIONS)
                .pick_files()
                .await
                .map(|files| files.into_iter().map(|f| f.path().to_path_buf()).collect::<Vec<_>>())
                .unwrap_or_default()
        },
        |files| Message::Images(ImageMsg::Picked(files)),
    )
}

pub fn update(app: &mut App, msg: ImageMsg) -> Task<Message> {
    match msg {
        ImageMsg::Picked(files) => {
            if files.is_empty() {
                return Task::none();
            }
            let Some(key) = api_key() else {
                app.flash("Set the image API key first: Ctrl+P → > Image upload: set API key");
                return Task::none();
            };
            let Some(ed) = &mut app.editor else { return Task::none() };
            let count = files.len();
            ed.uploading += count;
            let title = if ed.title.trim().is_empty() { "Calendar event".to_string() } else { ed.title.trim().to_string() };
            let public = app.settings.image_public;
            Task::perform(
                async move {
                    calen_images::upload(&key, &files, &title, public)
                        .await
                        .map(|(_, imgs)| imgs.into_iter().map(|i| i.url).collect::<Vec<_>>())
                        .map_err(|e| e.to_string())
                },
                move |result| Message::Images(ImageMsg::Uploaded { count, result }),
            )
        }
        ImageMsg::Uploaded { count, result } => {
            let Some(ed) = &mut app.editor else {
                if result.is_ok() {
                    app.flash("Upload finished after the editor was closed — images weren't attached");
                }
                return Task::none();
            };
            ed.uploading = ed.uploading.saturating_sub(count);
            match result {
                Ok(urls) => {
                    let new: Vec<String> = urls.into_iter().filter(|u| !ed.images.contains(u)).collect();
                    ed.images.extend(new.iter().cloned());
                    let wanted = app.images.wanted(new.iter());
                    return load_thumbs(app, wanted);
                }
                Err(e) => ed.error = Some(format!("Upload failed: {e}")),
            }
            Task::none()
        }
        ImageMsg::Thumb(url, result) => {
            match result {
                Ok(img) => app.images.insert_rgba(url, img.width, img.height, img.pixels),
                Err(e) => {
                    log::warn!("thumbnail {url}: {e}");
                    app.images.fail(&url);
                }
            }
            Task::none()
        }
    }
}

pub fn rgba_handle(w: u32, h: u32, px: Vec<u8>) -> Handle {
    Handle::from_rgba(w, h, px)
}
