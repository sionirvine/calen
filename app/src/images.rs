//! In-memory handles for event image thumbnails (downloaded + disk cached by
//! `calen-images`).

use iced::widget::image::Handle;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct ImageCache {
    handles: HashMap<String, Handle>,
    pub pending: HashSet<String>,
    pub failed: HashSet<String>,
}

impl ImageCache {
    pub fn thumb(&self, url: &str) -> Option<&Handle> {
        self.handles.get(url)
    }

    pub fn insert_rgba(&mut self, url: String, w: u32, h: u32, px: Vec<u8>) {
        self.pending.remove(&url);
        self.handles.insert(url, crate::image_tasks::rgba_handle(w, h, px));
    }

    pub fn fail(&mut self, url: &str) {
        self.pending.remove(url);
        self.failed.insert(url.to_string());
    }

    /// URLs that still need loading.
    pub fn wanted<'a>(&self, urls: impl IntoIterator<Item = &'a String>) -> Vec<String> {
        urls.into_iter()
            .filter(|u| !self.handles.contains_key(*u) && !self.pending.contains(*u) && !self.failed.contains(*u))
            .cloned()
            .collect()
    }
}
