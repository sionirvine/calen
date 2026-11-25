//! Image attachments: picking / drag-and-drop → upload → notes block; thumbnails.

use iced::Task;
use iced::widget::image::Handle;
use std::path::PathBuf;

use crate::app::{App, Message};

#[derive(Debug, Clone)]
pub enum ImageMsg {
    Picked(Vec<PathBuf>),
    Uploaded {
        files: Vec<PathBuf>,
        result: Result<Vec<String>, String>,
    },
    Thumb(String, Result<calen_images::Rgba, String>),
    /// Try a failed thumbnail download again.
    Retry(String),
}

fn cache_dir(app: &App) -> PathBuf {
    app.data_dir.join("thumbs")
}

pub fn load_thumbs(app: &mut App, urls: Vec<String>) -> Task<Message> {
    let tasks: Vec<Task<Message>> = urls.into_iter().map(|url| fetch_thumb(app, url)).collect();
    Task::batch(tasks)
}

fn fetch_thumb(app: &mut App, url: String) -> Task<Message> {
    app.images.pending.insert(url.clone());
    let dir = cache_dir(app);
    Task::perform(
        async move {
            let r = calen_images::thumbnail(&url, &dir).await.map_err(|e| e.to_string());
            (url, r)
        },
        |(u, r)| Message::Images(ImageMsg::Thumb(u, r)),
    )
}

/// Thumbnails for just-uploaded images, made from the files themselves (the
/// host may not serve them yet, and there's no need to download them back).
fn thumbs_from_files(app: &mut App, pairs: Vec<(String, PathBuf)>) -> Task<Message> {
    let dir = cache_dir(app);
    let tasks: Vec<Task<Message>> = pairs
        .into_iter()
        .map(|(url, file)| {
            app.images.pending.insert(url.clone());
            let dir = dir.clone();
            Task::perform(
                async move {
                    let r = match calen_images::thumbnail_from_file(&url, file, &dir).await {
                        Ok(t) => Ok(t),
                        // Unreadable here (e.g. HEIC): use the host's copy instead.
                        Err(_) => calen_images::thumbnail(&url, &dir).await,
                    };
                    (url, r.map_err(|e| e.to_string()))
                },
                |(u, r)| Message::Images(ImageMsg::Thumb(u, r)),
            )
        })
        .collect();
    Task::batch(tasks)
}

/// The viewer has the full image of `url`: make its missing thumbnail from it.
pub fn thumb_from_full(app: &mut App, url: String, full: calen_images::Rgba) -> Task<Message> {
    if app.images.thumb(&url).is_some() {
        return Task::none();
    }
    let dir = cache_dir(app);
    Task::perform(
        async move {
            let r = calen_images::thumbnail_from_rgba(&url, full, &dir).await.map_err(|e| e.to_string());
            (url, r)
        },
        |(u, r)| Message::Images(ImageMsg::Thumb(u, r)),
    )
}

pub fn files_dropped(app: &mut App, paths: Vec<PathBuf>) -> Task<Message> {
    let images: Vec<PathBuf> = paths.into_iter().filter(|p| calen_images::is_image_path(p)).collect();
    if images.is_empty() {
        app.flash("Only image files can be attached");
        return Task::none();
    }
    if !crate::storage::enabled(app) {
        app.flash("Image uploads are off — choose where to store images first");
        return crate::storage::open(app);
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

pub fn pick_images(app: &mut App) -> Task<Message> {
    if !crate::storage::enabled(app) {
        return crate::storage::open(app);
    }
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
            let provider = match crate::storage::provider(app) {
                Ok(p) => p,
                Err(e) => {
                    app.flash(e);
                    return crate::storage::open(app);
                }
            };
            let Some(ed) = &mut app.editor else { return Task::none() };
            ed.uploading += files.len();
            let title = if ed.title.trim().is_empty() { "Calendar event".to_string() } else { ed.title.trim().to_string() };
            Task::perform(
                async move {
                    let result = provider.upload(&files, &title).await.map_err(|e| e.to_string());
                    (files, result)
                },
                |(files, result)| Message::Images(ImageMsg::Uploaded { files, result }),
            )
        }
        ImageMsg::Uploaded { files, result } => {
            let count = files.len();
            let Some(ed) = &mut app.editor else {
                if result.is_ok() {
                    app.flash("Upload finished after the editor was closed — images weren't attached");
                }
                return Task::none();
            };
            ed.uploading = ed.uploading.saturating_sub(count);
            match result {
                Ok(urls) => {
                    // One URL per file, in order (see `Provider::upload`).
                    let pairs: Vec<(String, PathBuf)> = if urls.len() == files.len() {
                        urls.iter().cloned().zip(files).filter(|(u, _)| !ed.images.contains(u)).collect()
                    } else {
                        Vec::new()
                    };
                    let new: Vec<String> = urls.into_iter().filter(|u| !ed.images.contains(u)).collect();
                    ed.images.extend(new.iter().cloned());
                    if !pairs.is_empty() {
                        return thumbs_from_files(app, pairs);
                    }
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
                    if let Some(secs) = app.images.fail(&url) {
                        return Task::perform(tokio::time::sleep(std::time::Duration::from_secs(secs)), move |_| {
                            Message::Images(ImageMsg::Retry(url.clone()))
                        });
                    }
                }
            }
            Task::none()
        }
        ImageMsg::Retry(url) => {
            // Only while an editor or the viewer still shows it, and it hasn't loaded meanwhile.
            let shown =
                app.editor.as_ref().is_some_and(|e| e.images.contains(&url)) || app.viewer.as_ref().is_some_and(|v| v.urls.contains(&url));
            if shown && app.images.thumb(&url).is_none() {
                return fetch_thumb(app, url);
            }
            app.images.pending.remove(&url);
            Task::none()
        }
    }
}

pub fn rgba_handle(w: u32, h: u32, px: Vec<u8>) -> Handle {
    Handle::from_rgba(w, h, px)
}
