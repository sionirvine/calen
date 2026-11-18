//! Local SQLite cache: the app reads and writes only here (offline-first).
//! The sync engine reconciles this with CalDAV using the `pending_ops` queue.

use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

pub use rusqlite::Error as SqlError;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, StoreError>;

pub const LOCAL_CALENDAR: &str = "local";

#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub username: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalendarRow {
    pub id: String,
    pub account_id: Option<i64>,
    pub href: Option<String>,
    pub name: String,
    pub color: String,
    pub ctag: Option<String>,
    pub sync_token: Option<String>,
    pub visible: bool,
    pub read_only: bool,
    pub sort: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObjectRow {
    pub cal_id: String,
    pub uid: String,
    pub href: Option<String>,
    pub etag: Option<String>,
    pub ics: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    Put,
    Delete,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingOp {
    pub id: i64,
    pub cal_id: String,
    pub uid: String,
    pub kind: OpKind,
    pub href: Option<String>,
    pub etag: Option<String>,
}

pub struct Store {
    conn: Connection,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS accounts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    url TEXT NOT NULL,
    username TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS calendars (
    id TEXT PRIMARY KEY,
    account_id INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
    href TEXT,
    name TEXT NOT NULL,
    color TEXT NOT NULL DEFAULT '#4f8cff',
    ctag TEXT,
    sync_token TEXT,
    visible INTEGER NOT NULL DEFAULT 1,
    read_only INTEGER NOT NULL DEFAULT 0,
    sort INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS objects (
    cal_id TEXT NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
    uid TEXT NOT NULL,
    href TEXT,
    etag TEXT,
    ics TEXT NOT NULL,
    PRIMARY KEY (cal_id, uid)
);
CREATE TABLE IF NOT EXISTS pending_ops (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cal_id TEXT NOT NULL,
    uid TEXT NOT NULL,
    kind TEXT NOT NULL,
    href TEXT,
    etag TEXT
);
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS fired_alarms (
    key TEXT PRIMARY KEY,
    fired_at INTEGER NOT NULL
);
"#;

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        let store = Store { conn };
        if store.calendar(LOCAL_CALENDAR)?.is_none() {
            store.upsert_calendar(&CalendarRow {
                id: LOCAL_CALENDAR.into(),
                account_id: None,
                href: None,
                name: "On this computer".into(),
                color: "#4f8cff".into(),
                ctag: None,
                sync_token: None,
                visible: true,
                read_only: false,
                sort: 1000,
            })?;
        }
        Ok(store)
    }

    // ---------- accounts ----------

    pub fn accounts(&self) -> Result<Vec<Account>> {
        let mut st = self.conn.prepare("SELECT id, name, url, username FROM accounts ORDER BY id")?;
        let rows = st.query_map([], |r| Ok(Account { id: r.get(0)?, name: r.get(1)?, url: r.get(2)?, username: r.get(3)? }))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn add_account(&self, name: &str, url: &str, username: &str) -> Result<i64> {
        self.conn.execute("INSERT INTO accounts (name, url, username) VALUES (?1, ?2, ?3)", params![name, url, username])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn remove_account(&self, id: i64) -> Result<()> {
        let cals: Vec<String> = self.calendars()?.into_iter().filter(|c| c.account_id == Some(id)).map(|c| c.id).collect();
        for c in cals {
            self.conn.execute("DELETE FROM pending_ops WHERE cal_id = ?1", [&c])?;
        }
        self.conn.execute("DELETE FROM accounts WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---------- calendars ----------

    pub fn calendars(&self) -> Result<Vec<CalendarRow>> {
        let mut st = self.conn.prepare(
            "SELECT id, account_id, href, name, color, ctag, sync_token, visible, read_only, sort FROM calendars ORDER BY sort, name",
        )?;
        let rows = st.query_map([], row_to_calendar)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn calendar(&self, id: &str) -> Result<Option<CalendarRow>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, account_id, href, name, color, ctag, sync_token, visible, read_only, sort FROM calendars WHERE id = ?1",
                [id],
                row_to_calendar,
            )
            .optional()?)
    }

    pub fn upsert_calendar(&self, c: &CalendarRow) -> Result<()> {
        self.conn.execute(
            "INSERT INTO calendars (id, account_id, href, name, color, ctag, sync_token, visible, read_only, sort)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET account_id=?2, href=?3, name=?4, color=?5, ctag=?6, sync_token=?7,
                visible=?8, read_only=?9, sort=?10",
            params![c.id, c.account_id, c.href, c.name, c.color, c.ctag, c.sync_token, c.visible, c.read_only, c.sort],
        )?;
        Ok(())
    }

    pub fn set_calendar_visible(&self, id: &str, visible: bool) -> Result<()> {
        self.conn.execute("UPDATE calendars SET visible = ?2 WHERE id = ?1", params![id, visible])?;
        Ok(())
    }

    pub fn set_sync_state(&self, id: &str, ctag: Option<&str>, sync_token: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE calendars SET ctag = ?2, sync_token = ?3 WHERE id = ?1", params![id, ctag, sync_token])?;
        Ok(())
    }

    pub fn delete_calendar(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM pending_ops WHERE cal_id = ?1", [id])?;
        self.conn.execute("DELETE FROM calendars WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---------- objects ----------

    pub fn objects(&self) -> Result<Vec<ObjectRow>> {
        let mut st = self.conn.prepare("SELECT cal_id, uid, href, etag, ics FROM objects")?;
        let rows = st.query_map([], row_to_object)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn objects_in(&self, cal_id: &str) -> Result<Vec<ObjectRow>> {
        let mut st = self.conn.prepare("SELECT cal_id, uid, href, etag, ics FROM objects WHERE cal_id = ?1")?;
        let rows = st.query_map([cal_id], row_to_object)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn object(&self, cal_id: &str, uid: &str) -> Result<Option<ObjectRow>> {
        Ok(self
            .conn
            .query_row("SELECT cal_id, uid, href, etag, ics FROM objects WHERE cal_id = ?1 AND uid = ?2", [cal_id, uid], row_to_object)
            .optional()?)
    }

    /// A local edit: store it and queue an upload.
    pub fn save_local(&self, cal_id: &str, uid: &str, ics: &str) -> Result<()> {
        let existing = self.object(cal_id, uid)?;
        self.conn.execute(
            "INSERT INTO objects (cal_id, uid, href, etag, ics) VALUES (?1, ?2, NULL, NULL, ?3)
             ON CONFLICT(cal_id, uid) DO UPDATE SET ics = ?3",
            params![cal_id, uid, ics],
        )?;
        if self.is_synced_calendar(cal_id)? {
            self.conn.execute("DELETE FROM pending_ops WHERE cal_id = ?1 AND uid = ?2 AND kind = 'put'", [cal_id, uid])?;
            let (href, etag) = existing.map(|e| (e.href, e.etag)).unwrap_or_default();
            self.conn.execute(
                "INSERT INTO pending_ops (cal_id, uid, kind, href, etag) VALUES (?1, ?2, 'put', ?3, ?4)",
                params![cal_id, uid, href, etag],
            )?;
        }
        Ok(())
    }

    /// A local delete: remove it and queue a server delete if it was ever uploaded.
    pub fn delete_local(&self, cal_id: &str, uid: &str) -> Result<()> {
        let existing = self.object(cal_id, uid)?;
        self.conn.execute("DELETE FROM objects WHERE cal_id = ?1 AND uid = ?2", [cal_id, uid])?;
        self.conn.execute("DELETE FROM pending_ops WHERE cal_id = ?1 AND uid = ?2", [cal_id, uid])?;
        if let Some(ObjectRow { href: Some(href), etag, .. }) = existing
            && self.is_synced_calendar(cal_id)?
        {
            self.conn.execute(
                "INSERT INTO pending_ops (cal_id, uid, kind, href, etag) VALUES (?1, ?2, 'delete', ?3, ?4)",
                params![cal_id, uid, href, etag],
            )?;
        }
        Ok(())
    }

    /// Move an object to another calendar (delete + create).
    pub fn move_local(&self, from_cal: &str, to_cal: &str, uid: &str, ics: &str) -> Result<()> {
        self.delete_local(from_cal, uid)?;
        self.save_local(to_cal, uid, ics)
    }

    /// Data that came from the server; does not queue anything.
    pub fn apply_remote(&self, cal_id: &str, uid: &str, href: &str, etag: Option<&str>, ics: &str) -> Result<()> {
        // A remote object might arrive under a different UID than one we hold at the same href.
        self.conn.execute("DELETE FROM objects WHERE cal_id = ?1 AND href = ?2 AND uid != ?3", params![cal_id, href, uid])?;
        self.conn.execute(
            "INSERT INTO objects (cal_id, uid, href, etag, ics) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(cal_id, uid) DO UPDATE SET href = ?3, etag = ?4, ics = ?5",
            params![cal_id, uid, href, etag, ics],
        )?;
        Ok(())
    }

    pub fn remove_remote(&self, cal_id: &str, href: &str) -> Result<()> {
        self.conn.execute("DELETE FROM objects WHERE cal_id = ?1 AND href = ?2", params![cal_id, href])?;
        Ok(())
    }

    /// After a successful upload: record where it lives now.
    pub fn mark_uploaded(&self, op_id: i64, cal_id: &str, uid: &str, href: &str, etag: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE objects SET href = ?3, etag = ?4 WHERE cal_id = ?1 AND uid = ?2", params![cal_id, uid, href, etag])?;
        self.clear_op(op_id)
    }

    pub fn has_pending(&self, cal_id: &str, uid: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT 1 FROM pending_ops WHERE cal_id = ?1 AND uid = ?2 LIMIT 1", [cal_id, uid], |_| Ok(()))
            .optional()?
            .is_some())
    }

    fn is_synced_calendar(&self, cal_id: &str) -> Result<bool> {
        Ok(self.calendar(cal_id)?.is_some_and(|c| c.account_id.is_some()))
    }

    // ---------- pending ops ----------

    pub fn pending_ops(&self) -> Result<Vec<PendingOp>> {
        let mut st = self.conn.prepare("SELECT id, cal_id, uid, kind, href, etag FROM pending_ops ORDER BY id")?;
        let rows = st.query_map([], |r| {
            let kind: String = r.get(3)?;
            Ok(PendingOp {
                id: r.get(0)?,
                cal_id: r.get(1)?,
                uid: r.get(2)?,
                kind: if kind == "delete" { OpKind::Delete } else { OpKind::Put },
                href: r.get(4)?,
                etag: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn pending_count(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM pending_ops", [], |r| r.get(0))?)
    }

    pub fn clear_op(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM pending_ops WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---------- settings ----------

    pub fn setting<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let raw: Option<String> = self.conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()?;
        Ok(match raw {
            Some(s) => serde_json::from_str(&s).ok(),
            None => None,
        })
    }

    pub fn set_setting<T: serde::Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let s = serde_json::to_string(value)?;
        self.conn
            .execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2", params![key, s])?;
        Ok(())
    }

    // ---------- alarms ----------

    /// Returns true the first time it is called for `key`.
    pub fn claim_alarm(&self, key: &str) -> Result<bool> {
        let n = self.conn.execute("INSERT OR IGNORE INTO fired_alarms (key, fired_at) VALUES (?1, strftime('%s','now'))", [key])?;
        Ok(n == 1)
    }

    pub fn prune_alarms(&self, older_than_secs: i64) -> Result<()> {
        self.conn.execute("DELETE FROM fired_alarms WHERE fired_at < strftime('%s','now') - ?1", [older_than_secs])?;
        Ok(())
    }
}

fn row_to_calendar(r: &rusqlite::Row<'_>) -> rusqlite::Result<CalendarRow> {
    Ok(CalendarRow {
        id: r.get(0)?,
        account_id: r.get(1)?,
        href: r.get(2)?,
        name: r.get(3)?,
        color: r.get(4)?,
        ctag: r.get(5)?,
        sync_token: r.get(6)?,
        visible: r.get(7)?,
        read_only: r.get(8)?,
        sort: r.get(9)?,
    })
}

fn row_to_object(r: &rusqlite::Row<'_>) -> rusqlite::Result<ObjectRow> {
    Ok(ObjectRow { cal_id: r.get(0)?, uid: r.get(1)?, href: r.get(2)?, etag: r.get(3)?, ics: r.get(4)? })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synced(store: &Store) -> String {
        let acc = store.add_account("nc", "https://cloud.example", "me").unwrap();
        let cal = CalendarRow {
            id: "nc-personal".into(),
            account_id: Some(acc),
            href: Some("/remote.php/dav/calendars/me/personal/".into()),
            name: "Personal".into(),
            color: "#ff0000".into(),
            ctag: None,
            sync_token: None,
            visible: true,
            read_only: false,
            sort: 0,
        };
        store.upsert_calendar(&cal).unwrap();
        cal.id
    }

    #[test]
    fn local_calendar_exists_and_never_queues() {
        let s = Store::open_in_memory().unwrap();
        s.save_local(LOCAL_CALENDAR, "u1", "ICS").unwrap();
        assert_eq!(s.pending_count().unwrap(), 0);
        assert_eq!(s.objects().unwrap().len(), 1);
    }

    #[test]
    fn edits_coalesce_and_delete_of_unsynced_drops_op() {
        let s = Store::open_in_memory().unwrap();
        let cal = synced(&s);
        s.save_local(&cal, "u1", "A").unwrap();
        s.save_local(&cal, "u1", "B").unwrap();
        assert_eq!(s.pending_ops().unwrap().len(), 1);
        s.delete_local(&cal, "u1").unwrap();
        assert_eq!(s.pending_count().unwrap(), 0);
    }

    #[test]
    fn delete_of_synced_queues_delete() {
        let s = Store::open_in_memory().unwrap();
        let cal = synced(&s);
        s.apply_remote(&cal, "u2", "/x/u2.ics", Some("\"e1\""), "ICS").unwrap();
        s.delete_local(&cal, "u2").unwrap();
        let ops = s.pending_ops().unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].kind, OpKind::Delete);
        assert_eq!(ops[0].etag.as_deref(), Some("\"e1\""));
    }

    #[test]
    fn settings_and_alarms() {
        let s = Store::open_in_memory().unwrap();
        s.set_setting("theme", &"dark").unwrap();
        assert_eq!(s.setting::<String>("theme").unwrap().as_deref(), Some("dark"));
        assert!(s.claim_alarm("k").unwrap());
        assert!(!s.claim_alarm("k").unwrap());
    }
}
