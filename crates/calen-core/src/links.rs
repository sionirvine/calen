//! Detect meeting links (Zoom, Google Meet, Teams) and map links in events.

use regex::Regex;
use std::sync::OnceLock;

use crate::event::Event;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeetingKind {
    Zoom,
    GoogleMeet,
    Teams,
}

impl MeetingKind {
    pub fn label(self) -> &'static str {
        match self {
            MeetingKind::Zoom => "Zoom",
            MeetingKind::GoogleMeet => "Google Meet",
            MeetingKind::Teams => "Microsoft Teams",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Meeting {
    pub kind: MeetingKind,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MapsLink {
    pub url: String,
    /// Place name parsed from the URL, if any.
    pub place: Option<String>,
    pub geo: Option<(f64, f64)>,
    /// Short links (maps.app.goo.gl) must be resolved over HTTP to learn more.
    pub is_short: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Links {
    pub meeting: Option<Meeting>,
    pub maps: Option<MapsLink>,
}

fn re(cell: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pat).expect("valid regex"))
}

fn zoom_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"https://(?:[a-z0-9-]+\.)?zoom\.us/(?:j|my|w|s)/[A-Za-z0-9._?=&%/-]+")
}

fn meet_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"https://meet\.google\.com/[a-z]{3}-[a-z]{4}-[a-z]{3}(?:\?[A-Za-z0-9=&%._-]*)?")
}

fn teams_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"https://teams\.(?:microsoft|live)\.com/(?:l/meetup-join|meet)/[^\s<>]+")
}

fn maps_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(
        &R,
        r"https://(?:(?:www\.)?google\.[a-z.]+/maps[^\s<>]*|maps\.google\.[a-z.]+/[^\s<>]*|maps\.app\.goo\.gl/[A-Za-z0-9]+|goo\.gl/maps/[A-Za-z0-9]+)",
    )
}

/// Find the first meeting link in arbitrary text.
pub fn find_meeting(text: &str) -> Option<Meeting> {
    let candidates = [(MeetingKind::Zoom, zoom_re()), (MeetingKind::GoogleMeet, meet_re()), (MeetingKind::Teams, teams_re())];
    candidates
        .iter()
        .filter_map(|(k, r)| r.find(text).map(|m| (m.start(), *k, m.as_str())))
        .min_by_key(|(pos, _, _)| *pos)
        .map(|(_, kind, url)| Meeting { kind, url: trim_url(url).to_string() })
}

pub fn find_maps(text: &str) -> Option<MapsLink> {
    let m = maps_re().find(text)?;
    Some(parse_maps_url(trim_url(m.as_str())))
}

fn trim_url(u: &str) -> &str {
    u.trim_end_matches(['.', ',', ')', ']', '>', '"', '\''])
}

pub fn parse_maps_url(url: &str) -> MapsLink {
    let is_short = url.contains("goo.gl/");
    let place = url.split("/place/").nth(1).and_then(|rest| {
        let raw = rest.split(['/', '?', '@']).next()?;
        let decoded = percent_decode(&raw.replace('+', " "));
        let t = decoded.trim().to_string();
        (!t.is_empty()).then_some(t)
    });
    let place = place.or_else(|| {
        // ?q=Some+Place or ?query=...
        let q = url.split('?').nth(1)?;
        q.split('&').find_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            if k == "q" || k == "query" {
                let s = percent_decode(&v.replace('+', " "));
                // q=lat,lng is coordinates, not a name.
                (parse_pair(&s).is_none() && !s.is_empty()).then_some(s)
            } else {
                None
            }
        })
    });
    let geo = geo_from_maps_url(url);
    MapsLink { url: url.to_string(), place, geo, is_short }
}

fn geo_from_maps_url(url: &str) -> Option<(f64, f64)> {
    // Precise pin: !3d<lat>!4d<lng>
    if let (Some(a), Some(b)) = (url.find("!3d"), url.find("!4d")) {
        let lat = url[a + 3..].split('!').next()?.parse().ok();
        let lng = url[b + 3..].split(['!', '?', '/']).next()?.parse().ok();
        if let (Some(lat), Some(lng)) = (lat, lng) {
            return Some((lat, lng));
        }
    }
    if let Some(i) = url.find('@') {
        let tail = &url[i + 1..];
        let mut it = tail.split(',');
        let lat = it.next()?.parse().ok()?;
        let lng = it.next()?.split(['/', '?']).next()?.parse().ok()?;
        return Some((lat, lng));
    }
    let q = url.split('?').nth(1)?;
    q.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        if k == "q" || k == "query" || k == "ll" { parse_pair(&percent_decode(v)) } else { None }
    })
}

fn parse_pair(s: &str) -> Option<(f64, f64)> {
    let (a, b) = s.split_once(',')?;
    let lat: f64 = a.trim().parse().ok()?;
    let lng: f64 = b.trim().parse().ok()?;
    ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lng)).then_some((lat, lng))
}

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A Google Maps search URL for a free-text location or coordinates.
pub fn maps_search_url(location: &str, geo: Option<(f64, f64)>) -> String {
    match geo {
        Some((lat, lng)) => format!("https://www.google.com/maps/search/?api=1&query={lat},{lng}"),
        None => format!("https://www.google.com/maps/search/?api=1&query={}", percent_encode(location)),
    }
}

pub fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Scan the fields of an event where links usually live.
pub fn detect(ev: &Event) -> Links {
    let mut hay = String::new();
    for part in [ev.location.as_str(), ev.url.as_deref().unwrap_or(""), ev.description.as_str()] {
        hay.push_str(part);
        hay.push('\n');
    }
    for p in ev.component.props_named("X-GOOGLE-CONFERENCE").chain(ev.component.props_named("CONFERENCE")) {
        hay.push_str(&p.value);
        hay.push('\n');
    }
    Links { meeting: find_meeting(&hay), maps: find_maps(&hay) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meetings() {
        let m = find_meeting("Join: https://us02web.zoom.us/j/8812345678?pwd=abc123. See you").unwrap();
        assert_eq!(m.kind, MeetingKind::Zoom);
        assert_eq!(m.url, "https://us02web.zoom.us/j/8812345678?pwd=abc123");
        let m = find_meeting("Location https://meet.google.com/abc-defg-hij").unwrap();
        assert_eq!(m.kind, MeetingKind::GoogleMeet);
        assert!(find_meeting("https://meet.google.com/landing").is_none());
        assert_eq!(
            find_meeting("https://teams.microsoft.com/l/meetup-join/19%3ameeting_x%40thread.v2/0?context=y").unwrap().kind,
            MeetingKind::Teams
        );
        // earliest wins
        let m = find_meeting("https://meet.google.com/abc-defg-hij or https://zoom.us/j/1").unwrap();
        assert_eq!(m.kind, MeetingKind::GoogleMeet);
    }

    #[test]
    fn maps_place_and_geo() {
        let m = find_maps("at https://www.google.com/maps/place/DiBella's+Old+Fashioned+Subs/@43.0841,-77.6767,17z/data=!3m1!4b1!4m6!3m5!1s0x0:0x0!8m2!3d43.0841!4d-77.6741").unwrap();
        assert_eq!(m.place.as_deref(), Some("DiBella's Old Fashioned Subs"));
        assert_eq!(m.geo, Some((43.0841, -77.6741)));
        assert!(!m.is_short);
        let m = find_maps("https://maps.app.goo.gl/AbC123xyz").unwrap();
        assert!(m.is_short);
        assert!(m.place.is_none());
        let m = parse_maps_url("https://www.google.com/maps/search/?api=1&query=Gedung%20Sate%20Bandung");
        assert_eq!(m.place.as_deref(), Some("Gedung Sate Bandung"));
        let m = parse_maps_url("https://maps.google.com/?q=-6.9025,107.6188");
        assert_eq!(m.geo, Some((-6.9025, 107.6188)));
        assert!(m.place.is_none());
    }

    #[test]
    fn decode_encode() {
        assert_eq!(percent_decode("Caf%C3%A9%20Luna"), "Café Luna");
        assert_eq!(percent_encode("Café Luna"), "Caf%C3%A9+Luna");
    }
}
