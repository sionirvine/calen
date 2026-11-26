//! GNOME Shell search provider (`org.gnome.Shell.SearchProvider2`): typing in
//! Activities lists matching Calen events; picking one opens it in Calen.
//!
//! It runs inside the app and answers from the latest loaded data
//! (`crate::search`). The Flatpak installs the provider `.ini` and a D-Bus
//! service file, so a search while Calen isn't running starts it in the tray.

use std::collections::HashMap;
use std::sync::OnceLock;

use zbus::zvariant::{OwnedValue, Value};

use super::{PlatformEvent, emit};

pub const BUS_NAME: &str = "com.felixadr.Calen.SearchProvider";
pub const OBJECT_PATH: &str = "/com/felixadr/Calen/SearchProvider";
const MAX_RESULTS: usize = 10;
/// Separates the parts of a result id: calendar id, UID, start (unix seconds).
const SEP: char = '\u{1f}';

struct Provider;

fn result_ids(terms: &[String]) -> Vec<String> {
    let query = terms.join(" ");
    if query.trim().is_empty() {
        return Vec::new();
    }
    crate::search::with_index(|ix| {
        crate::search::search_events(&ix.data, &query, ix.time_24h, MAX_RESULTS)
            .into_iter()
            .map(|h| format!("{}{SEP}{}{SEP}{}", h.key.cal_id, h.key.uid, h.date.timestamp()))
            .collect()
    })
    .unwrap_or_default()
}

/// (calendar id, UID, start) from a result id.
pub fn parse_id(id: &str) -> Option<(String, String, i64)> {
    let mut parts = id.split(SEP);
    let (cal, uid, at) = (parts.next()?, parts.next()?, parts.next()?.parse().ok()?);
    Some((cal.to_string(), uid.to_string(), at))
}

fn meta(id: &str) -> Option<HashMap<String, OwnedValue>> {
    let (cal, uid, at) = parse_id(id)?;
    let (name, description) = crate::search::with_index(|ix| {
        let ev = ix.data.object(&cal, &uid)?.master()?;
        let date = chrono::DateTime::from_timestamp(at, 0)?.with_timezone(&chrono::Local);
        let name = if ev.summary.is_empty() { "(No title)".to_string() } else { ev.summary.clone() };
        Some((name, crate::search::detail(ev, date, ix.time_24h)))
    })??;
    let mut m = HashMap::new();
    m.insert("id".into(), owned(id));
    m.insert("name".into(), owned(&name));
    m.insert("description".into(), owned(&description));
    m.insert("gicon".into(), owned(super::APP_ID));
    Some(m)
}

fn owned(s: &str) -> OwnedValue {
    Value::from(s).try_to_owned().expect("strings have no file descriptors")
}

#[zbus::interface(name = "org.gnome.Shell.SearchProvider2")]
impl Provider {
    fn get_initial_result_set(&self, terms: Vec<String>) -> Vec<String> {
        result_ids(&terms)
    }

    /// Results are cheap to compute, so narrowing just searches again.
    fn get_subsearch_result_set(&self, _previous_results: Vec<String>, terms: Vec<String>) -> Vec<String> {
        result_ids(&terms)
    }

    fn get_result_metas(&self, identifiers: Vec<String>) -> Vec<HashMap<String, OwnedValue>> {
        identifiers.iter().filter_map(|id| meta(id)).collect()
    }

    fn activate_result(&self, identifier: String, _terms: Vec<String>, _timestamp: u32) {
        if let Some((cal_id, uid, at)) = parse_id(&identifier) {
            emit(PlatformEvent::OpenEvent { cal_id, uid, at });
        }
    }

    fn launch_search(&self, terms: Vec<String>, _timestamp: u32) {
        emit(PlatformEvent::Search(terms.join(" ")));
    }
}

/// Claim the bus name and serve the provider (in the background; best effort).
pub fn start() {
    static CONN: OnceLock<zbus::blocking::Connection> = OnceLock::new();
    std::thread::spawn(|| {
        let conn = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.name(BUS_NAME))
            .and_then(|b| b.serve_at(OBJECT_PATH, Provider))
            .and_then(|b| b.build());
        match conn {
            Ok(c) => {
                let _ = CONN.set(c);
                log::info!("GNOME Shell search provider on {BUS_NAME}");
            }
            Err(e) => log::info!("search provider unavailable: {e}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        let id = format!("1:/cal/home/{SEP}abc@x{SEP}1764000000");
        assert_eq!(parse_id(&id), Some(("1:/cal/home/".into(), "abc@x".into(), 1_764_000_000)));
        assert_eq!(parse_id("garbage"), None);
    }
}
