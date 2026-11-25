//! In-memory handles for event image thumbnails (downloaded + disk cached by
//! `calen-images`).

use iced::widget::image::Handle;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct ImageCache {
    handles: HashMap<String, Handle>,
    pub pending: HashSet<String>,
    pub failed: HashSet<String>,
    /// Downloads tried so far for thumbnails still loading.
    attempts: HashMap<String, u32>,
}

/// Waits (seconds) before each retry of a failed thumbnail download.
pub const RETRY_DELAYS: [u64; 3] = [1, 3, 9];

impl ImageCache {
    /// Forget decoded thumbnails (they're reloaded from the disk cache on demand).
    pub fn trim(&mut self) {
        self.handles = HashMap::new();
        self.failed.clear();
    }

    pub fn thumb(&self, url: &str) -> Option<&Handle> {
        self.handles.get(url)
    }

    pub fn insert_rgba(&mut self, url: String, w: u32, h: u32, px: Vec<u8>) {
        self.pending.remove(&url);
        self.failed.remove(&url);
        self.attempts.remove(&url);
        self.handles.insert(url, crate::image_tasks::rgba_handle(w, h, px));
    }

    /// A download failed: the seconds to wait before trying again, or None
    /// after the last retry, when it's marked failed.
    pub fn fail(&mut self, url: &str) -> Option<u64> {
        let n = self.attempts.entry(url.to_string()).or_insert(0);
        let delay = RETRY_DELAYS.get(*n as usize).copied();
        *n += 1;
        if delay.is_none() {
            self.pending.remove(url);
            self.attempts.remove(url);
            self.failed.insert(url.to_string());
        }
        delay
    }

    /// Give these another chance (the editor showing them was opened again).
    pub fn forget_failures<'a>(&mut self, urls: impl IntoIterator<Item = &'a String>) {
        for u in urls {
            self.failed.remove(u);
            if !self.pending.contains(u) {
                self.attempts.remove(u);
            }
        }
    }

    /// URLs that still need loading.
    pub fn wanted<'a>(&self, urls: impl IntoIterator<Item = &'a String>) -> Vec<String> {
        urls.into_iter()
            .filter(|u| !self.handles.contains_key(*u) && !self.pending.contains(*u) && !self.failed.contains(*u))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_then_fails() {
        let mut c = ImageCache::default();
        let u = "https://example.invalid/a.png".to_string();
        c.pending.insert(u.clone());
        assert_eq!(c.fail(&u), Some(1));
        assert_eq!(c.fail(&u), Some(3));
        assert_eq!(c.fail(&u), Some(9));
        assert!(c.wanted([&u]).is_empty(), "still pending while retrying");
        assert_eq!(c.fail(&u), None);
        assert!(c.failed.contains(&u) && !c.pending.contains(&u));
        c.forget_failures([&u]);
        assert_eq!(c.wanted([&u]), vec![u.clone()]);
        assert_eq!(c.fail(&u), Some(1), "a reopened editor starts the retries over");
    }
}
