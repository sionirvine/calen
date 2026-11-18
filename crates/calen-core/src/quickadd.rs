//! Natural-language quick add, e.g.
//! `Lunch with Susan tomorrow 12-1:30pm at DiBella's /work !15m`.

use chrono::{Datelike, Duration, NaiveDate, NaiveTime, Weekday};

use crate::event::{When, local_midnight};
use crate::links;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum DateOrder {
    /// 18/11 = 18 November.
    #[default]
    DayMonth,
    /// 11/18 = November 18.
    MonthDay,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct QuickAdd {
    pub title: String,
    pub date: Option<NaiveDate>,
    pub start_time: Option<NaiveTime>,
    pub end_time: Option<NaiveTime>,
    pub end_date: Option<NaiveDate>,
    pub duration: Option<Duration>,
    pub all_day: bool,
    pub location: Option<String>,
    pub url: Option<String>,
    pub calendar: Option<String>,
    pub alert_minutes: Option<i64>,
    pub rrule: Option<String>,
}

impl QuickAdd {
    /// Concrete start/end, using `default_minutes` when no end/duration was given.
    pub fn resolve(&self, today: NaiveDate, default_minutes: i64) -> (When, When) {
        let date = self.date.unwrap_or(today);
        match self.start_time {
            Some(t) if !self.all_day => {
                let start = local_midnight(date) + (t - NaiveTime::MIN);
                let end = if let Some(e) = self.end_time {
                    let end_date = self.end_date.unwrap_or(date);
                    let mut end = local_midnight(end_date) + (e - NaiveTime::MIN);
                    if end <= start {
                        end += Duration::days(1);
                    }
                    end
                } else {
                    start + self.duration.unwrap_or(Duration::minutes(default_minutes))
                };
                (When::at_local(start), When::at_local(end))
            }
            _ => {
                let days = self
                    .end_date
                    .map(|e| (e - date).num_days() + 1)
                    .or_else(|| self.duration.map(|d| d.num_days().max(1)))
                    .unwrap_or(1)
                    .max(1);
                (When::Date(date), When::Date(date + Duration::days(days)))
            }
        }
    }

    pub fn has_time(&self) -> bool {
        self.start_time.is_some()
    }
}

#[derive(Debug, Clone)]
struct Tok {
    raw: String,
    low: String,
    used: bool,
}

pub fn parse(input: &str, today: NaiveDate, order: DateOrder) -> QuickAdd {
    let mut q = QuickAdd::default();
    let mut toks = tokenize(input);

    // URLs first: maps → location, meetings/other → url.
    for t in toks.iter_mut() {
        if t.low.starts_with("http://") || t.low.starts_with("https://") {
            t.used = true;
            if let Some(m) = links::find_maps(&t.raw) {
                q.location = Some(m.place.clone().unwrap_or_else(|| m.url.clone()));
                if m.place.is_some() {
                    q.url.get_or_insert(m.url);
                }
            } else {
                q.url = Some(t.raw.clone());
            }
        }
    }

    // /calendar and !alert
    for t in toks.iter_mut().filter(|t| !t.used) {
        if let Some(cal) = t.raw.strip_prefix('/').filter(|s| !s.is_empty()) {
            q.calendar = Some(cal.replace('_', " "));
            t.used = true;
        } else if let Some(a) = t.low.strip_prefix('!').and_then(parse_short_duration) {
            q.alert_minutes = Some(a.num_minutes());
            t.used = true;
        }
    }

    parse_recurrence(&mut toks, &mut q, today);
    parse_dates(&mut toks, &mut q, today, order);
    parse_times(&mut toks, &mut q);
    parse_location(&mut toks, &mut q);

    // Recurrence by weekday without explicit date: first occurrence.
    if q.date.is_none()
        && let Some(r) = &q.rrule
        && let Some(wd) = r.split("BYDAY=").nth(1).and_then(|d| weekday_from_ical(&d[..2]))
    {
        q.date = Some(next_weekday(today, wd, true));
    }

    let words: Vec<&str> = toks.iter().filter(|t| !t.used).map(|t| t.raw.as_str()).collect();
    q.title = clean_title(&words);
    q
}

fn tokenize(input: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    for word in input.split_whitespace() {
        if word.starts_with("http://") || word.starts_with("https://") {
            out.push(word.to_string());
            continue;
        }
        // Split ranges glued together: "3-4pm", "10:00–11:30", but not ISO dates.
        let is_iso = word.len() == 10 && word.as_bytes().get(4) == Some(&b'-') && word.as_bytes().get(7) == Some(&b'-');
        if !is_iso && (word.contains('-') || word.contains('–')) && word.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            let normalized = word.replace('–', "-");
            let parts: Vec<&str> = normalized.split('-').collect();
            for (i, p) in parts.iter().enumerate() {
                if i > 0 {
                    out.push("-".into());
                }
                if !p.is_empty() {
                    out.push(p.to_string());
                }
            }
            continue;
        }
        out.push(word.to_string());
    }
    out.into_iter().map(|raw| Tok { low: raw.to_lowercase().trim_end_matches([',', '.']).to_string(), raw, used: false }).collect()
}

fn clean_title(words: &[&str]) -> String {
    let filler = ["at", "on", "from", "to", "in", "for", "every", "-", "@"];
    let mut ws: Vec<&str> = words.to_vec();
    while ws.last().is_some_and(|w| filler.contains(&w.to_lowercase().as_str())) {
        ws.pop();
    }
    while ws.first().is_some_and(|w| ["on", "at", "-"].contains(&w.to_lowercase().as_str())) {
        ws.remove(0);
    }
    // collapse connectors left dangling in the middle ("Lunch on with Sue")
    let mut out: Vec<&str> = Vec::new();
    for (i, w) in ws.iter().enumerate() {
        let lw = w.to_lowercase();
        let next_is_filler = ws.get(i + 1).is_some_and(|n| filler.contains(&n.to_lowercase().as_str()));
        if (lw == "on" || lw == "at") && (next_is_filler || i + 1 == ws.len()) {
            continue;
        }
        out.push(w);
    }
    out.join(" ").trim_end_matches([',', ' ']).to_string()
}

fn parse_short_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    let split = s.find(|c: char| !c.is_ascii_digit())?;
    let (n, unit) = s.split_at(split);
    let n: i64 = n.parse().ok()?;
    Some(match unit {
        "m" | "min" | "mins" | "minute" | "minutes" => Duration::minutes(n),
        "h" | "hr" | "hrs" | "hour" | "hours" => Duration::hours(n),
        "d" | "day" | "days" => Duration::days(n),
        "w" | "wk" | "week" | "weeks" => Duration::weeks(n),
        _ => return None,
    })
}

fn weekday(s: &str) -> Option<Weekday> {
    Some(match s {
        "mon" | "monday" => Weekday::Mon,
        "tue" | "tues" | "tuesday" => Weekday::Tue,
        "wed" | "weds" | "wednesday" => Weekday::Wed,
        "thu" | "thur" | "thurs" | "thursday" => Weekday::Thu,
        "fri" | "friday" => Weekday::Fri,
        "sat" | "saturday" => Weekday::Sat,
        "sun" | "sunday" => Weekday::Sun,
        _ => return None,
    })
}

fn weekday_ical(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "MO",
        Weekday::Tue => "TU",
        Weekday::Wed => "WE",
        Weekday::Thu => "TH",
        Weekday::Fri => "FR",
        Weekday::Sat => "SA",
        Weekday::Sun => "SU",
    }
}

fn weekday_from_ical(s: &str) -> Option<Weekday> {
    [Weekday::Mon, Weekday::Tue, Weekday::Wed, Weekday::Thu, Weekday::Fri, Weekday::Sat, Weekday::Sun]
        .into_iter()
        .find(|w| weekday_ical(*w) == s)
}

fn month(s: &str) -> Option<u32> {
    let m = match s.get(..3)? {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    };
    // Reject words that merely start like a month ("mayor", "decide").
    let full = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];
    let ok = full[(m - 1) as usize].starts_with(s) || (s.len() == 4 && s == "sept");
    ok.then_some(m)
}

fn day_number(s: &str) -> Option<u32> {
    let digits = s.trim_end_matches("st").trim_end_matches("nd").trim_end_matches("rd").trim_end_matches("th");
    let d: u32 = digits.parse().ok()?;
    (1..=31).contains(&d).then_some(d)
}

/// Next date with weekday `w`; `include_today` decides whether today counts.
fn next_weekday(today: NaiveDate, w: Weekday, include_today: bool) -> NaiveDate {
    let mut d = if include_today { today } else { today + Duration::days(1) };
    while d.weekday() != w {
        d += Duration::days(1);
    }
    d
}

/// A date without year: pick this year, or next year if it already passed.
fn upcoming(today: NaiveDate, m: u32, d: u32) -> Option<NaiveDate> {
    let this = NaiveDate::from_ymd_opt(today.year(), m, d)?;
    if this < today { NaiveDate::from_ymd_opt(today.year() + 1, m, d) } else { Some(this) }
}

fn parse_recurrence(toks: &mut [Tok], q: &mut QuickAdd, _today: NaiveDate) {
    let n = toks.len();
    for i in 0..n {
        if toks[i].used {
            continue;
        }
        let w = toks[i].low.clone();
        let simple = match w.as_str() {
            "daily" => Some("FREQ=DAILY".to_string()),
            "weekly" => Some("FREQ=WEEKLY".to_string()),
            "monthly" => Some("FREQ=MONTHLY".to_string()),
            "yearly" | "annually" => Some("FREQ=YEARLY".to_string()),
            _ => None,
        };
        if let Some(r) = simple {
            q.rrule = Some(r);
            toks[i].used = true;
            continue;
        }
        if w != "every" || i + 1 >= n {
            continue;
        }
        let mut j = i + 1;
        let mut interval = 1;
        if let Ok(k) = toks[j].low.parse::<u32>() {
            interval = k;
            j += 1;
        } else if toks[j].low == "other" {
            interval = 2;
            j += 1;
        }
        let Some(unit) = toks.get(j).map(|t| t.low.clone()) else { continue };
        let unit = unit.trim_end_matches('s');
        let rule = match unit {
            "day" => Some("FREQ=DAILY".to_string()),
            "week" => Some("FREQ=WEEKLY".to_string()),
            "month" => Some("FREQ=MONTHLY".to_string()),
            "year" => Some("FREQ=YEARLY".to_string()),
            "weekday" => Some("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR".to_string()),
            "weekend" => Some("FREQ=WEEKLY;BYDAY=SA,SU".to_string()),
            other => weekday(other).map(|wd| format!("FREQ=WEEKLY;BYDAY={}", weekday_ical(wd))),
        };
        if let Some(mut r) = rule {
            if interval > 1 {
                r.push_str(&format!(";INTERVAL={interval}"));
            }
            q.rrule = Some(r);
            for t in &mut toks[i..=j] {
                t.used = true;
            }
        }
    }
}

fn parse_dates(toks: &mut [Tok], q: &mut QuickAdd, today: NaiveDate, order: DateOrder) {
    let n = toks.len();
    let mut i = 0;
    while i < n {
        if toks[i].used {
            i += 1;
            continue;
        }
        let w = toks[i].low.clone();
        let next = toks.get(i + 1).filter(|t| !t.used).map(|t| t.low.clone());
        let mut consumed = 0;
        let mut date = None;

        match w.as_str() {
            "today" | "tonight" => {
                date = Some(today);
                consumed = 1;
                if w == "tonight" && q.start_time.is_none() {
                    q.start_time = NaiveTime::from_hms_opt(19, 0, 0);
                }
            }
            "tomorrow" | "tmr" | "tmrw" | "tom" => {
                date = Some(today + Duration::days(1));
                consumed = 1;
            }
            "yesterday" => {
                date = Some(today - Duration::days(1));
                consumed = 1;
            }
            "all-day" | "allday" => {
                q.all_day = true;
                toks[i].used = true;
            }
            "all" if next.as_deref() == Some("day") => {
                q.all_day = true;
                toks[i].used = true;
                toks[i + 1].used = true;
            }
            "next" | "this" => {
                if let Some(nx) = next.as_deref() {
                    if let Some(wd) = weekday(nx) {
                        let base = next_weekday(today, wd, w == "this");
                        // "next friday" when today is Wed = friday of next week, like most apps.
                        let d = if w == "next" && (base - today).num_days() < 7 && base.iso_week() == today.iso_week() {
                            base + Duration::days(7)
                        } else {
                            base
                        };
                        date = Some(d);
                        consumed = 2;
                    } else if nx == "week" {
                        date = Some(next_weekday(today, Weekday::Mon, false));
                        consumed = 2;
                    } else if nx == "month" {
                        let (y, m) = if today.month() == 12 { (today.year() + 1, 1) } else { (today.year(), today.month() + 1) };
                        date = NaiveDate::from_ymd_opt(y, m, 1);
                        consumed = 2;
                    }
                }
            }
            "in" => {
                if let (Some(nx), Some(unit)) = (next.as_deref(), toks.get(i + 2).map(|t| t.low.clone()))
                    && let Ok(k) = nx.parse::<i64>()
                {
                    let d = match unit.trim_end_matches('s') {
                        "day" => Some(today + Duration::days(k)),
                        "week" => Some(today + Duration::weeks(k)),
                        "month" => today.checked_add_months(chrono::Months::new(k as u32)),
                        _ => None,
                    };
                    if d.is_some() {
                        date = d;
                        consumed = 3;
                    }
                }
            }
            _ => {
                if let Some(wd) = weekday(&w) {
                    date = Some(next_weekday(today, wd, true));
                    consumed = 1;
                } else if let Some(m) = month(&w) {
                    // "nov 18", "november 18th 2027"
                    if let Some(d) = next.as_deref().and_then(day_number) {
                        consumed = 2;
                        let year = toks.get(i + 2).and_then(|t| t.low.parse::<i32>().ok()).filter(|y| *y > 1900);
                        date = match year {
                            Some(y) => {
                                consumed = 3;
                                NaiveDate::from_ymd_opt(y, m, d)
                            }
                            None => upcoming(today, m, d),
                        };
                    }
                } else if let Some(d) = day_number(&w).filter(|_| !w.contains(':')) {
                    // "18 nov", "18th of november"
                    let (mi, skip) = if next.as_deref() == Some("of") { (i + 2, 1) } else { (i + 1, 0) };
                    if let Some(m) = toks.get(mi).filter(|t| !t.used).and_then(|t| month(&t.low)) {
                        consumed = 2 + skip;
                        let year = toks.get(mi + 1).and_then(|t| t.low.parse::<i32>().ok()).filter(|y| *y > 1900);
                        date = match year {
                            Some(y) => {
                                consumed += 1;
                                NaiveDate::from_ymd_opt(y, m, d)
                            }
                            None => upcoming(today, m, d),
                        };
                    }
                } else if let Some(d) = parse_numeric_date(&w, today, order) {
                    date = Some(d);
                    consumed = 1;
                }
            }
        }

        if let Some(d) = date {
            // Skip a preceding "on".
            if i > 0 && !toks[i - 1].used && toks[i - 1].low == "on" {
                toks[i - 1].used = true;
            }
            for t in &mut toks[i..i + consumed] {
                t.used = true;
            }
            if q.date.is_none() {
                q.date = Some(d);
            } else if q.end_date.is_none() && q.date.is_some_and(|start| d >= start) {
                q.end_date = Some(d);
            }
            i += consumed;
        } else {
            i += 1;
        }
    }
}

fn parse_numeric_date(w: &str, today: NaiveDate, order: DateOrder) -> Option<NaiveDate> {
    if let Ok(d) = NaiveDate::parse_from_str(w, "%Y-%m-%d") {
        return Some(d);
    }
    let parts: Vec<&str> = w.split(['/', '.']).collect();
    if parts.len() < 2 || parts.len() > 3 || parts.iter().any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    // Avoid eating times like "1.30".
    if parts.len() == 2 && w.contains('.') {
        return None;
    }
    let a: u32 = parts[0].parse().ok()?;
    let b: u32 = parts[1].parse().ok()?;
    let (d, m) = match order {
        DateOrder::DayMonth => (a, b),
        DateOrder::MonthDay => (b, a),
    };
    if parts.len() == 3 {
        let mut y: i32 = parts[2].parse().ok()?;
        if y < 100 {
            y += 2000;
        }
        NaiveDate::from_ymd_opt(y, m, d)
    } else {
        upcoming(today, m, d)
    }
}

#[derive(Debug, Clone, Copy)]
struct TimeTok {
    hour: u32,
    minute: u32,
    /// Some(true) = pm, Some(false) = am, None = unspecified (or 24h).
    pm: Option<bool>,
    explicit_24h: bool,
}

impl TimeTok {
    fn to_time(self) -> Option<NaiveTime> {
        let mut h = self.hour;
        match self.pm {
            Some(true) if h < 12 => h += 12,
            Some(false) if h == 12 => h = 0,
            _ => {}
        }
        NaiveTime::from_hms_opt(h, self.minute, 0)
    }
}

/// Parse one time token; may also consume a following "am"/"pm" token.
fn parse_time_at(toks: &[Tok], i: usize, needs_marker: bool) -> Option<(TimeTok, usize)> {
    let w = toks.get(i).filter(|t| !t.used)?.low.as_str();
    match w {
        "noon" | "midday" => return Some((TimeTok { hour: 12, minute: 0, pm: Some(true), explicit_24h: true }, 1)),
        "midnight" => return Some((TimeTok { hour: 0, minute: 0, pm: None, explicit_24h: true }, 1)),
        _ => {}
    }
    let (body, mut pm) = if let Some(b) = w.strip_suffix("am").or_else(|| w.strip_suffix("a.m.")).or_else(|| w.strip_suffix('a')) {
        (b, Some(false))
    } else if let Some(b) = w.strip_suffix("pm").or_else(|| w.strip_suffix("p.m.")).or_else(|| w.strip_suffix('p')) {
        (b, Some(true))
    } else {
        (w, None)
    };
    let body = body.trim_end_matches(['h']);
    let (h, m, has_colon) = if let Some((h, m)) = body.split_once([':', '.']) {
        (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?, true)
    } else if body.len() == 4 && body.chars().all(|c| c.is_ascii_digit()) && pm.is_none() && w.ends_with('h') {
        (body[..2].parse().ok()?, body[2..].parse().ok()?, true)
    } else {
        (body.parse::<u32>().ok()?, 0, false)
    };
    if h > 23 || m > 59 || body.is_empty() {
        return None;
    }
    let mut used = 1;
    if pm.is_none()
        && let Some(nt) = toks.get(i + 1).filter(|t| !t.used)
    {
        match nt.low.as_str() {
            "am" | "a.m." => {
                pm = Some(false);
                used = 2;
            }
            "pm" | "p.m." => {
                pm = Some(true);
                used = 2;
            }
            _ => {}
        }
    }
    if needs_marker && pm.is_none() && !has_colon {
        return None;
    }
    if pm.is_some() && h > 12 {
        return None;
    }
    Some((TimeTok { hour: h, minute: m, pm, explicit_24h: has_colon && h >= 13 || h == 0 }, used))
}

/// Unqualified hour (no am/pm, no 24h): guess like a human would.
fn guess(t: TimeTok) -> TimeTok {
    if t.pm.is_some() || t.explicit_24h || t.hour >= 13 {
        return t;
    }
    // 1–7 → afternoon, 8–11 → morning, 12 → noon.
    let pm = (1..=7).contains(&t.hour) || t.hour == 12;
    TimeTok { pm: Some(pm), ..t }
}

fn parse_times(toks: &mut [Tok], q: &mut QuickAdd) {
    let n = toks.len();
    let mut i = 0;
    while i < n {
        if toks[i].used {
            i += 1;
            continue;
        }
        let w = toks[i].low.clone();

        // "for 2h", "for 30 min", "for 2 days"
        if w == "for"
            && let Some(nt) = toks.get(i + 1).filter(|t| !t.used)
        {
            if let Some(d) = parse_short_duration(&nt.low) {
                q.duration = Some(d);
                toks[i].used = true;
                toks[i + 1].used = true;
                i += 2;
                continue;
            }
            if let (Ok(k), Some(unit)) = (nt.low.parse::<i64>(), toks.get(i + 2).map(|t| t.low.clone()))
                && let Some(d) = parse_short_duration(&format!("{k}{unit}"))
            {
                q.duration = Some(d);
                for t in &mut toks[i..i + 3] {
                    t.used = true;
                }
                i += 3;
                continue;
            }
        }

        let lead = matches!(w.as_str(), "at" | "@" | "from" | "by");
        let start_i = if lead { i + 1 } else { i };
        // Bare numbers need am/pm or a colon unless introduced by "at"/"from" or part of a range.
        let is_range_next =
            |j: usize| toks.get(j).is_some_and(|t| !t.used && (t.low == "-" || t.low == "to" || t.low == "until" || t.low == "till"));
        let needs_marker = !lead;
        let first = parse_time_at(toks, start_i, needs_marker).or_else(|| {
            // "12 - 1:30 pm": a bare start is fine if a range follows.
            let (t, used) = parse_time_at(toks, start_i, false)?;
            is_range_next(start_i + used).then_some((t, used))
        });
        let Some((t1, used1)) = first else {
            i += 1;
            continue;
        };
        let mut end_idx = start_i + used1;
        let mut t2 = None;
        if is_range_next(end_idx)
            && let Some((t, used2)) = parse_time_at(toks, end_idx + 1, false)
        {
            t2 = Some(t);
            end_idx = end_idx + 1 + used2;
        }

        let (s, e) = match t2 {
            Some(e) => {
                let e = guess(e);
                let mut s = t1;
                if s.pm.is_none() && !s.explicit_24h {
                    // Borrow the end's meridiem unless that puts start after end.
                    let candidate = TimeTok { pm: e.pm, ..s };
                    s = match (candidate.to_time(), e.to_time()) {
                        (Some(a), Some(b)) if a <= b => candidate,
                        _ => TimeTok { pm: Some(false), ..s },
                    };
                }
                (s, Some(e))
            }
            None => (guess(t1), None),
        };
        if q.start_time.is_none() {
            q.start_time = s.to_time();
            q.end_time = e.and_then(|e| e.to_time());
            for t in &mut toks[i..end_idx] {
                t.used = true;
            }
        }
        i = end_idx;
    }
}

fn parse_location(toks: &mut [Tok], q: &mut QuickAdd) {
    if q.location.is_some() {
        return;
    }
    // Last unused "at"/"@" followed by unused words → location till the end (or next used token).
    let Some(i) = toks.iter().rposition(|t| !t.used && (t.low == "at" || t.low == "@")) else { return };
    let mut words = Vec::new();
    let mut j = i + 1;
    while j < toks.len() && !toks[j].used {
        words.push(toks[j].raw.clone());
        j += 1;
    }
    if words.is_empty() {
        return;
    }
    q.location = Some(words.join(" "));
    for t in &mut toks[i..j] {
        t.used = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        // Wednesday
        NaiveDate::from_ymd_opt(2026, 11, 18).unwrap()
    }

    fn p(s: &str) -> QuickAdd {
        parse(s, today(), DateOrder::DayMonth)
    }

    fn t(h: u32, m: u32) -> Option<NaiveTime> {
        NaiveTime::from_hms_opt(h, m, 0)
    }

    fn d(m: u32, day: u32) -> Option<NaiveDate> {
        NaiveDate::from_ymd_opt(2026, m, day)
    }

    #[test]
    fn fantastical_example() {
        let q = p("Catch up over lunch with Susan at 12 - 1:30 pm at DiBella's Old Fashioned Subs");
        assert_eq!(q.title, "Catch up over lunch with Susan");
        assert_eq!(q.start_time, t(12, 0));
        assert_eq!(q.end_time, t(13, 30));
        assert_eq!(q.location.as_deref(), Some("DiBella's Old Fashioned Subs"));
        assert_eq!(q.date, None);
    }

    #[test]
    fn table() {
        struct Case(&'static str, &'static str, Option<NaiveDate>, Option<NaiveTime>, Option<NaiveTime>);
        let cases = [
            Case("Dentist tomorrow 3pm", "Dentist", d(11, 19), t(15, 0), None),
            Case("Standup 9:15", "Standup", None, t(9, 15), None),
            Case("Gym at 7", "Gym", None, t(19, 0), None),
            Case("Gym at 7am", "Gym", None, t(7, 0), None),
            Case("Review 15:00-16:30 friday", "Review", d(11, 20), t(15, 0), t(16, 30)),
            Case("Call mom on sunday at noon", "Call mom", d(11, 22), t(12, 0), None),
            Case("Bayar Pegawai 21/11 14:30", "Bayar Pegawai", d(11, 21), t(14, 30), None),
            Case("Service AC dec 1 8am", "Service AC", d(12, 1), t(8, 0), None),
            Case("Lapor PLN 24 nov", "Lapor PLN", d(11, 24), None, None),
            Case("Party next friday 8pm", "Party", d(11, 27), t(20, 0), None),
            Case("Sync 10-11", "Sync", None, t(10, 0), t(11, 0)),
            Case("Movie 11pm-1am", "Movie", None, t(23, 0), t(1, 0)),
            Case("Trip 2027-01-05", "Trip", NaiveDate::from_ymd_opt(2027, 1, 5), None, None),
            Case("Old 1 jan", "Old", NaiveDate::from_ymd_opt(2027, 1, 1), None, None),
            Case("Meet mayor today", "Meet mayor", d(11, 18), None, None),
        ];
        for c in cases {
            let q = p(c.0);
            assert_eq!(q.title, c.1, "title for {:?}", c.0);
            assert_eq!(q.date, c.2, "date for {:?}", c.0);
            assert_eq!(q.start_time, c.3, "start for {:?}", c.0);
            assert_eq!(q.end_time, c.4, "end for {:?}", c.0);
        }
    }

    #[test]
    fn extras() {
        let q = p("Budget meeting tomorrow 10am for 90m /work !15m https://meet.google.com/abc-defg-hij");
        assert_eq!(q.title, "Budget meeting");
        assert_eq!(q.duration, Some(Duration::minutes(90)));
        assert_eq!(q.calendar.as_deref(), Some("work"));
        assert_eq!(q.alert_minutes, Some(15));
        assert_eq!(q.url.as_deref(), Some("https://meet.google.com/abc-defg-hij"));

        let q = p("Connect Group every tuesday 7pm");
        assert_eq!(q.title, "Connect Group");
        assert_eq!(q.rrule.as_deref(), Some("FREQ=WEEKLY;BYDAY=TU"));
        assert_eq!(q.date, d(11, 24));

        let q = p("Holiday dec 24 - dec 26 all day");
        assert!(q.all_day);
        let (s, e) = q.resolve(today(), 60);
        assert_eq!(s, When::Date(NaiveDate::from_ymd_opt(2026, 12, 24).unwrap()));
        assert_eq!(e, When::Date(NaiveDate::from_ymd_opt(2026, 12, 27).unwrap()));

        let q = p("Lunch https://www.google.com/maps/place/Gedung+Sate/@-6.9025,107.6188,17z");
        assert_eq!(q.location.as_deref(), Some("Gedung Sate"));
        assert_eq!(q.title, "Lunch");

        let q = parse("Bill 11/21", today(), DateOrder::MonthDay);
        assert_eq!(q.date, d(11, 21));
    }

    #[test]
    fn resolve_defaults() {
        let q = p("Focus 9am");
        let (s, e) = q.resolve(today(), 60);
        assert_eq!((e.utc() - s.utc()).num_minutes(), 60);
        let q = p("Laundry");
        let (s, _) = q.resolve(today(), 60);
        assert!(s.is_date());
    }
}
