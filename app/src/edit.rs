//! Applying user edits to calendar objects (incl. recurring "this / all" scope).

use calen_core::{CalendarObject, Event, When};
use chrono::{DateTime, Local};

use crate::model::OccKey;

/// Which occurrence an edit targets.
#[derive(Debug, Clone)]
pub struct Target {
    pub key: OccKey,
    pub event_index: usize,
    pub recurrence_id: Option<When>,
    pub occ_start: DateTime<Local>,
    pub occ_end: DateTime<Local>,
}

impl Target {
    pub fn is_recurring(&self) -> bool {
        self.recurrence_id.is_some()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fields {
    pub title: String,
    pub location: String,
    pub url: Option<String>,
    pub description: String,
    pub images: Vec<String>,
    pub start: When,
    pub end: When,
    pub alarms: Vec<i64>,
    /// `None` = leave recurrence unchanged.
    pub rrule: Option<Option<String>>,
    pub color: Option<String>,
    pub geo: Option<(f64, f64)>,
}

#[derive(Debug, Clone)]
pub enum Change {
    Times { start: DateTime<Local>, end: DateTime<Local>, all_day: bool },
    Fields(Box<Fields>),
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    This,
    All,
}

pub enum Outcome {
    Save(CalendarObject),
    DeleteObject,
}

fn when_like(template: &When, dt: DateTime<Local>, all_day: bool) -> When {
    if all_day {
        return When::Date(dt.date_naive());
    }
    match template {
        When::At { tzid, floating, .. } => When::At { utc: dt.with_timezone(&chrono::Utc), tzid: tzid.clone(), floating: *floating },
        When::Date(_) => When::at_local(dt),
    }
}

fn apply_fields(ev: &mut Event, f: &Fields) {
    ev.summary = f.title.clone();
    ev.location = f.location.clone();
    ev.url = f.url.clone();
    ev.description = f.description.clone();
    calen_core::notes::set_images(ev, &f.images);
    ev.alarms = f.alarms.clone();
    ev.color = f.color.clone();
    ev.geo = f.geo;
    if let Some(r) = &f.rrule {
        ev.rrule = r.clone();
    }
    sync_apple_location(ev);
}

/// Keep `X-APPLE-STRUCTURED-LOCATION` in step with LOCATION/GEO so iOS/macOS show a map.
fn sync_apple_location(ev: &mut Event) {
    use calen_core::ics::Property;
    let c = &mut ev.component;
    c.remove("X-APPLE-STRUCTURED-LOCATION");
    if let (Some((lat, lng)), false) = (ev.geo, ev.location.is_empty()) {
        let title = ev.location.replace('"', "'");
        let p = Property::new("X-APPLE-STRUCTURED-LOCATION", format!("geo:{lat:.6},{lng:.6}"))
            .with_param("VALUE", "URI")
            .with_param("X-APPLE-RADIUS", "70")
            .with_param("X-TITLE", &title);
        c.props.push(p);
    }
}

/// Apply `change` to `obj` for `target` with `scope`.
pub fn apply(mut obj: CalendarObject, target: &Target, change: &Change, scope: Scope) -> Outcome {
    let idx = target.event_index.min(obj.events.len().saturating_sub(1));
    let is_override = obj.events[idx].recurrence_id.is_some();

    if !target.is_recurring() || (scope == Scope::All && !is_override && obj.events.len() == 1 && obj.events[0].rrule.is_none()) {
        // Plain single event.
        return match change {
            Change::Delete => Outcome::DeleteObject,
            Change::Times { start, end, all_day } => {
                let ev = &mut obj.events[idx];
                ev.start = when_like(&ev.start, *start, *all_day);
                ev.end = when_like(&ev.end, *end, *all_day);
                Outcome::Save(obj)
            }
            Change::Fields(f) => {
                let ev = &mut obj.events[idx];
                apply_fields(ev, f);
                ev.start = f.start.clone();
                ev.end = f.end.clone();
                Outcome::Save(obj)
            }
        };
    }

    let rid = target.recurrence_id.clone().expect("recurring");
    match scope {
        Scope::This => {
            if let Change::Delete = change {
                // Exclude the instance and drop any override for it.
                obj.events.retain(|e| e.recurrence_id.as_ref().is_none_or(|r| r.key() != rid.key()));
                if let Some(m) = obj.master_mut() {
                    m.exdates.push(rid);
                }
                return Outcome::Save(obj);
            }
            let ov_idx = if is_override {
                idx
            } else {
                let master = obj.master().expect("master").clone();
                let mut ov = master.clone();
                ov.rrule = None;
                ov.exdates.clear();
                ov.component.remove("RRULE");
                ov.component.remove("RDATE");
                ov.component.remove("EXDATE");
                ov.recurrence_id = Some(rid.clone());
                ov.start = when_like(&master.start, target.occ_start, master.all_day());
                ov.end = when_like(&master.end, target.occ_end, master.all_day());
                obj.events.push(ov);
                obj.events.len() - 1
            };
            let ev = &mut obj.events[ov_idx];
            match change {
                Change::Times { start, end, all_day } => {
                    ev.start = when_like(&ev.start, *start, *all_day);
                    ev.end = when_like(&ev.end, *end, *all_day);
                }
                Change::Fields(f) => {
                    let mut f = (**f).clone();
                    f.rrule = None; // an override never carries a rule
                    apply_fields(ev, &f);
                    ev.start = f.start.clone();
                    ev.end = f.end.clone();
                }
                Change::Delete => unreachable!(),
            }
            Outcome::Save(obj)
        }
        Scope::All => {
            if let Change::Delete = change {
                return Outcome::DeleteObject;
            }
            let (new_start, new_end, all_day) = match change {
                Change::Times { start, end, all_day } => (*start, *end, *all_day),
                Change::Fields(f) => (f.start.local(), f.end.local(), f.start.is_date()),
                Change::Delete => unreachable!(),
            };
            let delta = new_start - target.occ_start;
            let dur = new_end - new_start;
            let Some(m) = obj.master_mut() else { return Outcome::Save(obj) };
            let shifted = m.start.local() + delta;
            m.start = when_like(&m.start, shifted, all_day);
            m.end = when_like(&m.end, shifted + dur, all_day);
            if let Change::Fields(f) = change {
                apply_fields(m, f);
            }
            if !delta.is_zero() {
                // Exceptions are anchored to old instance times; they no longer match.
                m.exdates.clear();
                obj.events.retain(|e| e.recurrence_id.is_none());
            }
            Outcome::Save(obj)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use calen_core::event::local_midnight;
    use calen_core::expand;
    use chrono::{Duration, NaiveDate};

    fn weekly() -> CalendarObject {
        CalendarObject::parse(
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:w\r\nSUMMARY:Gym\r\nDTSTART:20261102T110000Z\r\nDTEND:20261102T120000Z\r\nRRULE:FREQ=WEEKLY\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        )
        .unwrap()
    }

    fn nov() -> (DateTime<Local>, DateTime<Local>) {
        (local_midnight(NaiveDate::from_ymd_opt(2026, 11, 1).unwrap()), local_midnight(NaiveDate::from_ymd_opt(2026, 12, 1).unwrap()))
    }

    fn target(obj: &CalendarObject, n: usize) -> Target {
        let (a, b) = nov();
        let o = expand(obj, a, b)[n].clone();
        Target {
            key: OccKey { cal_id: "c".into(), uid: o.uid.clone(), instance: o.recurrence_id.as_ref().map(|r| r.key()) },
            event_index: o.event_index,
            recurrence_id: o.recurrence_id,
            occ_start: o.start,
            occ_end: o.end,
        }
    }

    #[test]
    fn move_this_creates_override() {
        let obj = weekly();
        let t = target(&obj, 1);
        let change = Change::Times { start: t.occ_start + Duration::hours(2), end: t.occ_end + Duration::hours(2), all_day: false };
        let Outcome::Save(out) = apply(obj, &t, &change, Scope::This) else { panic!() };
        assert_eq!(out.events.len(), 2);
        let (a, b) = nov();
        let occ = expand(&out, a, b);
        assert_eq!(occ.len(), 5);
        assert!(occ.iter().any(|o| o.start == t.occ_start + Duration::hours(2)));
        // round trip through ICS
        let again = CalendarObject::parse(&out.to_ics()).unwrap();
        assert_eq!(expand(&again, a, b).len(), 5);
    }

    #[test]
    fn delete_this_adds_exdate() {
        let obj = weekly();
        let t = target(&obj, 2);
        let Outcome::Save(out) = apply(obj, &t, &Change::Delete, Scope::This) else { panic!() };
        let (a, b) = nov();
        assert_eq!(expand(&out, a, b).len(), 4);
    }

    #[test]
    fn move_all_shifts_series() {
        let obj = weekly();
        let t = target(&obj, 2);
        let change = Change::Times { start: t.occ_start + Duration::days(1), end: t.occ_end + Duration::days(1), all_day: false };
        let Outcome::Save(out) = apply(obj, &t, &change, Scope::All) else { panic!() };
        let (a, b) = nov();
        let occ = expand(&out, a, b);
        assert!(occ.iter().all(|o| o.start.weekday_num() == 1));
    }

    trait Wd {
        fn weekday_num(&self) -> u32;
    }
    impl Wd for DateTime<Local> {
        fn weekday_num(&self) -> u32 {
            use chrono::Datelike;
            self.with_timezone(&chrono::Utc).weekday().num_days_from_monday()
        }
    }
}
