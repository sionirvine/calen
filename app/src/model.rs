//! In-memory view of the store: parsed calendar objects + expanded occurrences.

use calen_core::{CalendarObject, Event, Occurrence, When, expand};
use calen_store::{CalendarRow, ObjectRow, Store};
use chrono::{DateTime, Duration, Local, NaiveDate};
use iced::Color;
use std::collections::HashMap;

use crate::theme;

/// Identifies one displayed occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OccKey {
    pub cal_id: String,
    pub uid: String,
    /// `When::key()` of the instance's RECURRENCE-ID for recurring events.
    pub instance: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct Occ {
    pub key: OccKey,
    pub event_index: usize,
    pub recurrence_id: Option<When>,
    pub start: DateTime<Local>,
    pub end: DateTime<Local>,
    pub all_day: bool,
    pub title: String,
    pub location: String,
    pub color: Color,
    pub calendar_name: String,
    pub read_only: bool,
    pub has_meeting: bool,
    pub has_images: bool,
}

impl Occ {
    /// Last day the event touches (all-day DTEND is exclusive).
    pub fn last_day(&self) -> NaiveDate {
        if self.end <= self.start {
            return self.start.date_naive();
        }
        (self.end - Duration::seconds(1)).date_naive()
    }

    pub fn spans_days(&self) -> bool {
        self.all_day || self.last_day() != self.start.date_naive()
    }
}

pub struct Data {
    pub calendars: Vec<CalendarRow>,
    pub objects: HashMap<(String, String), (ObjectRow, CalendarObject)>,
}

impl Data {
    pub fn load(store: &Store) -> Self {
        let calendars = store.calendars().unwrap_or_default();
        let mut objects = HashMap::new();
        for row in store.objects().unwrap_or_default() {
            match CalendarObject::parse(&row.ics) {
                Ok(obj) => {
                    objects.insert((row.cal_id.clone(), row.uid.clone()), (row, obj));
                }
                Err(e) => log::warn!("skipping unparsable object {}: {e}", row.uid),
            }
        }
        Data { calendars, objects }
    }

    pub fn calendar(&self, id: &str) -> Option<&CalendarRow> {
        self.calendars.iter().find(|c| c.id == id)
    }

    pub fn calendar_color(&self, id: &str) -> Color {
        self.calendar(id).and_then(|c| theme::parse_hex(&c.color)).unwrap_or(Color::from_rgb8(0x4f, 0x8c, 0xff))
    }

    pub fn object(&self, cal_id: &str, uid: &str) -> Option<&CalendarObject> {
        self.objects.get(&(cal_id.to_string(), uid.to_string())).map(|(_, o)| o)
    }

    pub fn event(&self, key: &OccKey, event_index: usize) -> Option<&Event> {
        self.object(&key.cal_id, &key.uid)?.events.get(event_index)
    }

    pub fn writable_calendars(&self) -> Vec<&CalendarRow> {
        self.calendars.iter().filter(|c| !c.read_only).collect()
    }

    /// Expand all visible calendars over `[from, to)`, sorted by start.
    pub fn occurrences(&self, from: DateTime<Local>, to: DateTime<Local>) -> Vec<Occ> {
        let mut out = Vec::new();
        for ((cal_id, _), (_, obj)) in &self.objects {
            let Some(cal) = self.calendar(cal_id) else { continue };
            if !cal.visible {
                continue;
            }
            let cal_color = theme::parse_hex(&cal.color).unwrap_or(Color::from_rgb8(0x4f, 0x8c, 0xff));
            for o in expand(obj, from, to) {
                out.push(self.make_occ(cal, cal_color, obj, o));
            }
        }
        out.sort_by(|a, b| {
            (a.start.date_naive(), !a.all_day, a.start)
                .cmp(&(b.start.date_naive(), !b.all_day, b.start))
                .then((b.end - b.start).cmp(&(a.end - a.start)))
                .then(a.title.cmp(&b.title))
        });
        out
    }

    fn make_occ(&self, cal: &CalendarRow, cal_color: Color, obj: &CalendarObject, o: Occurrence) -> Occ {
        let ev = &obj.events[o.event_index];
        let color = ev.color.as_deref().and_then(css_color).unwrap_or(cal_color);
        let links = calen_core::links::detect(ev);
        Occ {
            key: OccKey { cal_id: cal.id.clone(), uid: o.uid.clone(), instance: o.recurrence_id.as_ref().map(|r| r.key()) },
            event_index: o.event_index,
            recurrence_id: o.recurrence_id,
            start: o.start,
            end: o.end,
            all_day: o.all_day,
            title: if ev.summary.is_empty() { "(No title)".into() } else { ev.summary.clone() },
            location: calen_core::notes::display_location(ev),
            color,
            calendar_name: cal.name.clone(),
            read_only: cal.read_only,
            has_meeting: links.meeting.is_some(),
            has_images: !calen_core::notes::split(&ev.description).1.is_empty(),
        }
    }
}

/// Accepts `#rrggbb` or a CSS3 colour name (RFC 7986 `COLOR`).
fn css_color(s: &str) -> Option<Color> {
    if s.starts_with('#') {
        return theme::parse_hex(s);
    }
    let hex = match s.to_ascii_lowercase().as_str() {
        "red" | "crimson" => "#e5484d",
        "orange" | "darkorange" => "#f76b15",
        "yellow" | "gold" => "#f5a524",
        "green" | "forestgreen" | "seagreen" => "#30a46c",
        "teal" | "darkcyan" => "#12a594",
        "blue" | "royalblue" | "dodgerblue" => "#3e63dd",
        "purple" | "rebeccapurple" | "mediumpurple" => "#8e4ec6",
        "pink" | "hotpink" | "deeppink" => "#e93d82",
        "brown" | "sienna" => "#978365",
        "gray" | "grey" | "slategray" => "#8b8d98",
        _ => return None,
    };
    theme::parse_hex(hex)
}
