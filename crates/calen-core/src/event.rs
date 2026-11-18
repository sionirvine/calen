//! Calendar event model on top of the lossless ICS tree.

use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

use crate::ics::{self, Component, Property};

pub const PRODID: &str = "-//Calen//Calen Desktop//EN";

/// A DTSTART/DTEND style value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum When {
    /// All-day date (`VALUE=DATE`).
    Date(NaiveDate),
    /// A concrete instant. `tzid` is kept so recurrence expansion is DST correct
    /// and so we write the value back the way we found it. `None` = UTC.
    /// `floating` = no zone at all in the source (interpreted as local time).
    At { utc: DateTime<Utc>, tzid: Option<String>, floating: bool },
}

impl When {
    pub fn at_local(dt: DateTime<Local>) -> Self {
        When::At { utc: dt.with_timezone(&Utc), tzid: local_tzid(), floating: false }
    }

    pub fn is_date(&self) -> bool {
        matches!(self, When::Date(_))
    }

    /// Start of this value in local time (dates → local midnight).
    pub fn local(&self) -> DateTime<Local> {
        match self {
            When::Date(d) => local_midnight(*d),
            When::At { utc, .. } => utc.with_timezone(&Local),
        }
    }

    pub fn utc(&self) -> DateTime<Utc> {
        self.local().with_timezone(&Utc)
    }

    pub fn date(&self) -> NaiveDate {
        match self {
            When::Date(d) => *d,
            When::At { .. } => self.local().date_naive(),
        }
    }

    /// Shift by a duration, keeping the kind (date vs instant) and zone.
    pub fn shifted(&self, by: Duration) -> Self {
        match self {
            When::Date(d) => When::Date(*d + Duration::days(by.num_days())),
            When::At { utc, tzid, floating } => When::At { utc: *utc + by, tzid: tzid.clone(), floating: *floating },
        }
    }

    pub fn parse(prop: &Property) -> Option<Self> {
        let v = prop.value.trim();
        // EXDATE etc. may hold lists; callers split first.
        if prop.param("VALUE").is_some_and(|x| x.eq_ignore_ascii_case("DATE")) || v.len() == 8 {
            return NaiveDate::parse_from_str(v, "%Y%m%d").ok().map(When::Date);
        }
        Self::parse_datetime(v, prop.param("TZID"))
    }

    fn parse_datetime(v: &str, tzid: Option<&str>) -> Option<Self> {
        if let Some(stripped) = v.strip_suffix('Z') {
            let naive = NaiveDateTime::parse_from_str(stripped, "%Y%m%dT%H%M%S").ok()?;
            return Some(When::At { utc: Utc.from_utc_datetime(&naive), tzid: None, floating: false });
        }
        let naive = NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S").ok()?;
        if let Some(tz) = tzid.and_then(parse_tz) {
            let dt = tz.from_local_datetime(&naive).earliest()?;
            return Some(When::At { utc: dt.with_timezone(&Utc), tzid: Some(tz.name().to_string()), floating: false });
        }
        // Floating or unknown (e.g. Windows) zone: interpret as local time.
        let dt = Local.from_local_datetime(&naive).earliest()?;
        Some(When::At { utc: dt.with_timezone(&Utc), tzid: None, floating: tzid.is_none() })
    }

    pub fn to_property(&self, name: &str) -> Property {
        match self {
            When::Date(d) => Property::new(name, d.format("%Y%m%d").to_string()).with_param("VALUE", "DATE"),
            When::At { utc, tzid, floating } => {
                if *floating {
                    let local = utc.with_timezone(&Local).naive_local();
                    Property::new(name, local.format("%Y%m%dT%H%M%S").to_string())
                } else if let Some(tz) = tzid.as_deref().and_then(parse_tz) {
                    let local = utc.with_timezone(&tz).naive_local();
                    Property::new(name, local.format("%Y%m%dT%H%M%S").to_string()).with_param("TZID", tz.name())
                } else {
                    Property::new(name, utc.format("%Y%m%dT%H%M%SZ").to_string())
                }
            }
        }
    }

    /// The value as an `rrule` crate datetime (keeps the zone for DST-correct rules).
    pub fn to_rrule_dt(&self) -> DateTime<rrule::Tz> {
        match self {
            When::Date(d) => rrule::Tz::LOCAL
                .from_local_datetime(&d.and_time(NaiveTime::MIN))
                .earliest()
                .unwrap_or_else(|| rrule::Tz::LOCAL.from_utc_datetime(&d.and_time(NaiveTime::MIN))),
            When::At { utc, tzid, floating } => {
                if let Some(tz) = tzid.as_deref().and_then(parse_tz) {
                    utc.with_timezone(&rrule::Tz::Tz(tz))
                } else if *floating {
                    utc.with_timezone(&rrule::Tz::LOCAL)
                } else {
                    utc.with_timezone(&rrule::Tz::UTC)
                }
            }
        }
    }

    /// Identity used to match RECURRENCE-ID / EXDATE against expanded instances.
    pub fn key(&self) -> i64 {
        match self {
            When::Date(d) => local_midnight(*d).timestamp(),
            When::At { utc, .. } => utc.timestamp(),
        }
    }
}

pub fn parse_tz(name: &str) -> Option<Tz> {
    name.trim_start_matches('/').parse::<Tz>().ok()
}

pub fn local_tzid() -> Option<String> {
    iana_time_zone::get_timezone().ok().filter(|n| parse_tz(n).is_some())
}

pub fn local_midnight(d: NaiveDate) -> DateTime<Local> {
    let naive = d.and_time(NaiveTime::MIN);
    Local.from_local_datetime(&naive).earliest().unwrap_or_else(|| Local.from_utc_datetime(&naive))
}

/// One VEVENT (a master event or a recurrence override).
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub uid: String,
    pub summary: String,
    pub location: String,
    /// Full DESCRIPTION, including Calen's image block (see `notes`).
    pub description: String,
    pub url: Option<String>,
    pub start: When,
    pub end: When,
    pub rrule: Option<String>,
    pub exdates: Vec<When>,
    pub recurrence_id: Option<When>,
    /// Alarm offsets in minutes before start.
    pub alarms: Vec<i64>,
    pub color: Option<String>,
    pub geo: Option<(f64, f64)>,
    pub attachments: Vec<String>,
    /// Original component, so unknown data survives edits.
    pub component: Component,
}

impl Event {
    pub fn new(summary: &str, start: When, end: When) -> Self {
        let uid = format!("{}@calen", uuid::Uuid::new_v4());
        Event {
            uid,
            summary: summary.to_string(),
            location: String::new(),
            description: String::new(),
            url: None,
            start,
            end,
            rrule: None,
            exdates: Vec::new(),
            recurrence_id: None,
            alarms: Vec::new(),
            color: None,
            geo: None,
            attachments: Vec::new(),
            component: Component::new("VEVENT"),
        }
    }

    pub fn all_day(&self) -> bool {
        self.start.is_date()
    }

    pub fn duration(&self) -> Duration {
        let d = self.end.utc() - self.start.utc();
        if d < Duration::zero() { Duration::zero() } else { d }
    }

    pub fn from_component(c: &Component) -> Option<Self> {
        let uid = c.text("UID").unwrap_or_default();
        let start = When::parse(c.prop("DTSTART")?)?;
        let end = match c.prop("DTEND").and_then(When::parse) {
            Some(e) => e,
            None => match c.prop("DURATION").and_then(|p| parse_duration(&p.value)) {
                Some(d) => start.shifted(d),
                None if start.is_date() => start.shifted(Duration::days(1)),
                None => start.clone(),
            },
        };
        let exdates = c
            .props_named("EXDATE")
            .flat_map(|p| {
                p.value
                    .split(',')
                    .filter_map(|v| {
                        let single = Property { name: p.name.clone(), params: p.params.clone(), value: v.to_string() };
                        When::parse(&single)
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        let alarms = c
            .children_named("VALARM")
            .filter_map(|a| {
                let t = a.prop("TRIGGER")?;
                if t.param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE-TIME")) {
                    let at = When::parse(&Property::new("X", t.value.clone()))?;
                    return Some((start.utc() - at.utc()).num_minutes());
                }
                let d = parse_duration(&t.value)?;
                Some(-d.num_minutes())
            })
            .collect();
        let geo = c.prop("GEO").and_then(|p| {
            let (a, b) = p.value.split_once(';')?;
            Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
        });
        Some(Event {
            uid,
            summary: c.text("SUMMARY").unwrap_or_default(),
            location: c.text("LOCATION").unwrap_or_default(),
            description: c.text("DESCRIPTION").unwrap_or_default(),
            url: c.prop("URL").map(|p| p.value.clone()),
            start,
            end,
            rrule: c.prop("RRULE").map(|p| p.value.clone()),
            exdates,
            recurrence_id: c.prop("RECURRENCE-ID").and_then(When::parse),
            alarms,
            color: c.prop("COLOR").map(|p| p.value.clone()),
            geo,
            attachments: c.props_named("ATTACH").map(|p| p.value.clone()).collect(),
            component: c.clone(),
        })
    }

    /// Serialize, starting from the original component so unknown props survive.
    pub fn to_component(&self) -> Component {
        let mut c = self.component.clone();
        c.name = "VEVENT".into();
        c.set(Property::new("UID", self.uid.clone()));
        c.set(Property::new("DTSTAMP", Utc::now().format("%Y%m%dT%H%M%SZ").to_string()));
        c.set(Property::new("LAST-MODIFIED", Utc::now().format("%Y%m%dT%H%M%SZ").to_string()));
        let seq = c.prop("SEQUENCE").and_then(|p| p.value.parse::<i64>().ok()).unwrap_or(-1) + 1;
        c.set(Property::new("SEQUENCE", seq.to_string()));
        c.set_text("SUMMARY", &self.summary);
        c.set_text("LOCATION", &self.location);
        c.set_text("DESCRIPTION", &self.description);
        match &self.url {
            Some(u) if !u.is_empty() => c.set(Property::new("URL", u.clone())),
            _ => c.remove("URL"),
        }
        c.set(self.start.to_property("DTSTART"));
        c.remove("DURATION");
        c.set(self.end.to_property("DTEND"));
        match &self.rrule {
            Some(r) if !r.is_empty() => c.set(Property::new("RRULE", r.clone())),
            _ => c.remove("RRULE"),
        }
        c.remove("EXDATE");
        for ex in &self.exdates {
            c.props.push(ex.to_property("EXDATE"));
        }
        match &self.recurrence_id {
            Some(r) => c.set(r.to_property("RECURRENCE-ID")),
            None => c.remove("RECURRENCE-ID"),
        }
        match &self.color {
            Some(col) => c.set(Property::new("COLOR", col.clone())),
            None => c.remove("COLOR"),
        }
        match self.geo {
            Some((lat, lng)) => c.set(Property::new("GEO", format!("{lat:.6};{lng:.6}"))),
            None => c.remove("GEO"),
        }
        // Attachments: rewrite URI attachments, keep inline binary ones.
        c.props.retain(|p| !(p.name == "ATTACH" && p.param("ENCODING").is_none()));
        for a in &self.attachments {
            let mut p = Property::new("ATTACH", a.clone());
            if let Some(mime) = mime_from_url(a) {
                p = p.with_param("FMTTYPE", mime);
            }
            c.props.push(p);
        }
        // Alarms: regenerate display alarms if they changed.
        let existing: Vec<i64> = Event::from_component(&self.component).map(|e| e.alarms).unwrap_or_default();
        if existing != self.alarms {
            c.children.retain(|ch| !ch.name.eq_ignore_ascii_case("VALARM"));
            for m in &self.alarms {
                let mut a = Component::new("VALARM");
                a.props.push(Property::new("ACTION", "DISPLAY"));
                a.props.push(Property::new("DESCRIPTION", ics::escape_text(&self.summary)));
                a.props.push(Property::new("TRIGGER", format_duration(-*m)));
                c.children.push(a);
            }
        }
        c
    }

    /// Move to a new start, preserving duration.
    pub fn move_to(&mut self, start: When) {
        let dur = self.end.utc() - self.start.utc();
        match (&start, &self.end) {
            (When::Date(_), _) => {
                let days = (self.end.date() - self.start.date()).num_days().max(1);
                self.end = start.shifted(Duration::days(days));
            }
            _ => self.end = start.shifted(dur),
        }
        self.start = start;
    }
}

fn mime_from_url(u: &str) -> Option<&'static str> {
    let lower = u.to_ascii_lowercase();
    let path = lower.split(['?', '#']).next().unwrap_or("");
    Some(match path.rsplit('.').next()? {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "avif" => "image/avif",
        _ => return None,
    })
}

/// Parse an ISO-8601 / RFC 5545 duration like `-PT15M`, `P1D`, `PT1H30M`, `P1W`.
pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    let (neg, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let rest = rest.strip_prefix('P')?;
    let mut total = Duration::zero();
    let mut num = String::new();
    let mut in_time = false;
    for ch in rest.chars() {
        match ch {
            'T' => in_time = true,
            '0'..='9' => num.push(ch),
            unit => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                total += match (unit, in_time) {
                    ('W', _) => Duration::weeks(n),
                    ('D', _) => Duration::days(n),
                    ('H', true) => Duration::hours(n),
                    ('M', true) => Duration::minutes(n),
                    ('S', true) => Duration::seconds(n),
                    _ => return None,
                };
            }
        }
    }
    Some(if neg { -total } else { total })
}

pub fn format_duration(minutes: i64) -> String {
    let neg = minutes < 0;
    let m = minutes.abs();
    let body = if m == 0 {
        "PT0S".to_string()
    } else if m % (60 * 24) == 0 {
        format!("P{}D", m / (60 * 24))
    } else if m % 60 == 0 {
        format!("PT{}H", m / 60)
    } else {
        format!("PT{m}M")
    };
    if neg { format!("-{body}") } else { body }
}

/// One CalDAV resource: a VCALENDAR with a master event and its overrides.
#[derive(Debug, Clone, PartialEq)]
pub struct CalendarObject {
    pub calendar: Component,
    pub events: Vec<Event>,
}

impl CalendarObject {
    pub fn parse(ics_text: &str) -> Result<Self, ics::IcsError> {
        let calendar = ics::parse(ics_text)?;
        let events = calendar.children_named("VEVENT").filter_map(Event::from_component).collect();
        Ok(CalendarObject { calendar, events })
    }

    pub fn new(event: Event) -> Self {
        let mut calendar = Component::new("VCALENDAR");
        calendar.props.push(Property::new("VERSION", "2.0"));
        calendar.props.push(Property::new("PRODID", PRODID));
        CalendarObject { calendar, events: vec![event] }
    }

    pub fn uid(&self) -> &str {
        self.events.first().map(|e| e.uid.as_str()).unwrap_or("")
    }

    pub fn master(&self) -> Option<&Event> {
        self.events.iter().find(|e| e.recurrence_id.is_none()).or(self.events.first())
    }

    pub fn master_mut(&mut self) -> Option<&mut Event> {
        let i = self.events.iter().position(|e| e.recurrence_id.is_none()).unwrap_or(0);
        self.events.get_mut(i)
    }

    pub fn to_ics(&self) -> String {
        let mut cal = self.calendar.clone();
        cal.children.retain(|c| !c.name.eq_ignore_ascii_case("VEVENT"));
        for e in &self.events {
            cal.children.push(e.to_component());
        }
        cal.to_ics()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(parse_duration("-PT15M"), Some(Duration::minutes(-15)));
        assert_eq!(parse_duration("P1DT2H"), Some(Duration::hours(26)));
        assert_eq!(parse_duration("P1W"), Some(Duration::days(7)));
        assert_eq!(format_duration(-15), "-PT15M");
        assert_eq!(format_duration(-60), "-PT1H");
        assert_eq!(format_duration(-1440), "-P1D");
    }

    #[test]
    fn event_round_trip_keeps_unknowns() {
        let src = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTIMEZONE\r\nTZID:Europe/Berlin\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:x1\r\nSUMMARY:Standup\r\nX-NEXTCLOUD-THING:1\r\nDTSTART;TZID=Europe/Berlin:20241015T090000\r\nDURATION:PT30M\r\nBEGIN:VALARM\r\nTRIGGER:-PT10M\r\nACTION:DISPLAY\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let mut obj = CalendarObject::parse(src).unwrap();
        let ev = obj.master_mut().unwrap();
        assert_eq!(ev.duration(), Duration::minutes(30));
        assert_eq!(ev.alarms, vec![10]);
        ev.summary = "Daily standup".into();
        let out = obj.to_ics();
        assert!(out.contains("X-NEXTCLOUD-THING:1"));
        assert!(out.contains("BEGIN:VTIMEZONE"));
        assert!(out.contains("SUMMARY:Daily standup"));
        assert!(out.contains("DTSTART;TZID=Europe/Berlin:20241015T090000"));
        assert!(out.contains("DTEND;TZID=Europe/Berlin:20241015T093000"));
        let again = CalendarObject::parse(&out).unwrap();
        assert_eq!(again.master().unwrap().alarms, vec![10]);
    }

    #[test]
    fn all_day_defaults() {
        let src = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nDTSTART;VALUE=DATE:20241024\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let obj = CalendarObject::parse(src).unwrap();
        let e = obj.master().unwrap();
        assert!(e.all_day());
        assert_eq!(e.end, When::Date(NaiveDate::from_ymd_opt(2024, 10, 25).unwrap()));
    }
}
