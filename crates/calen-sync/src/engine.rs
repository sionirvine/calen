//! Offline-first reconciliation: push queued local changes, then pull remote ones.

use calen_core::CalendarObject;
use calen_store::{Account, CalendarRow, OpKind, Store};
use std::collections::HashMap;
use std::path::Path;

use crate::client::{Client, DavError, RemoteCalendar};

#[derive(Debug, Default, Clone)]
pub struct Report {
    pub pushed: usize,
    pub pulled: usize,
    pub deleted: usize,
    pub conflicts: usize,
    pub calendars: usize,
}

pub const KEYRING_SERVICE: &str = "calen-caldav";

pub fn password_for(account: &Account) -> Option<String> {
    keyring::Entry::new(KEYRING_SERVICE, &format!("{}@{}", account.username, account.url)).ok()?.get_password().ok()
}

pub fn save_password(url: &str, username: &str, password: &str) -> Result<(), String> {
    keyring::Entry::new(KEYRING_SERVICE, &format!("{username}@{url}")).and_then(|e| e.set_password(password)).map_err(|e| e.to_string())
}

pub fn delete_password(url: &str, username: &str) {
    if let Ok(e) = keyring::Entry::new(KEYRING_SERVICE, &format!("{username}@{url}")) {
        let _ = e.delete_credential();
    }
}

fn calendar_id(account_id: i64, href: &str) -> String {
    format!("a{account_id}:{href}")
}

fn store_err(e: calen_store::StoreError) -> DavError {
    DavError::Http(format!("local database: {e}"))
}

/// Safe file name for a new object.
fn object_href(cal_href: &str, uid: &str) -> String {
    let safe: String = uid.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' }).collect();
    let base = if cal_href.ends_with('/') { cal_href.to_string() } else { format!("{cal_href}/") };
    format!("{base}{safe}.ics")
}

/// Sync one account. Opens its own DB connection so it can run off the UI thread.
pub async fn sync_account(db: &Path, account: Account, password: String) -> Result<Report, DavError> {
    let client = Client::new(&account.url, &account.username, &password)?;
    let mut report = Report::default();

    // 1. Calendars
    let remote = client.discover().await?;
    report.calendars = remote.len();
    let cal_rows = {
        let store = Store::open(db).map_err(store_err)?;
        reconcile_calendars(&store, account.id, &remote).map_err(store_err)?
    };

    // 2. Push
    let ops = {
        let store = Store::open(db).map_err(store_err)?;
        store.pending_ops().map_err(store_err)?
    };
    for op in ops {
        let Some(cal) = cal_rows.iter().find(|c| c.id == op.cal_id) else { continue };
        let Some(cal_href) = cal.href.clone() else { continue };
        match op.kind {
            OpKind::Put => {
                let row = { Store::open(db).map_err(store_err)?.object(&op.cal_id, &op.uid).map_err(store_err)? };
                let Some(row) = row else {
                    Store::open(db).map_err(store_err)?.clear_op(op.id).map_err(store_err)?;
                    continue;
                };
                let href = row.href.clone().unwrap_or_else(|| object_href(&cal_href, &op.uid));
                let etag = if row.href.is_some() { row.etag.clone() } else { None };
                match client.put(&href, &row.ics, etag.as_deref()).await {
                    Ok(new_etag) => {
                        Store::open(db)
                            .map_err(store_err)?
                            .mark_uploaded(op.id, &op.cal_id, &op.uid, &href, new_etag.as_deref())
                            .map_err(store_err)?;
                        report.pushed += 1;
                    }
                    Err(DavError::Conflict) => {
                        // Someone else changed it. Keep our edit as a copy; server version wins.
                        report.conflicts += 1;
                        let store = Store::open(db).map_err(store_err)?;
                        if let Ok(mut obj) = CalendarObject::parse(&row.ics) {
                            let new_uid = format!("{}@calen", uuid_like());
                            for e in &mut obj.events {
                                e.uid = new_uid.clone();
                                if e.recurrence_id.is_none() {
                                    e.summary = format!("{} (conflicted copy)", e.summary);
                                }
                            }
                            store.save_local(&op.cal_id, &new_uid, &obj.to_ics()).map_err(store_err)?;
                        }
                        store.clear_op(op.id).map_err(store_err)?;
                        // Force a refetch of the original.
                        store.apply_remote(&op.cal_id, &op.uid, &href, Some("\"stale\""), &row.ics).map_err(store_err)?;
                    }
                    Err(DavError::NotFound) if etag.is_some() => {
                        // Deleted remotely; recreate.
                        match client.put(&href, &row.ics, None).await {
                            Ok(new_etag) => {
                                Store::open(db)
                                    .map_err(store_err)?
                                    .mark_uploaded(op.id, &op.cal_id, &op.uid, &href, new_etag.as_deref())
                                    .map_err(store_err)?;
                                report.pushed += 1;
                            }
                            Err(e) => return Err(e),
                        }
                    }
                    Err(DavError::Http(msg)) if msg.starts_with("403") || msg.starts_with("405") => {
                        log::warn!("server refused upload of {}: {msg}", op.uid);
                        Store::open(db).map_err(store_err)?.clear_op(op.id).map_err(store_err)?;
                    }
                    Err(e) => return Err(e),
                }
            }
            OpKind::Delete => {
                let href = op.href.clone().unwrap_or_else(|| object_href(&cal_href, &op.uid));
                match client.delete(&href, op.etag.as_deref()).await {
                    Ok(()) | Err(DavError::Conflict) | Err(DavError::NotFound) => {
                        Store::open(db).map_err(store_err)?.clear_op(op.id).map_err(store_err)?;
                        report.pushed += 1;
                    }
                    Err(e) => return Err(e),
                }
            }
        }
    }

    // 3. Pull
    for cal in &cal_rows {
        let Some(href) = cal.href.clone() else { continue };
        pull_calendar(db, &client, cal, &href, &mut report).await?;
    }
    Ok(report)
}

fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{n:x}")
}

fn reconcile_calendars(store: &Store, account_id: i64, remote: &[RemoteCalendar]) -> calen_store::Result<Vec<CalendarRow>> {
    let existing: Vec<CalendarRow> = store.calendars()?.into_iter().filter(|c| c.account_id == Some(account_id)).collect();
    let mut rows = Vec::new();
    for (i, rc) in remote.iter().enumerate() {
        let id = calendar_id(account_id, &rc.href);
        let prev = existing.iter().find(|c| c.id == id);
        let row = CalendarRow {
            id: id.clone(),
            account_id: Some(account_id),
            href: Some(rc.href.clone()),
            name: rc.name.clone(),
            color: rc
                .color
                .clone()
                .map(|c| c.chars().take(7).collect())
                .unwrap_or_else(|| prev.map(|p| p.color.clone()).unwrap_or("#4f8cff".into())),
            ctag: prev.and_then(|p| p.ctag.clone()),
            sync_token: prev.and_then(|p| p.sync_token.clone()),
            visible: prev.map(|p| p.visible).unwrap_or(true),
            read_only: rc.read_only,
            sort: i as i64,
        };
        store.upsert_calendar(&row)?;
        rows.push(row);
    }
    for old in existing {
        if !remote.iter().any(|r| calendar_id(account_id, &r.href) == old.id) {
            store.delete_calendar(&old.id)?;
        }
    }
    Ok(rows)
}

async fn pull_calendar(db: &Path, client: &Client, cal: &CalendarRow, href: &str, report: &mut Report) -> Result<(), DavError> {
    // Try incremental first.
    let incremental = match &cal.sync_token {
        Some(t) => match client.sync_collection(href, Some(t)).await {
            Ok(c) => Some(c),
            Err(DavError::TokenExpired) | Err(DavError::Http(_)) | Err(DavError::Parse(_)) => None,
            Err(e) => return Err(e),
        },
        None => None,
    };

    let local: Vec<calen_store::ObjectRow> = Store::open(db).map_err(store_err)?.objects_in(&cal.id).map_err(store_err)?;
    let by_href: HashMap<String, &calen_store::ObjectRow> = local.iter().filter_map(|o| o.href.clone().map(|h| (h, o))).collect();
    let norm = |h: &str| client.url(h).map(|u| u.path().to_string()).unwrap_or_else(|_| h.to_string());
    let by_path: HashMap<String, &calen_store::ObjectRow> = by_href.iter().map(|(h, o)| (norm(h), *o)).collect();

    let (changed, deleted, token) = match incremental {
        Some(c) => (c.changed, c.deleted, c.token),
        None => {
            // Full comparison (first sync or token rejected).

            match client.sync_collection(href, None).await {
                Ok(c) => {
                    let changed = c.changed;
                    let remote_paths: std::collections::HashSet<String> = changed.iter().map(|(h, _)| norm(h)).collect();
                    let deleted = by_path.keys().filter(|p| !remote_paths.contains(*p)).cloned().collect::<Vec<_>>();
                    (changed, deleted, c.token)
                }
                Err(DavError::Offline(e)) => return Err(DavError::Offline(e)),
                Err(DavError::Auth) => return Err(DavError::Auth),
                Err(_) => {
                    let all = client.list_etags(href).await?;
                    let remote_paths: std::collections::HashSet<String> = all.iter().map(|(h, _)| norm(h)).collect();
                    let deleted = by_path.keys().filter(|p| !remote_paths.contains(*p)).cloned().collect::<Vec<_>>();
                    let token = client.ctag(href).await.ok().flatten();
                    (all, deleted, token)
                }
            }
        }
    };

    let store = Store::open(db).map_err(store_err)?;
    // Deletions (skip ones with unpushed local edits).
    for d in &deleted {
        if let Some(o) = by_path.get(&norm(d)) {
            if store.has_pending(&cal.id, &o.uid).map_err(store_err)? {
                continue;
            }
            store.remove_remote(&cal.id, o.href.as_deref().unwrap_or(d)).map_err(store_err)?;
            report.deleted += 1;
        }
    }
    drop(store);

    // Fetch changed objects whose etag differs.
    let need: Vec<String> = changed
        .into_iter()
        .filter(|(h, etag)| match by_path.get(&norm(h)) {
            Some(o) => etag.is_none() || o.etag != *etag,
            None => true,
        })
        .map(|(h, _)| h)
        .collect();
    if !need.is_empty() {
        let fetched = client.multiget(href, &need).await?;
        let store = Store::open(db).map_err(store_err)?;
        for (h, etag, ics) in fetched {
            let Ok(obj) = CalendarObject::parse(&ics) else {
                log::warn!("unparsable object at {h}");
                continue;
            };
            if obj.events.is_empty() {
                continue; // VTODO / VJOURNAL
            }
            let uid = obj.uid().to_string();
            if store.has_pending(&cal.id, &uid).map_err(store_err)? {
                continue;
            }
            store.apply_remote(&cal.id, &uid, &h, etag.as_deref(), &ics).map_err(store_err)?;
            report.pulled += 1;
        }
    }

    let store = Store::open(db).map_err(store_err)?;
    store.set_sync_state(&cal.id, cal.ctag.as_deref(), token.as_deref()).map_err(store_err)?;
    Ok(())
}

/// Validate credentials and list calendars (used by the "add account" dialog).
pub async fn test_account(url: String, user: String, pass: String) -> Result<Vec<RemoteCalendar>, DavError> {
    Client::new(&url, &user, &pass)?.discover().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hrefs() {
        assert_eq!(object_href("/dav/cal/", "abc@calen"), "/dav/cal/abc-calen.ics");
        assert_eq!(object_href("/dav/cal", "a b"), "/dav/cal/a-b.ics");
    }
}
