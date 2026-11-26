//! Event search shared by the command palette and the GNOME Shell search
//! provider: fuzzy match on title, location and notes, ranked by score and
//! then by how soon the event is.

use std::sync::{Arc, RwLock};

use chrono::{DateTime, Duration, Local};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::model::{Data, OccKey};

#[derive(Debug, Clone)]
pub struct EventHit {
    pub key: OccKey,
    /// Next occurrence, else the most recent one.
    pub date: DateTime<Local>,
    pub title: String,
    pub detail: String,
    pub color: iced::Color,
}

/// Up to `limit` events matching `query`. An empty query lists upcoming events.
pub fn search_events(data: &Data, query: &str, time_24h: bool, limit: usize) -> Vec<EventHit> {
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut buf = Vec::new();
    let q = query.trim();
    let now = Local::now();
    let pat = Pattern::parse(q, CaseMatching::Ignore, Normalization::Smart);
    let mut found: Vec<(u32, i64, EventHit)> = Vec::new();
    for ((cal_id, uid), (_, obj)) in &data.objects {
        let Some(cal) = data.calendar(cal_id) else { continue };
        let Some(master) = obj.master() else { continue };
        let (notes, _) = calen_core::notes::split(&master.description);
        let title = if master.summary.is_empty() { "(No title)".to_string() } else { master.summary.clone() };
        let score = if q.is_empty() {
            0
        } else {
            let t = pat.score(Utf32Str::new(&title, &mut buf), &mut matcher).map(|s| s * 3);
            let l = pat.score(Utf32Str::new(&master.location, &mut buf), &mut matcher);
            let n = pat.score(Utf32Str::new(&notes, &mut buf), &mut matcher).map(|s| s / 2);
            match t.max(l).max(n) {
                Some(s) => s,
                None => continue,
            }
        };
        let next = calen_core::expand(obj, now - Duration::hours(1), now + Duration::days(400)).into_iter().next();
        let (date, rid) = match next {
            Some(o) => (o.start, o.recurrence_id),
            None => (master.start.local(), None),
        };
        if q.is_empty() && date < now {
            continue; // empty query → upcoming only
        }
        let distance = (date - now).num_minutes();
        let rank = if distance >= 0 { distance } else { 1_000_000 + distance.abs() };
        found.push((
            score,
            rank,
            EventHit {
                key: OccKey { cal_id: cal_id.clone(), uid: uid.clone(), instance: rid.as_ref().map(|r| r.key()) },
                date,
                detail: detail(master, date, time_24h),
                title,
                color: crate::theme::parse_hex(&cal.color).unwrap_or(iced::Color::from_rgb8(0x4f, 0x8c, 0xff)),
            },
        ));
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    found.into_iter().take(limit).map(|(_, _, h)| h).collect()
}

/// "Fri 27 Nov 2026 · 20:00 · Room 4"
pub fn detail(ev: &calen_core::Event, date: DateTime<Local>, time_24h: bool) -> String {
    format!(
        "{}{}{}",
        date.format("%a %-d %b %Y"),
        if ev.all_day() { String::new() } else { format!(" · {}", crate::views::grid::fmt_time(date, time_24h)) },
        if ev.location.is_empty() { String::new() } else { format!(" · {}", calen_core::notes::display_location(ev)) }
    )
}

/// The latest loaded data, for searches from outside the UI thread.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))] // read by the GNOME search provider
pub struct Index {
    pub data: Arc<Data>,
    pub time_24h: bool,
}

static INDEX: RwLock<Option<Index>> = RwLock::new(None);

/// Called after every reload.
pub fn publish(data: &Arc<Data>, time_24h: bool) {
    if let Ok(mut g) = INDEX.write() {
        *g = Some(Index { data: data.clone(), time_24h });
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))] // read by the GNOME search provider
pub fn with_index<T>(f: impl FnOnce(&Index) -> T) -> Option<T> {
    INDEX.read().ok()?.as_ref().map(f)
}
