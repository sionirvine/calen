//! Glue between the app and the CalDAV sync engine: scheduling, offline
//! back-off, the toolbar status pill and the "connect account" dialog.

use chrono::{DateTime, Local};
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::widget::{Space, button, canvas as canvas_w, column, container, row, text, text_input, tooltip};
use iced::{Alignment, Element, Font, Length, Rectangle, Renderer, Subscription, Task, Theme, font};
use std::time::{Duration, Instant};

use crate::app::{App, Message};
use crate::icons;
use crate::theme::{self, Palette};

const BOLD: Font = Font { weight: font::Weight::Semibold, ..Font::DEFAULT };

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    NoAccount,
    Idle,
    Syncing,
    Synced(DateTime<Local>),
    Offline,
    Error(String),
}

pub struct SyncState {
    pub status: Status,
    pub dirty_since: Option<Instant>,
    pub retry_at: Option<Instant>,
    pub backoff: u64,
    /// When the current sync started (drives the spinner).
    pub started: Instant,
    pub queued_again: bool,
    pub last_report: Option<calen_sync::Report>,
    pub dialog: Option<AccountDialog>,
}

impl Default for SyncState {
    fn default() -> Self {
        SyncState {
            status: Status::Idle,
            dirty_since: None,
            retry_at: None,
            backoff: 15,
            started: Instant::now(),
            queued_again: false,
            last_report: None,
            dialog: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AccountDialog {
    pub url: String,
    pub user: String,
    pub pass: String,
    pub busy: bool,
    pub error: Option<String>,
    /// Accounts from GNOME Online Accounts that aren't in Calen yet.
    pub goa: Vec<crate::platform::GoaAccount>,
}

#[derive(Debug, Clone)]
pub enum SyncMsg {
    Start,
    Check,
    Frame,
    Done(Result<calen_sync::Report, (bool, String)>),
    OpenDialog,
    Url(String),
    User(String),
    Pass(String),
    Connect,
    Tested(Result<usize, String>),
    GoaListed(Vec<crate::platform::GoaAccount>),
    /// Use this GNOME Online Accounts account (index into `AccountDialog::goa`).
    GoaPick(usize),
    GoaPassword(Result<String, String>),
    CloseDialog,
}

pub fn local_changed(app: &mut App) {
    app.sync.dirty_since = Some(Instant::now());
}

pub fn boot(app: &mut App) -> Task<Message> {
    if app.store.accounts().map(|a| a.is_empty()).unwrap_or(true) {
        app.sync.status = Status::NoAccount;
        return Task::none();
    }
    Task::done(Message::Sync(SyncMsg::Start))
}

pub fn pending(app: &App) -> i64 {
    app.store.pending_count().unwrap_or(0)
}

fn start(app: &mut App) -> Task<Message> {
    if app.sync.status == Status::Syncing {
        app.sync.queued_again = true;
        return Task::none();
    }
    let accounts = app.store.accounts().unwrap_or_default();
    if accounts.is_empty() {
        app.sync.status = Status::NoAccount;
        return Task::none();
    }
    // Look the passwords up here, on the UI thread. The keyring talks to the
    // Secret Service through zbus' blocking API, which panics inside a tokio
    // runtime (zbus is built with its `tokio` feature), so it must not run in
    // the async task below.
    let mut creds = Vec::with_capacity(accounts.len());
    for acc in accounts {
        let Some(pass) = calen_sync::password_for(&acc) else {
            let e = format!("No saved password for {}", acc.name);
            log::warn!("sync failed: {e}");
            app.sync.status = Status::Error(e);
            app.sync.dirty_since = None;
            app.sync.retry_at = Some(Instant::now() + Duration::from_secs(app.sync.backoff.max(60)));
            app.sync.backoff = (app.sync.backoff * 2).min(600);
            return Task::none();
        };
        creds.push((acc, pass));
    }
    app.sync.status = Status::Syncing;
    app.sync.started = Instant::now();
    app.sync.dirty_since = None;
    app.sync.retry_at = None;
    let db = app.data_dir.join("calen.db");
    let job = async move {
        let mut total = calen_sync::Report::default();
        for (acc, pass) in creds {
            match calen_sync::sync_account(&db, acc, pass).await {
                Ok(r) => {
                    total.pushed += r.pushed;
                    total.pulled += r.pulled;
                    total.deleted += r.deleted;
                    total.conflicts += r.conflicts;
                    total.calendars += r.calendars;
                }
                Err(calen_sync::DavError::Offline(e)) => return Err((true, e)),
                Err(e) => return Err((false, e.to_string())),
            }
        }
        Ok(total)
    };
    Task::perform(
        // Run it as its own task so a panic comes back as an error instead of
        // leaving the pill on "Syncing…" forever.
        async move { tokio::spawn(job).await.unwrap_or_else(|e| Err((false, format!("sync crashed: {e}")))) },
        |r| Message::Sync(SyncMsg::Done(r)),
    )
}

pub fn update(app: &mut App, msg: SyncMsg) -> Task<Message> {
    match msg {
        SyncMsg::Start => return start(app),
        SyncMsg::Check => {
            let due_dirty = app.sync.dirty_since.is_some_and(|t| t.elapsed() >= Duration::from_secs(2));
            let due_retry = app.sync.retry_at.is_some_and(|t| Instant::now() >= t);
            if (due_dirty || due_retry) && app.sync.status != Status::NoAccount {
                return start(app);
            }
        }
        SyncMsg::Frame => {}
        SyncMsg::Done(result) => {
            match result {
                Ok(report) => {
                    if report.conflicts > 0 {
                        app.flash(format!("{} event(s) changed elsewhere — kept your version as a copy", report.conflicts));
                    }
                    app.sync.status = Status::Synced(Local::now());
                    app.sync.backoff = 15;
                    app.sync.last_report = Some(report);
                }
                Err((true, e)) => {
                    log::info!("offline: {e}");
                    app.sync.status = Status::Offline;
                    app.sync.retry_at = Some(Instant::now() + Duration::from_secs(app.sync.backoff));
                    app.sync.backoff = (app.sync.backoff * 2).min(300);
                }
                Err((false, e)) => {
                    log::warn!("sync failed: {e}");
                    app.sync.status = Status::Error(e);
                    app.sync.retry_at = Some(Instant::now() + Duration::from_secs(app.sync.backoff.max(60)));
                    app.sync.backoff = (app.sync.backoff * 2).min(600);
                }
            }
            app.reload();
            if std::mem::take(&mut app.sync.queued_again) || app.sync.dirty_since.is_some() {
                app.sync.dirty_since = Some(Instant::now());
            }
        }
        SyncMsg::OpenDialog => {
            app.sync.dialog = Some(AccountDialog::default());
            let goa = Task::perform(
                async { tokio::task::spawn_blocking(crate::platform::goa_accounts).await.unwrap_or_default() },
                |a| Message::Sync(SyncMsg::GoaListed(a)),
            );
            return Task::batch([iced::widget::operation::focus("acct-url"), goa]);
        }
        SyncMsg::GoaListed(list) => {
            let known = app.store.accounts().unwrap_or_default();
            if let Some(d) = &mut app.sync.dialog {
                d.goa = list
                    .into_iter()
                    .filter(|g| !known.iter().any(|a| a.url == normalize_url(&g.uri) && a.username == g.username))
                    .collect();
            }
        }
        SyncMsg::GoaPick(i) => {
            let Some(d) = &mut app.sync.dialog else { return Task::none() };
            let Some(g) = d.goa.get(i).filter(|g| g.supported).cloned() else { return Task::none() };
            d.url = g.uri.clone();
            d.user = g.username.clone();
            d.pass.clear();
            d.busy = true;
            d.error = None;
            return Task::perform(
                async move { tokio::task::spawn_blocking(move || crate::platform::goa_password(&g.path)).await.map_err(|e| e.to_string())? },
                |r| Message::Sync(SyncMsg::GoaPassword(r)),
            );
        }
        SyncMsg::GoaPassword(r) => {
            let Some(d) = &mut app.sync.dialog else { return Task::none() };
            d.busy = false;
            match r {
                Ok(pass) if !pass.is_empty() => {
                    d.pass = pass;
                    return update(app, SyncMsg::Connect);
                }
                Ok(_) => d.error = Some("GNOME Online Accounts has no password for this account".into()),
                Err(e) => d.error = Some(format!("Couldn't get the password from GNOME Online Accounts: {e}")),
            }
        }
        SyncMsg::Url(s) => with_dialog(app, |d| d.url = s),
        SyncMsg::User(s) => with_dialog(app, |d| d.user = s),
        SyncMsg::Pass(s) => with_dialog(app, |d| d.pass = s),
        SyncMsg::CloseDialog => app.sync.dialog = None,
        SyncMsg::Connect => {
            let Some(d) = &mut app.sync.dialog else { return Task::none() };
            if d.url.trim().is_empty() || d.user.trim().is_empty() || d.pass.is_empty() {
                d.error = Some("Fill in the server, username and app password".into());
                return Task::none();
            }
            d.busy = true;
            d.error = None;
            let (url, user, pass) = (d.url.trim().to_string(), d.user.trim().to_string(), d.pass.clone());
            return Task::perform(
                async move { calen_sync::test_account(url, user, pass).await.map(|c| c.len()).map_err(|e| e.to_string()) },
                |r| Message::Sync(SyncMsg::Tested(r)),
            );
        }
        SyncMsg::Tested(r) => {
            let Some(d) = &mut app.sync.dialog else { return Task::none() };
            d.busy = false;
            match r {
                Err(e) => d.error = Some(e),
                Ok(0) => d.error = Some("Connected, but no calendars were found".into()),
                Ok(n) => {
                    let d = app.sync.dialog.take().unwrap();
                    let url = normalize_url(&d.url);
                    if let Err(e) = calen_sync::save_password(&url, d.user.trim(), &d.pass) {
                        app.flash(format!("Couldn't store the password securely: {e}"));
                        return Task::none();
                    }
                    let name =
                        url.trim_start_matches("https://").trim_start_matches("http://").split('/').next().unwrap_or("CalDAV").to_string();
                    if let Err(e) = app.store.add_account(&name, &url, d.user.trim()) {
                        app.flash(format!("Couldn't save the account: {e}"));
                        return Task::none();
                    }
                    app.flash(format!("Connected · {n} calendars"));
                    app.sync.status = Status::Idle;
                    return start(app);
                }
            }
        }
    }
    Task::none()
}

fn normalize_url(s: &str) -> String {
    let s = s.trim();
    if s.starts_with("http://") || s.starts_with("https://") { s.to_string() } else { format!("https://{s}") }
}

fn with_dialog(app: &mut App, f: impl FnOnce(&mut AccountDialog)) {
    if let Some(d) = &mut app.sync.dialog {
        f(d);
        d.error = None;
    }
}

pub fn subscription(app: &App) -> Subscription<Message> {
    let mut subs = vec![iced::time::every(Duration::from_secs(300)).map(|_| Message::Sync(SyncMsg::Start))];
    if app.sync.dirty_since.is_some() || app.sync.retry_at.is_some() {
        subs.push(iced::time::every(Duration::from_millis(1000)).map(|_| Message::Sync(SyncMsg::Check)));
    }
    if app.sync.status == Status::Syncing && app.main_visible {
        subs.push(iced::time::every(Duration::from_millis(33)).map(|_| Message::Sync(SyncMsg::Frame)));
    }
    Subscription::batch(subs)
}

// ---------------------------------------------------------------------------
// view

struct Spinner {
    phase: f32,
    color: iced::Color,
    track: iced::Color,
}

impl canvas::Program<Message> for Spinner {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: Cursor) -> Vec<Geometry> {
        const WIDTH: f32 = 2.0;
        let mut frame = Frame::new(renderer, bounds.size());
        let c = frame.center();
        // Keep the whole stroke inside the canvas, with a little room for anti-aliasing.
        let r = bounds.width.min(bounds.height) / 2.0 - WIDTH / 2.0 - 0.5;
        // Built from explicit points rather than `Path::arc`: the CPU renderer
        // drew the arc with a stray segment and a misshapen cap.
        let polyline = |start: f32, sweep: f32, closed: bool| {
            let steps = ((sweep / std::f32::consts::TAU) * 32.0).ceil().max(2.0) as usize;
            Path::new(|b| {
                for i in 0..=steps {
                    let t = start + sweep * i as f32 / steps as f32;
                    let pt = iced::Point::new(c.x + r * t.cos(), c.y + r * t.sin());
                    if i == 0 { b.move_to(pt) } else { b.line_to(pt) }
                }
                if closed {
                    b.close();
                }
            })
        };
        let stroke = |color| Stroke::default().with_color(color).with_width(WIDTH);
        frame.stroke(&polyline(0.0, std::f32::consts::TAU, true), stroke(self.track));
        frame.stroke(&polyline(self.phase, 1.9, false), stroke(self.color).with_line_join(canvas::LineJoin::Round));
        vec![frame.into_geometry()]
    }
}

/// Spinner angle: one turn per second since the sync started.
fn spinner_phase(app: &App) -> f32 {
    (app.sync.started.elapsed().as_secs_f32() * std::f32::consts::TAU) % std::f32::consts::TAU
}

fn ago(t: DateTime<Local>) -> String {
    let s = (Local::now() - t).num_seconds();
    match s {
        s if s < 60 => "just now".into(),
        s if s < 3600 => format!("{} min ago", s / 60),
        s if s < 86400 => format!("{} h ago", s / 3600),
        _ => t.format("%-d %b").to_string(),
    }
}

pub fn status_pill<'a>(app: &'a App, p: Palette) -> Element<'a, Message> {
    let n = pending(app);
    let (icon, label, color, tip): (Element<'a, Message>, String, iced::Color, String) = match &app.sync.status {
        Status::NoAccount => {
            return button(
                row![icons::icon(icons::cloud(), 14.0, p.muted), text("Connect calendar").size(12).color(p.muted)]
                    .spacing(6)
                    .align_y(Alignment::Center),
            )
            .style(theme::ghost)
            .padding([4, 8])
            .on_press(Message::Sync(SyncMsg::OpenDialog))
            .into();
        }
        Status::Syncing => (
            canvas_w(Spinner { phase: spinner_phase(app), color: p.today, track: theme::with_alpha(p.today, 0.2) })
                .width(14)
                .height(14)
                .into(),
            "Syncing…".into(),
            p.today,
            "Syncing with your CalDAV server".into(),
        ),
        Status::Synced(t) if n == 0 => (
            icons::icon(icons::check(), 14.0, p.success).into(),
            format!("Synced {}", ago(*t)),
            p.muted,
            format!("Last synced {}", t.format("%H:%M:%S")),
        ),
        Status::Synced(_) | Status::Idle => (
            icons::icon(icons::cloud(), 14.0, p.muted).into(),
            if n > 0 { format!("{n} to sync") } else { "Ready".into() },
            p.muted,
            "Waiting to sync".into(),
        ),
        Status::Offline => (
            icons::icon(icons::cloud_off(), 14.0, p.muted).into(),
            if n > 0 { format!("Offline · {n} pending") } else { "Offline".into() },
            p.muted,
            "Can't reach the server. Your changes are saved and will sync automatically.".into(),
        ),
        Status::Error(e) => (icons::icon(icons::alert(), 14.0, p.danger).into(), "Sync problem".into(), p.danger, e.clone()),
    };
    let pill = button(row![icon, text(label).size(12).color(color)].spacing(6).align_y(Alignment::Center))
        .style(move |t, s| {
            let mut st = theme::ghost(t, s);
            if !matches!(s, button::Status::Hovered | button::Status::Pressed) {
                st.background = Some(theme::with_alpha(p.surface_alt, 0.6).into());
            }
            st.border.radius = 999.0.into();
            st
        })
        .padding([4, 10])
        .on_press(Message::Sync(SyncMsg::Start));
    tooltip(pill, container(text(tip).size(12)).padding([6, 10]).max_width(320).style(theme::popover), tooltip::Position::Bottom).into()
}

pub fn dialog<'a>(app: &'a App, p: Palette) -> Option<Element<'a, Message>> {
    let d = app.sync.dialog.as_ref()?;
    let field = |label: &'static str, placeholder: &'static str, value: &str, f: fn(String) -> SyncMsg, secure: bool, id: &'static str| {
        column![
            text(label).size(12).color(p.muted),
            text_input(placeholder, value)
                .id(iced::widget::Id::new(id))
                .on_input(move |s| Message::Sync(f(s)))
                .on_submit(Message::Sync(SyncMsg::Connect))
                .secure(secure)
                .style(theme::input)
                .size(14)
                .padding([7, 10]),
        ]
        .spacing(4)
    };
    let mut footer = row![].spacing(8).align_y(Alignment::Center);
    if let Some(e) = &d.error {
        footer = footer.push(text(e.clone()).size(12).color(p.danger).width(Length::Fill));
    } else {
        footer = footer.push(Space::new().width(Length::Fill));
    }
    footer = footer
        .push(button(text("Cancel").size(13)).style(theme::ghost).padding([6, 12]).on_press(Message::Sync(SyncMsg::CloseDialog)))
        .push({
            let b = button(text(if d.busy { "Connecting…" } else { "Connect" }).size(13)).style(theme::primary).padding([6, 14]);
            if d.busy { b } else { b.on_press(Message::Sync(SyncMsg::Connect)) }
        });
    // Zero height (no GOA accounts) leaves the column without it, gap included.
    let mut goa = column![].spacing(6).height(if d.goa.is_empty() { Length::Fixed(0.0) } else { Length::Shrink });
    if !d.goa.is_empty() {
        goa = goa.push(text("From GNOME Online Accounts").size(12).color(p.muted));
        for (i, g) in d.goa.iter().enumerate() {
            let label = if g.label.is_empty() { g.username.clone() } else { g.label.clone() };
            let line = row![
                text(g.provider.clone()).size(13).font(BOLD).color(if g.supported { p.text } else { p.muted }),
                text(label).size(13).color(p.muted).width(Length::Fill),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            goa = goa.push(if g.supported {
                let b = button(line.push(text("Use").size(12).color(p.today))).style(theme::ghost).padding([6, 8]).width(Length::Fill);
                if d.busy { b } else { b.on_press(Message::Sync(SyncMsg::GoaPick(i))) }
            } else {
                button(line.push(text("Not supported yet").size(12).color(p.muted))).style(theme::ghost).padding([6, 8]).width(Length::Fill)
            });
        }
        goa = goa.push(text("Or enter a server:").size(12).color(p.muted));
    }
    Some(
        container(
            column![
                text("Connect a CalDAV account").size(17).font(BOLD),
                text("For Nextcloud, create an app password under Settings → Security → Devices & sessions.").size(12).color(p.muted),
                goa,
                field("Server", "cloud.example.com", &d.url, SyncMsg::Url, false, "acct-url"),
                field("Username", "you", &d.user, SyncMsg::User, false, "acct-user"),
                field("App password", "xxxxx-xxxxx-xxxxx-xxxxx-xxxxx", &d.pass, SyncMsg::Pass, true, "acct-pass"),
                footer,
            ]
            .spacing(12),
        )
        .padding(20)
        .width(Length::Fixed(440.0))
        .style(theme::popover)
        .into(),
    )
}

/// Compact status for the tray flyout header.
pub fn status_dot<'a>(app: &'a App, p: Palette) -> Element<'a, Message> {
    let (icon, color) = match &app.sync.status {
        Status::NoAccount => return Space::new().into(),
        Status::Syncing => {
            return canvas_w(Spinner { phase: spinner_phase(app), color: p.today, track: theme::with_alpha(p.today, 0.2) })
                .width(14)
                .height(14)
                .into();
        }
        Status::Synced(_) | Status::Idle => (icons::check(), p.success),
        Status::Offline => (icons::cloud_off(), p.sidebar_muted),
        Status::Error(_) => (icons::alert(), p.danger),
    };
    button(icons::icon(icon, 14.0, color)).style(theme::side_ghost).padding(4).on_press(Message::Sync(SyncMsg::Start)).into()
}
