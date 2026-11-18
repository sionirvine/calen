//! Runs against a real CalDAV server when `CALEN_TEST_DAV=url,user,pass` is set
//! (e.g. a local Radicale). Skipped otherwise.

use calen_core::{CalendarObject, Event, When};
use calen_store::Store;
use chrono::{Duration, Utc};

fn creds() -> Option<(String, String, String)> {
    let v = std::env::var("CALEN_TEST_DAV").ok()?;
    let mut it = v.splitn(3, ',');
    Some((it.next()?.into(), it.next()?.into(), it.next()?.into()))
}

#[tokio::test(flavor = "multi_thread")]
async fn full_cycle() {
    let Some((url, user, pass)) = creds() else {
        eprintln!("CALEN_TEST_DAV not set; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("calen-sync-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("t.db");
    let _ = std::fs::remove_file(&db);

    let cals = calen_sync::test_account(url.clone(), user.clone(), pass.clone()).await.expect("discover");
    assert!(!cals.is_empty(), "no calendars");

    let store = Store::open(&db).unwrap();
    let acc_id = store.add_account("test", &url, &user).unwrap();
    let account = store.accounts().unwrap().into_iter().find(|a| a.id == acc_id).unwrap();
    drop(store);

    // First sync pulls existing server data.
    let r = calen_sync::sync_account(&db, account.clone(), pass.clone()).await.expect("sync 1");
    println!("first: {r:?}");
    let store = Store::open(&db).unwrap();
    let cal = store.calendars().unwrap().into_iter().find(|c| c.account_id == Some(acc_id)).expect("remote calendar");
    let before = store.objects_in(&cal.id).unwrap().len();

    // Create locally, push.
    let start = Utc::now() + Duration::days(1);
    let ev = Event::new(
        "Created by Calen test",
        When::At { utc: start, tzid: None, floating: false },
        When::At { utc: start + Duration::hours(1), tzid: None, floating: false },
    );
    let uid = ev.uid.clone();
    let obj = CalendarObject::new(ev);
    store.save_local(&cal.id, &uid, &obj.to_ics()).unwrap();
    assert_eq!(store.pending_count().unwrap(), 1);
    drop(store);
    let r = calen_sync::sync_account(&db, account.clone(), pass.clone()).await.expect("sync 2");
    assert_eq!(r.pushed, 1, "{r:?}");
    let store = Store::open(&db).unwrap();
    assert_eq!(store.pending_count().unwrap(), 0);
    let row = store.object(&cal.id, &uid).unwrap().unwrap();
    assert!(row.href.is_some());

    // Fresh database sees it from the server.
    let db2 = dir.join("t2.db");
    let _ = std::fs::remove_file(&db2);
    let s2 = Store::open(&db2).unwrap();
    let acc2 = s2.add_account("test", &url, &user).unwrap();
    let account2 = s2.accounts().unwrap().into_iter().find(|a| a.id == acc2).unwrap();
    drop(s2);
    calen_sync::sync_account(&db2, account2.clone(), pass.clone()).await.expect("sync fresh");
    let s2 = Store::open(&db2).unwrap();
    let cal2 = s2.calendars().unwrap().into_iter().find(|c| c.id == cal.id.replace(&format!("a{acc_id}:"), &format!("a{acc2}:"))).unwrap();
    assert_eq!(s2.objects_in(&cal2.id).unwrap().len(), before + 1);
    drop(s2);

    // Edit locally, push; then incremental pull on the other DB picks it up.
    let mut obj = CalendarObject::parse(&row.ics).unwrap();
    obj.master_mut().unwrap().summary = "Edited by Calen test".into();
    store.save_local(&cal.id, &uid, &obj.to_ics()).unwrap();
    drop(store);
    calen_sync::sync_account(&db, account.clone(), pass.clone()).await.expect("sync 3");
    let r = calen_sync::sync_account(&db2, account2.clone(), pass.clone()).await.expect("sync fresh 2");
    assert_eq!(r.pulled, 1, "incremental pull: {r:?}");
    let s2 = Store::open(&db2).unwrap();
    assert!(s2.object(&cal2.id, &uid).unwrap().unwrap().ics.contains("Edited by Calen test"));
    drop(s2);

    // Delete locally, push; other DB removes it.
    let store = Store::open(&db).unwrap();
    store.delete_local(&cal.id, &uid).unwrap();
    drop(store);
    calen_sync::sync_account(&db, account, pass.clone()).await.expect("sync 4");
    let r = calen_sync::sync_account(&db2, account2, pass).await.expect("sync fresh 3");
    assert_eq!(r.deleted, 1, "{r:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
