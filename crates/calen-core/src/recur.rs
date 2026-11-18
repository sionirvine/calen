//! Expand events (including RRULE + overrides) into concrete occurrences.

use chrono::{DateTime, Duration, Local};
use std::collections::HashMap;

use crate::event::{CalendarObject, Event, When};

#[derive(Debug, Clone, PartialEq)]
pub struct Occurrence {
    pub uid: String,
    /// Index into `CalendarObject::events` of the event this instance came from.
    pub event_index: usize,
    /// Set for instances of a recurring series (original start of the instance).
    pub recurrence_id: Option<When>,
    pub start: DateTime<Local>,
    pub end: DateTime<Local>,
    pub all_day: bool,
}

impl Occurrence {
    pub fn is_recurring(&self) -> bool {
        self.recurrence_id.is_some()
    }
}

const MAX_INSTANCES: u16 = 1000;

/// All occurrences of `obj` overlapping `[from, to)`.
pub fn expand(obj: &CalendarObject, from: DateTime<Local>, to: DateTime<Local>) -> Vec<Occurrence> {
    let mut out = Vec::new();
    let Some(master_idx) = obj.events.iter().position(|e| e.recurrence_id.is_none()) else {
        // Orphan overrides only (we were invited to a single instance).
        for (i, e) in obj.events.iter().enumerate() {
            push_if_overlaps(&mut out, e, i, e.recurrence_id.clone(), from, to);
        }
        return out;
    };
    let master = &obj.events[master_idx];

    let overrides: HashMap<i64, usize> =
        obj.events.iter().enumerate().filter_map(|(i, e)| e.recurrence_id.as_ref().map(|r| (r.key(), i))).collect();

    let Some(rule) = master.rrule.as_deref().filter(|r| !r.is_empty()) else {
        push_if_overlaps(&mut out, master, master_idx, None, from, to);
        return out;
    };

    let dur = master.duration();
    for start in instances(master, rule, from - dur - Duration::days(1), to) {
        let rid = match &master.start {
            When::Date(_) => When::Date(start.date_naive()),
            When::At { tzid, floating, .. } => When::At { utc: start.with_timezone(&chrono::Utc), tzid: tzid.clone(), floating: *floating },
        };
        if overrides.contains_key(&rid.key()) {
            continue;
        }
        let end = match &master.start {
            When::Date(_) => {
                let days = (master.end.date() - master.start.date()).num_days().max(1);
                crate::event::local_midnight(start.date_naive() + Duration::days(days))
            }
            _ => start + dur,
        };
        if overlaps(start, end, from, to) {
            out.push(Occurrence {
                uid: master.uid.clone(),
                event_index: master_idx,
                recurrence_id: Some(rid),
                start,
                end,
                all_day: master.all_day(),
            });
        }
    }
    for &i in overrides.values() {
        let e = &obj.events[i];
        if e.component.prop("STATUS").is_some_and(|p| p.value.eq_ignore_ascii_case("CANCELLED")) {
            continue;
        }
        push_if_overlaps(&mut out, e, i, e.recurrence_id.clone(), from, to);
    }
    out.sort_by_key(|o| o.start);
    out
}

fn instances(master: &Event, rule: &str, from: DateTime<Local>, to: DateTime<Local>) -> Vec<DateTime<Local>> {
    let dtstart = master.start.to_rrule_dt();
    let Ok(parsed) = rule.parse::<rrule::RRule<rrule::Unvalidated>>() else {
        return vec![master.start.local()];
    };
    let Ok(valid) = parsed.validate(dtstart) else {
        return vec![master.start.local()];
    };
    let mut set = rrule::RRuleSet::new(dtstart).rrule(valid);
    for ex in &master.exdates {
        let ex_dt = match ex {
            // Date EXDATEs on timed series: exclude the instance on that day at the series time.
            When::Date(d) if !master.start.is_date() => {
                let t = master.start.to_rrule_dt();
                let naive = d.and_time(t.time());
                match chrono::TimeZone::from_local_datetime(&t.timezone(), &naive).earliest() {
                    Some(v) => v,
                    None => continue,
                }
            }
            other => other.to_rrule_dt(),
        };
        set = set.exdate(ex_dt);
    }
    let from_tz = from.with_timezone(&rrule::Tz::LOCAL);
    let to_tz = to.with_timezone(&rrule::Tz::LOCAL);
    set.after(from_tz).before(to_tz).all(MAX_INSTANCES).dates.into_iter().map(|d| d.with_timezone(&Local)).collect()
}

fn push_if_overlaps(out: &mut Vec<Occurrence>, e: &Event, idx: usize, rid: Option<When>, from: DateTime<Local>, to: DateTime<Local>) {
    let (start, end) = (e.start.local(), e.end.local());
    if overlaps(start, end, from, to) {
        out.push(Occurrence { uid: e.uid.clone(), event_index: idx, recurrence_id: rid, start, end, all_day: e.all_day() });
    }
}

fn overlaps(start: DateTime<Local>, end: DateTime<Local>, from: DateTime<Local>, to: DateTime<Local>) -> bool {
    let end = if end <= start { start + Duration::minutes(1) } else { end };
    start < to && end > from
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::local_midnight;
    use chrono::NaiveDate;

    fn day(y: i32, m: u32, d: u32) -> DateTime<Local> {
        local_midnight(NaiveDate::from_ymd_opt(y, m, d).unwrap())
    }

    #[test]
    fn weekly_with_exdate_and_override() {
        let src = "BEGIN:VCALENDAR\r\n\
BEGIN:VEVENT\r\nUID:w\r\nSUMMARY:Connect Group\r\nDTSTART;TZID=Asia/Jakarta:20261103T190000\r\nDTEND;TZID=Asia/Jakarta:20261103T210000\r\nRRULE:FREQ=WEEKLY;BYDAY=TU\r\nEXDATE;TZID=Asia/Jakarta:20261110T190000\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:w\r\nSUMMARY:Connect Group (moved)\r\nRECURRENCE-ID;TZID=Asia/Jakarta:20261117T190000\r\nDTSTART;TZID=Asia/Jakarta:20261118T190000\r\nDTEND;TZID=Asia/Jakarta:20261118T210000\r\nEND:VEVENT\r\n\
END:VCALENDAR\r\n";
        let obj = CalendarObject::parse(src).unwrap();
        let occ = expand(&obj, day(2026, 11, 1), day(2026, 12, 1));
        // Nov 3, (10 excluded), 17 overridden -> 18, 24
        assert_eq!(occ.len(), 3, "{occ:#?}");
        assert!(occ.iter().any(|o| o.event_index == 1));
    }

    #[test]
    fn all_day_yearly() {
        let src = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:b\r\nSUMMARY:Birthday\r\nDTSTART;VALUE=DATE:20000305\r\nDTEND;VALUE=DATE:20000306\r\nRRULE:FREQ=YEARLY\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let obj = CalendarObject::parse(src).unwrap();
        let occ = expand(&obj, day(2026, 1, 1), day(2027, 1, 1));
        assert_eq!(occ.len(), 1);
        assert_eq!(occ[0].start.date_naive(), NaiveDate::from_ymd_opt(2026, 3, 5).unwrap());
        assert!(occ[0].all_day);
    }

    #[test]
    fn single_event_overlap() {
        let src = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:s\r\nDTSTART:20261120T030000Z\r\nDTEND:20261120T040000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let obj = CalendarObject::parse(src).unwrap();
        assert_eq!(expand(&obj, day(2026, 11, 1), day(2026, 12, 1)).len(), 1);
        assert_eq!(expand(&obj, day(2026, 12, 1), day(2027, 1, 1)).len(), 0);
    }
}
