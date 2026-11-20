//! Shared bits for the canvas-based calendar grids.

use chrono::{DateTime, Local, NaiveDate};
use iced::widget::canvas::{Frame, Path, Text};
use iced::{Color, Font, Pixels, Point, Rectangle, Size, alignment, font};

use crate::model::{Occ, OccKey};

/// What the grids report back to the app.
#[derive(Debug, Clone)]
pub enum GridMsg {
    Select(Option<OccKey>),
    Open(OccKey),
    /// Move and/or resize an occurrence.
    Change {
        key: OccKey,
        start: DateTime<Local>,
        end: DateTime<Local>,
    },
    /// Drag-created (or double-clicked) time range.
    Create {
        start: DateTime<Local>,
        end: DateTime<Local>,
        all_day: bool,
    },
    SelectDate(NaiveDate),
    /// Right-click (or two-finger tap) on an event: open its menu.
    Context(OccKey),
    /// Double-click on a day (month view): start quick add on that day.
    NewOn(NaiveDate),
}

pub const BOLD: Font = Font { weight: font::Weight::Semibold, ..Font::DEFAULT };

pub fn text(frame: &mut Frame, content: &str, pos: Point, size: f32, color: Color, font: Font, max_w: f32) {
    let s = ellipsize(content, max_w, size);
    if s.is_empty() {
        return;
    }
    frame.fill_text(Text {
        content: s,
        position: pos,
        color,
        size: Pixels(size),
        font,
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });
}

pub fn text_centered(frame: &mut Frame, content: &str, center: Point, size: f32, color: Color, font: Font) {
    frame.fill_text(Text {
        content: content.to_string(),
        position: center,
        color,
        size: Pixels(size),
        font,
        align_x: iced::advanced::text::Alignment::Center,
        align_y: alignment::Vertical::Center,
        ..Text::default()
    });
}

pub fn text_right(frame: &mut Frame, content: &str, right_top: Point, size: f32, color: Color, font: Font) {
    frame.fill_text(Text {
        content: content.to_string(),
        position: right_top,
        color,
        size: Pixels(size),
        font,
        align_x: iced::advanced::text::Alignment::Right,
        align_y: alignment::Vertical::Center,
        ..Text::default()
    });
}

/// Rough width-based truncation (canvas text has no ellipsis support).
pub fn ellipsize(s: &str, max_w: f32, size: f32) -> String {
    if max_w <= size * 0.6 {
        return String::new();
    }
    let avg = size * 0.56;
    let max_chars = (max_w / avg).floor() as usize;
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    if max_chars <= 1 {
        return String::new();
    }
    let mut out: String = s.chars().take(max_chars - 1).collect();
    out = out.trim_end().to_string();
    out.push('…');
    out
}

pub fn rounded(frame: &mut Frame, r: Rectangle, radius: f32, color: Color) {
    let path = Path::rounded_rectangle(r.position(), r.size(), radius.into());
    frame.fill(&path, color);
}

pub fn rect(frame: &mut Frame, r: Rectangle, color: Color) {
    frame.fill_rectangle(r.position(), r.size(), color);
}

pub fn hline(frame: &mut Frame, y: f32, x0: f32, x1: f32, color: Color) {
    frame.fill_rectangle(Point::new(x0, y.floor()), Size::new(x1 - x0, 1.0), color);
}

pub fn vline(frame: &mut Frame, x: f32, y0: f32, y1: f32, color: Color) {
    frame.fill_rectangle(Point::new(x.floor(), y0), Size::new(1.0, y1 - y0), color);
}

pub fn fmt_time(t: DateTime<Local>, h24: bool) -> String {
    if h24 {
        t.format("%H:%M").to_string()
    } else if t.format("%M").to_string() == "00" {
        t.format("%-I%P").to_string()
    } else {
        t.format("%-I:%M%P").to_string()
    }
}

/// Assign lanes to multi-day bars inside `[days[0], days[last]]`.
/// Returns (occ index, first col, last col, lane).
pub fn bar_lanes(occs: &[&Occ], first: NaiveDate, ncols: usize) -> Vec<(usize, usize, usize, usize)> {
    let last = first + chrono::Duration::days(ncols as i64 - 1);
    let mut items: Vec<(usize, usize, usize)> = occs
        .iter()
        .enumerate()
        .filter_map(|(i, o)| {
            let s = o.start.date_naive();
            let e = o.last_day();
            if e < first || s > last {
                return None;
            }
            let c0 = (s.max(first) - first).num_days() as usize;
            let c1 = (e.min(last) - first).num_days() as usize;
            Some((i, c0, c1))
        })
        .collect();
    items.sort_by_key(|&(i, c0, c1)| (c0, std::cmp::Reverse(c1 - c0), occs[i].start));
    let mut lanes: Vec<Vec<bool>> = Vec::new();
    let mut out = Vec::new();
    for (i, c0, c1) in items {
        let lane = (0..).find(|&l| lanes.get(l).is_none_or(|row: &Vec<bool>| (c0..=c1).all(|c| !row[c]))).unwrap();
        if lanes.len() <= lane {
            lanes.resize(lane + 1, vec![false; ncols]);
        }
        lanes[lane][c0..=c1].fill(true);
        out.push((i, c0, c1, lane));
    }
    out
}

/// A canvas geometry cache that is cleared whenever its inputs' key changes.
/// Lets the grids keep their drawing across repaints that don't touch them
/// (hovering a button elsewhere repaints the window).
#[derive(Default)]
pub struct KeyedCache {
    cache: iced::widget::canvas::Cache,
    key: std::cell::Cell<u64>,
}

impl KeyedCache {
    pub fn with_key(&self, key: u64) -> &iced::widget::canvas::Cache {
        if self.key.replace(key) != key {
            self.cache.clear();
        }
        &self.cache
    }
}

#[derive(Default)]
pub struct GridCaches {
    pub week: KeyedCache,
    pub header: KeyedCache,
    pub month: KeyedCache,
}

/// Hash of what the grids draw from a set of occurrences.
pub fn occs_key(h: &mut impl std::hash::Hasher, occs: &[&Occ]) {
    use std::hash::Hash;
    for o in occs {
        o.key.hash(h);
        o.start.timestamp().hash(h);
        o.end.timestamp().hash(h);
        o.all_day.hash(h);
        o.title.hash(h);
        o.read_only.hash(h);
        o.color.r.to_bits().hash(h);
        o.color.g.to_bits().hash(h);
        o.color.b.to_bits().hash(h);
    }
}
