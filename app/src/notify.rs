//! Alarm scheduling: fire each VALARM once, respecting the app's Do Not
//! Disturb toggle and the OS (Focus Assist, full-screen games, GNOME/KDE DND).

use chrono::{Duration, Local, Utc};
use iced::Task;
use std::time::Instant;

use crate::app::{App, Message};
use crate::platform::{self, Notice};

#[derive(Default)]
pub struct NotifyState {
    pub snoozed: Vec<(Instant, Notice)>,
    pub missed: Vec<String>,
    recent: Vec<Notice>,
    last_prune: Option<Instant>,
}

impl NotifyState {
    pub fn remember(&mut self, n: Notice) {
        self.recent.retain(|r| r.key != n.key);
        self.recent.push(n);
        if self.recent.len() > 50 {
            self.recent.remove(0);
        }
    }

    pub fn find(&self, key: &str) -> Option<Notice> {
        self.recent.iter().find(|n| n.key == key).cloned()
    }
}

fn fmt_body(app: &App, o: &crate::model::Occ, minutes_before: i64) -> String {
    let h24 = app.settings.time_24h;
    let when = if o.all_day {
        "All day".to_string()
    } else {
        format!("{} – {}", crate::views::grid::fmt_time(o.start, h24), crate::views::grid::fmt_time(o.end, h24))
    };
    let lead = match minutes_before {
        0 => "Starting now".to_string(),
        m if m < 60 => format!("In {m} min"),
        m if m % 60 == 0 && m < 1440 => format!("In {} h", m / 60),
        m if m >= 1440 => format!("In {} day(s)", m / 1440),
        m => format!("In {} h {} min", m / 60, m % 60),
    };
    let mut s = format!("{lead} · {when}");
    if !o.location.is_empty() {
        s.push_str(" · ");
        s.push_str(&o.location);
    }
    s
}

/// Called on every tick (~20 s).
pub fn tick(app: &mut App) -> Task<Message> {
    let now_ts = Utc::now().timestamp();
    let dnd = app.settings.dnd_active(now_ts);
    if app.settings.dnd_until.is_some() && !dnd {
        // Timed DND ran out.
        app.settings.dnd_until = None;
        app.save_settings();
        report_missed(app);
    }
    platform::update_tray(dnd, matches!(app.sync.status, crate::sync_hook::Status::Error(_)));

    // Day rollover: make sure today's occurrences are loaded.
    app.refresh_occurrences(false);

    if !app.settings.notifications {
        return Task::none();
    }
    let quiet = if dnd {
        Some("Do Not Disturb is on")
    } else if app.settings.respect_system_dnd {
        platform::system_quiet_reason()
    } else {
        None
    };

    let mut tasks = vec![crate::alert_popup::tick(app)];

    // Snoozed alerts.
    let due: Vec<Notice> = {
        let (due, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut app.notify.snoozed).into_iter().partition(|(t, _)| Instant::now() >= *t);
        app.notify.snoozed = keep;
        due.into_iter().map(|(_, n)| n).collect()
    };
    for n in due {
        tasks.push(fire(app, n, quiet));
    }

    let now = Local::now();
    let mut to_fire = Vec::new();
    for o in &app.occs {
        if o.start > now + Duration::days(8) || o.end < now - Duration::hours(1) {
            continue;
        }
        let Some(ev) = app.data.event(&o.key, o.event_index) else { continue };
        for &m in &ev.alarms {
            let at = o.start - Duration::minutes(m);
            // Fire when due; skip alarms that are more than 10 minutes stale (app was closed).
            if at > now || at < now - Duration::minutes(10) {
                continue;
            }
            let key = format!("{}|{}|{}|{m}", o.key.cal_id, o.key.uid, o.key.instance.unwrap_or(0));
            let links = calen_core::links::detect(ev);
            to_fire.push((
                key,
                Notice {
                    key: String::new(),
                    title: o.title.clone(),
                    body: fmt_body(app, o, m.max(0)),
                    join_url: links.meeting.map(|m| m.url),
                },
            ));
        }
    }
    for (key, mut n) in to_fire {
        if app.store.claim_alarm(&key).unwrap_or(false) {
            n.key = key;
            tasks.push(fire(app, n, quiet));
        }
    }

    if app.notify.last_prune.is_none_or(|t| t.elapsed().as_secs() > 3600) {
        let _ = app.store.prune_alarms(60 * 60 * 24 * 30);
        app.notify.last_prune = Some(Instant::now());
    }
    Task::batch(tasks)
}

fn fire(app: &mut App, n: Notice, quiet: Option<&str>) -> Task<Message> {
    app.notify.remember(n.clone());
    if let Some(reason) = quiet {
        log::info!("holding alert for {:?}: {reason}", n.title);
        app.notify.missed.push(n.title.clone());
        return Task::none();
    }
    deliver(app, n)
}

/// Hand an alert to the OS, or show it in-app when the OS won't display it.
fn deliver(app: &mut App, n: Notice) -> Task<Message> {
    match platform::toasts_blocked() {
        Some(reason) => {
            log::info!("{reason}; showing {:?} in-app", n.title);
            crate::alert_popup::show(app, n, Some(reason.to_string()))
        }
        None => {
            log::info!("notifying {:?}", n.title);
            platform::notify(n);
            Task::none()
        }
    }
}

/// The OS failed to show `n` (reported back from the notification thread).
pub fn failed(app: &mut App, n: Notice) -> Task<Message> {
    crate::alert_popup::show(app, n, Some("The system couldn't show the notification".into()))
}

/// `> Notifications: send a test alert`. Ignores Do Not Disturb (it was asked for).
pub fn test(app: &mut App) -> Task<Message> {
    let n =
        Notice { key: "test".into(), title: "Calen test alert".into(), body: "This is how event alerts will look".into(), join_url: None };
    app.notify.remember(n.clone());
    deliver(app, n)
}

pub fn report_missed(app: &mut App) {
    let missed = std::mem::take(&mut app.notify.missed);
    match missed.len() {
        0 => {}
        1 => app.flash(format!("You missed an alert: {}", missed[0])),
        n => app.flash(format!("You missed {n} alerts while notifications were paused")),
    }
}

pub fn snooze(app: &mut App, key: &str) {
    if let Some(n) = app.notify.find(key) {
        app.notify.snoozed.push((Instant::now() + std::time::Duration::from_secs(300), n));
    }
}
