//! Calen's image block inside DESCRIPTION.
//!
//! Images are stored as plain URLs under a marker line so every other client
//! still shows clickable links, while Calen renders them as thumbnails:
//!
//! ```text
//! Bring the printed invoice.
//!
//! 📷 Images (calen)
//! https://img.felixadr.com/i/abc.jpg
//! https://img.felixadr.com/i/def.png
//! ```

use crate::event::Event;
use crate::links;

pub const MARKER: &str = "📷 Images (calen)";

/// Split a description into (user notes, image URLs).
pub fn split(description: &str) -> (String, Vec<String>) {
    let mut notes: Vec<&str> = Vec::new();
    let mut images = Vec::new();
    let mut in_block = false;
    for line in description.lines() {
        let t = line.trim();
        if t == MARKER {
            in_block = true;
            continue;
        }
        if in_block {
            if t.starts_with("http://") || t.starts_with("https://") {
                images.push(t.to_string());
                continue;
            }
            if t.is_empty() {
                continue;
            }
            in_block = false;
        }
        notes.push(line);
    }
    let text = notes.join("\n").trim_end().to_string();
    (text, images)
}

/// Inverse of [`split`].
pub fn join(notes: &str, images: &[String]) -> String {
    let notes = notes.trim_end();
    if images.is_empty() {
        return notes.to_string();
    }
    let mut out = String::new();
    if !notes.is_empty() {
        out.push_str(notes);
        out.push_str("\n\n");
    }
    out.push_str(MARKER);
    for url in images {
        out.push('\n');
        out.push_str(url);
    }
    out
}

/// Set the image list on an event: description block + ATTACH properties.
pub fn set_images(ev: &mut Event, images: &[String]) {
    let (notes, old) = split(&ev.description);
    ev.description = join(&notes, images);
    ev.attachments.retain(|a| !old.contains(a) && !images.contains(a));
    ev.attachments.extend(images.iter().cloned());
}

/// What to show as the location: a place name instead of a raw maps URL.
pub fn display_location(ev: &Event) -> String {
    let loc = ev.location.trim();
    if loc.starts_with("http://") || loc.starts_with("https://") {
        if let Some(m) = links::find_maps(loc) {
            return m.place.unwrap_or_else(|| "Map location".into());
        }
        if links::find_meeting(loc).is_some() {
            return "Online".into();
        }
    }
    loc.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let imgs = vec!["https://img.felixadr.com/a.jpg".to_string(), "https://img.felixadr.com/b.png".to_string()];
        let d = join("Bring invoice.\nAnd pen.", &imgs);
        let (n, i) = split(&d);
        assert_eq!(n, "Bring invoice.\nAnd pen.");
        assert_eq!(i, imgs);
        assert_eq!(join("", &imgs), format!("{MARKER}\nhttps://img.felixadr.com/a.jpg\nhttps://img.felixadr.com/b.png"));
        assert_eq!(split("plain").1.len(), 0);
    }

    #[test]
    fn text_after_block_is_kept() {
        let (n, i) = split(&format!("a\n\n{MARKER}\nhttps://x/1.jpg\n\nadded by phone"));
        assert_eq!(i, vec!["https://x/1.jpg"]);
        assert_eq!(n, "a\n\nadded by phone");
    }
}
