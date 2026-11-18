//! `--seed`: fill the local calendar with sample events (development aid).

use calen_core::{CalendarObject, Event, When};
use calen_store::{LOCAL_CALENDAR, Store};
use chrono::{Duration, Local, NaiveTime};

pub fn seed(store: &Store) {
    let today = Local::now().date_naive();
    let at = |d: i64, h: u32, m: u32| {
        let t = NaiveTime::from_hms_opt(h, m, 0).unwrap();
        When::at_local(calen_core::event::local_midnight(today + Duration::days(d)) + (t - NaiveTime::MIN))
    };
    let mut items: Vec<Event> = Vec::new();
    let mut e = Event::new("Project planning", at(0, 13, 0), at(0, 14, 0));
    e.location = "Google Meet".into();
    e.url = Some("https://meet.google.com/abc-defg-hij".into());
    items.push(e);
    let mut e = Event::new("Catch up over lunch", at(1, 12, 0), at(1, 13, 30));
    e.location = "DiBella's Old Fashioned Subs".into();
    e.url = Some("https://www.google.com/maps/place/DiBella's+Old+Fashioned+Subs/@43.08,-77.67,17z".into());
    e.geo = Some((43.08, -77.67));
    items.push(e);
    let mut e = Event::new("Connect Group", at(-6, 19, 0), at(-6, 21, 0));
    e.rrule = Some("FREQ=WEEKLY".into());
    items.push(e);
    items.push(Event::new("Check email", at(-1, 8, 0), at(-1, 8, 45)));
    items.push(Event::new("Band practice", at(2, 19, 0), at(2, 20, 0)));
    items.push(Event::new("Budget meeting", at(2, 10, 0), at(2, 11, 0)));
    items.push(Event::new("Design review", at(2, 10, 30), at(2, 12, 0)));
    let mut e = Event::new("Web Design Meeting", at(3, 11, 0), at(3, 12, 0));
    e.description = "Zoom: https://us02web.zoom.us/j/8812345678".into();
    items.push(e);
    items.push(Event::new("Lapor PLN Kutisari", When::Date(today + Duration::days(4)), When::Date(today + Duration::days(8))));
    items.push(Event::new("Grocery delivery", When::Date(today + Duration::days(1)), When::Date(today + Duration::days(2))));
    for ev in items {
        let obj = CalendarObject::new(ev);
        let _ = store.save_local(LOCAL_CALENDAR, obj.uid(), &obj.to_ics());
    }
}
