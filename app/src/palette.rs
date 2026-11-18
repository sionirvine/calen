//! VS Code–style command palette.
//! - no prefix: fuzzy search over all events (title, location, notes)
//! - `>` prefix: commands and settings

use calen_core::quickadd::DateOrder;
use chrono::{DateTime, Duration, Local};
use iced::keyboard::{Key, Modifiers, key::Named};
use iced::widget::{Space, button, column, container, operation, row, scrollable, text, text_input};
use iced::{Alignment, Element, Font, Length, Task, font};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::app::{App, Message};
use crate::icons;
use crate::model::OccKey;
use crate::settings::{ThemeChoice, ViewKind};
use crate::theme::{self, Palette};

pub const INPUT_ID: &str = "palette-input";
const MAX_RESULTS: usize = 60;

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    ToggleDnd,
    DndFor(i64),
    DndUntilTomorrow,
    Theme(ThemeChoice),
    StartAtLogin(bool),
    CloseToTray(bool),
    Notifications(bool),
    RespectSystemDnd(bool),
    Clock24(bool),
    WeekStart(u8),
    DefaultAlert(Option<i64>),
    DefaultDuration(i64),
    DateOrder(DateOrder),
    DefaultCalendar(String),
    ToggleCalendar(String, bool),
    SyncNow,
    AddAccount,
    RemoveAccount(i64),
    ImageApiKey,
    ImagePublic(bool),
    CheckUpdates,
    AutoUpdate(bool),
    NewEvent,
    Today,
    View(ViewKind),
    ToggleSidebar,
    Shortcuts,
    Quit,
}

#[derive(Debug, Clone)]
pub enum Msg {
    OpenWith(String),
    Query(String),
    Submit,
    Pick(usize),
    Run(Command),
    Close,
}

#[derive(Debug, Clone, PartialEq)]
enum PromptKind {
    ImageApiKey,
}

pub struct PaletteState {
    pub query: String,
    pub selected: usize,
    prompt: Option<PromptKind>,
}

#[derive(Debug, Clone)]
enum Item {
    Cmd { cmd: Command, title: String, detail: String },
    Event { key: OccKey, date: DateTime<Local>, title: String, detail: String, color: iced::Color },
}

pub fn is_open(app: &App) -> bool {
    app.palette_state.is_some()
}

pub fn open(app: &mut App, prefix: &str) -> Task<Message> {
    app.quick = None;
    app.palette_state = Some(PaletteState { query: prefix.to_string(), selected: 0, prompt: None });
    Task::batch([app.open_main(), operation::focus(INPUT_ID), operation::move_cursor_to_end(INPUT_ID)])
}

pub fn close(app: &mut App) -> bool {
    app.palette_state.take().is_some()
}

fn on_off(b: bool) -> &'static str {
    if b { "On" } else { "Off" }
}

fn commands(app: &App) -> Vec<(Command, String, String)> {
    let s = &app.settings;
    let dnd = s.dnd_active(chrono::Utc::now().timestamp());
    let mut v: Vec<(Command, String, String)> = vec![
        (Command::ToggleDnd, format!("Do Not Disturb: turn {}", if dnd { "off" } else { "on" }), "Pause event alerts".into()),
        (Command::DndFor(60), "Do Not Disturb: for 1 hour".into(), String::new()),
        (Command::DndFor(180), "Do Not Disturb: for 3 hours".into(), String::new()),
        (Command::DndUntilTomorrow, "Do Not Disturb: until tomorrow morning".into(), String::new()),
        (Command::NewEvent, "New event".into(), "N / Ctrl+N".into()),
        (Command::Today, "Go to today".into(), "T".into()),
        (Command::View(ViewKind::Day), "View: Day".into(), "D".into()),
        (Command::View(ViewKind::Week), "View: Week".into(), "W".into()),
        (Command::View(ViewKind::Month), "View: Month".into(), "M".into()),
        (Command::ToggleSidebar, "Toggle sidebar".into(), String::new()),
        (Command::SyncNow, "Sync now".into(), String::new()),
        (Command::AddAccount, "Add CalDAV account…".into(), "Nextcloud, Fastmail, iCloud…".into()),
        (Command::Theme(ThemeChoice::System), "Theme: System".into(), current(s.theme == ThemeChoice::System)),
        (Command::Theme(ThemeChoice::Dark), "Theme: Dark".into(), current(s.theme == ThemeChoice::Dark)),
        (Command::Theme(ThemeChoice::Light), "Theme: Light".into(), current(s.theme == ThemeChoice::Light)),
        (
            Command::StartAtLogin(!s.start_at_login),
            format!("Start at login: turn {}", on_off(!s.start_at_login).to_lowercase()),
            format!("Currently {}", on_off(s.start_at_login)),
        ),
        (
            Command::CloseToTray(!s.close_to_tray),
            format!("Close to tray: turn {}", on_off(!s.close_to_tray).to_lowercase()),
            format!("Currently {}", on_off(s.close_to_tray)),
        ),
        (
            Command::Notifications(!s.notifications),
            format!("Event alerts: turn {}", on_off(!s.notifications).to_lowercase()),
            format!("Currently {}", on_off(s.notifications)),
        ),
        (
            Command::RespectSystemDnd(!s.respect_system_dnd),
            format!("Follow system Do Not Disturb / game mode: turn {}", on_off(!s.respect_system_dnd).to_lowercase()),
            format!("Currently {}", on_off(s.respect_system_dnd)),
        ),
        (Command::Clock24(true), "Time format: 24-hour".into(), current(s.time_24h)),
        (Command::Clock24(false), "Time format: 12-hour".into(), current(!s.time_24h)),
        (Command::WeekStart(0), "Week starts on Monday".into(), current(s.week_start == 0)),
        (Command::WeekStart(6), "Week starts on Sunday".into(), current(s.week_start == 6)),
        (Command::WeekStart(5), "Week starts on Saturday".into(), current(s.week_start == 5)),
        (Command::DateOrder(DateOrder::DayMonth), "Typed dates: day/month (18/11)".into(), current(s.date_order == DateOrder::DayMonth)),
        (Command::DateOrder(DateOrder::MonthDay), "Typed dates: month/day (11/18)".into(), current(s.date_order == DateOrder::MonthDay)),
        (
            Command::ImageApiKey,
            "Image upload: set API key…".into(),
            if crate::image_tasks::has_api_key() { "Key saved".into() } else { "Not set".into() },
        ),
        (
            Command::ImagePublic(!s.image_public),
            format!("Image upload: make images {}", if s.image_public { "private" } else { "public" }),
            format!("Currently {}", if s.image_public { "public (links open in other apps)" } else { "private" }),
        ),
        (Command::CheckUpdates, "Check for updates".into(), format!("Version {}", env!("CARGO_PKG_VERSION"))),
        (
            Command::AutoUpdate(!s.auto_update),
            format!("Automatic updates: turn {}", on_off(!s.auto_update).to_lowercase()),
            format!("Currently {}", on_off(s.auto_update)),
        ),
        (Command::Shortcuts, "Keyboard shortcuts".into(), String::new()),
        (Command::Quit, "Quit Calen".into(), String::new()),
    ];
    for m in [None, Some(0), Some(5), Some(10), Some(15), Some(30), Some(60)] {
        let label = match m {
            None => "none".to_string(),
            Some(0) => "at start".to_string(),
            Some(m) => format!("{m} min before"),
        };
        v.push((Command::DefaultAlert(m), format!("Default alert: {label}"), current(s.default_alert == m)));
    }
    for m in [15, 30, 45, 60, 90, 120] {
        v.push((Command::DefaultDuration(m), format!("Default event length: {m} min"), current(s.default_duration == m)));
    }
    for c in &app.data.calendars {
        v.push((
            Command::ToggleCalendar(c.id.clone(), !c.visible),
            format!("{} calendar: {}", if c.visible { "Hide" } else { "Show" }, c.name),
            String::new(),
        ));
        if !c.read_only {
            v.push((
                Command::DefaultCalendar(c.id.clone()),
                format!("Default calendar: {}", c.name),
                current(s.default_calendar.as_deref() == Some(&c.id)),
            ));
        }
    }
    for a in app.store.accounts().unwrap_or_default() {
        v.push((Command::RemoveAccount(a.id), format!("Remove account: {} ({})", a.name, a.username), "Deletes its cached events".into()));
    }
    v
}

fn current(b: bool) -> String {
    if b { "current".into() } else { String::new() }
}

fn items(app: &App, st: &PaletteState) -> Vec<Item> {
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut buf = Vec::new();
    if let Some(q) = st.query.strip_prefix('>') {
        let q = q.trim();
        let pat = Pattern::parse(q, CaseMatching::Ignore, Normalization::Smart);
        let mut scored: Vec<(u32, usize, Command, String, String)> = commands(app)
            .into_iter()
            .enumerate()
            .filter_map(|(i, (c, t, d))| {
                let score = if q.is_empty() { Some(0) } else { pat.score(Utf32Str::new(&t, &mut buf), &mut matcher) }?;
                Some((score, i, c, t, d))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        return scored.into_iter().take(MAX_RESULTS).map(|(_, _, cmd, title, detail)| Item::Cmd { cmd, title, detail }).collect();
    }

    let q = st.query.trim();
    let now = Local::now();
    let pat = Pattern::parse(q, CaseMatching::Ignore, Normalization::Smart);
    let mut found: Vec<(u32, i64, Item)> = Vec::new();
    for ((cal_id, uid), (_, obj)) in &app.data.objects {
        let Some(cal) = app.data.calendar(cal_id) else { continue };
        let Some(master) = obj.master() else { continue };
        let (notes, _) = calen_core::notes::split(&master.description);
        let title = if master.summary.is_empty() { "(No title)".to_string() } else { master.summary.clone() };
        let score = if q.is_empty() {
            0
        } else {
            let t = pat.score(Utf32Str::new(&title, &mut buf), &mut matcher).map(|s| s * 3);
            let l = pat.score(Utf32Str::new(&master.location, &mut buf), &mut matcher);
            let n = pat.score(Utf32Str::new(&notes, &mut buf), &mut matcher).map(|s| s / 2);
            match t.max(l).max(n) {
                Some(s) => s,
                None => continue,
            }
        };
        // Representative date: next occurrence, else the most recent one.
        let next = calen_core::expand(obj, now - Duration::hours(1), now + Duration::days(400)).into_iter().next();
        let (date, idx, rid) = match next {
            Some(o) => (o.start, o.event_index, o.recurrence_id),
            None => (master.start.local(), 0, None),
        };
        if q.is_empty() && date < now {
            continue; // empty query → upcoming only
        }
        let detail = format!(
            "{}{}{}",
            date.format("%a %-d %b %Y"),
            if master.all_day() { String::new() } else { format!(" · {}", crate::views::grid::fmt_time(date, app.settings.time_24h)) },
            if master.location.is_empty() { String::new() } else { format!(" · {}", calen_core::notes::display_location(master)) }
        );
        let _ = idx;
        let distance = (date - now).num_minutes();
        let rank = if distance >= 0 { distance } else { 1_000_000 + distance.abs() };
        found.push((
            score,
            rank,
            Item::Event {
                key: OccKey { cal_id: cal_id.clone(), uid: uid.clone(), instance: rid.as_ref().map(|r| r.key()) },
                date,
                title,
                detail,
                color: theme::parse_hex(&cal.color).unwrap_or(iced::Color::from_rgb8(0x4f, 0x8c, 0xff)),
            },
        ));
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    found.into_iter().take(MAX_RESULTS).map(|(_, _, i)| i).collect()
}

pub fn update(app: &mut App, m: Msg) -> Task<Message> {
    match m {
        Msg::OpenWith(prefix) => return open(app, &prefix),
        Msg::Close => {
            app.palette_state = None;
        }
        Msg::Query(q) => {
            if let Some(st) = &mut app.palette_state {
                st.query = q;
                st.selected = 0;
            }
        }
        Msg::Pick(i) => {
            if let Some(st) = &mut app.palette_state {
                st.selected = i;
            }
            return update(app, Msg::Submit);
        }
        Msg::Submit => {
            let Some(st) = &app.palette_state else { return Task::none() };
            if let Some(PromptKind::ImageApiKey) = st.prompt {
                let key = st.query.trim().to_string();
                app.palette_state = None;
                match crate::image_tasks::save_api_key(&key) {
                    Ok(()) => app.flash(if key.is_empty() { "Image API key removed" } else { "Image API key saved" }),
                    Err(e) => app.flash(format!("Couldn't save the key: {e}")),
                }
                return Task::none();
            }
            let list = items(app, st);
            let Some(item) = list.get(st.selected.min(list.len().saturating_sub(1))).cloned() else { return Task::none() };
            match item {
                Item::Cmd { cmd, .. } => {
                    app.palette_state = None;
                    return run(app, cmd);
                }
                Item::Event { key, date, .. } => {
                    app.palette_state = None;
                    app.jump_to(date.date_naive());
                    app.selected = Some(key);
                    return crate::app::scroll_to_time(date);
                }
            }
        }
        Msg::Run(cmd) => return run(app, cmd),
    }
    Task::none()
}

pub fn run(app: &mut App, cmd: Command) -> Task<Message> {
    let s = &mut app.settings;
    let now = chrono::Utc::now().timestamp();
    let mut task = Task::none();
    match cmd {
        Command::ToggleDnd => {
            if s.dnd_active(now) {
                s.dnd_until = None;
                app.save_settings();
                crate::notify::report_missed(app);
            } else {
                s.dnd_until = Some(i64::MAX);
                app.flash("Do Not Disturb is on");
            }
        }
        Command::DndFor(min) => {
            s.dnd_until = Some(now + min * 60);
            app.flash(format!("Do Not Disturb for {} h", min / 60));
        }
        Command::DndUntilTomorrow => {
            let tomorrow = Local::now().date_naive() + Duration::days(1);
            let t = calen_core::event::local_midnight(tomorrow) + Duration::hours(7);
            s.dnd_until = Some(t.timestamp());
            app.flash("Do Not Disturb until 07:00 tomorrow");
        }
        Command::Theme(t) => s.theme = t,
        Command::StartAtLogin(b) => {
            s.start_at_login = b;
            if let Err(e) = crate::platform::set_autostart(b) {
                app.flash(format!("Couldn't change start at login: {e}"));
            }
        }
        Command::CloseToTray(b) => s.close_to_tray = b,
        Command::Notifications(b) => s.notifications = b,
        Command::RespectSystemDnd(b) => s.respect_system_dnd = b,
        Command::Clock24(b) => s.time_24h = b,
        Command::WeekStart(d) => s.week_start = d,
        Command::DefaultAlert(m) => s.default_alert = m,
        Command::DefaultDuration(m) => s.default_duration = m,
        Command::DateOrder(o) => s.date_order = o,
        Command::DefaultCalendar(id) => s.default_calendar = Some(id),
        Command::ToggleCalendar(id, v) => {
            let _ = app.store.set_calendar_visible(&id, v);
            app.reload();
        }
        Command::SyncNow => task = Task::done(Message::Sync(crate::sync_hook::SyncMsg::Start)),
        Command::AddAccount => task = Task::done(Message::Sync(crate::sync_hook::SyncMsg::OpenDialog)),
        Command::RemoveAccount(id) => {
            if let Some(a) = app.store.accounts().unwrap_or_default().into_iter().find(|a| a.id == id) {
                calen_sync::delete_password(&a.url, &a.username);
            }
            let _ = app.store.remove_account(id);
            app.reload();
            app.flash("Account removed");
            if app.store.accounts().map(|a| a.is_empty()).unwrap_or(true) {
                app.sync.status = crate::sync_hook::Status::NoAccount;
            }
        }
        Command::ImageApiKey => {
            app.palette_state = Some(PaletteState { query: String::new(), selected: 0, prompt: Some(PromptKind::ImageApiKey) });
            return Task::batch([operation::focus(INPUT_ID)]);
        }
        Command::ImagePublic(b) => s.image_public = b,
        Command::CheckUpdates => task = Task::done(Message::Update(crate::updater::UpdateMsg::Check { manual: true })),
        Command::AutoUpdate(b) => s.auto_update = b,
        Command::NewEvent => task = Task::done(Message::OpenQuickAdd(None)),
        Command::Today => task = Task::done(Message::Today),
        Command::View(v) => task = Task::done(Message::SetView(v)),
        Command::ToggleSidebar => task = Task::done(Message::ToggleSidebar),
        Command::Shortcuts => {
            app.show_shortcuts = true;
        }
        Command::Quit => task = iced::exit(),
    }
    app.save_settings();
    task
}

pub fn on_key(app: &mut App, key: &Key, _mods: Modifiers) -> Option<Task<Message>> {
    let st = app.palette_state.as_mut()?;
    match key.as_ref() {
        Key::Named(Named::ArrowDown) => {
            st.selected = st.selected.saturating_add(1);
            let n = items(app, app.palette_state.as_ref().unwrap()).len();
            let st = app.palette_state.as_mut().unwrap();
            st.selected = st.selected.min(n.saturating_sub(1));
            Some(Task::none())
        }
        Key::Named(Named::ArrowUp) => {
            st.selected = st.selected.saturating_sub(1);
            Some(Task::none())
        }
        Key::Named(Named::Escape) => {
            app.palette_state = None;
            Some(Task::none())
        }
        _ => None,
    }
}

pub fn view<'a>(app: &'a App, p: Palette) -> Option<Element<'a, Message>> {
    let st = app.palette_state.as_ref()?;
    let bold = Font { weight: font::Weight::Semibold, ..Font::DEFAULT };
    let (placeholder, secure) = match st.prompt {
        Some(PromptKind::ImageApiKey) => ("Paste the img.felixadr.com API key (imk_…) and press Enter", true),
        None => ("Search events…  (type > for commands & settings)", false),
    };
    let input = text_input(placeholder, &st.query)
        .id(iced::widget::Id::new(INPUT_ID))
        .on_input(|q| Message::Palette(Msg::Query(q)))
        .on_submit(Message::Palette(Msg::Submit))
        .secure(secure)
        .size(16)
        .padding([6, 2])
        .style(theme::bare_input);
    let head = row![icons::icon(if st.query.starts_with('>') { icons::settings() } else { icons::search() }, 16.0, p.muted), input]
        .spacing(10)
        .align_y(Alignment::Center);

    let mut list = column![].spacing(1);
    if st.prompt.is_none() {
        let its = items(app, st);
        let sel = st.selected.min(its.len().saturating_sub(1));
        if its.is_empty() {
            list = list.push(
                container(
                    text(if st.query.starts_with('>') { "No matching commands" } else { "No matching events" }).size(13).color(p.muted),
                )
                .padding([10, 12]),
            );
        }
        for (i, it) in its.into_iter().enumerate() {
            let active = i == sel;
            let content: Element<'a, Message> = match it {
                Item::Cmd { title, detail, .. } => row![
                    text(title).size(13.5).color(p.text).width(Length::Fill),
                    text(detail).size(12).color(if active { p.text } else { p.muted }),
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .into(),
                Item::Event { title, detail, color, .. } => row![
                    container(Space::new().width(8).height(8)).style(theme::dot(color)),
                    column![text(title).size(13.5).font(bold).color(p.text), text(detail).size(12).color(p.muted)].spacing(1),
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .into(),
            };
            list = list.push(
                button(content)
                    .width(Length::Fill)
                    .padding([7, 12])
                    .style(move |t, s| {
                        let mut b = theme::ghost(t, s);
                        if active {
                            b.background = Some(theme::with_alpha(p.today, 0.22).into());
                        }
                        b
                    })
                    .on_press(Message::Palette(Msg::Pick(i))),
            );
        }
    }
    let hint = text(match st.prompt {
        Some(_) => "Enter to save · Esc to cancel",
        None => "↑↓ to move · Enter to open · Esc to close",
    })
    .size(11)
    .color(p.faint);
    Some(
        container(
            column![
                head,
                container(Space::new().height(1).width(Length::Fill)).style(theme::divider),
                scrollable(list).height(Length::Shrink).style(theme::main_scroll),
                hint
            ]
            .spacing(8),
        )
        .padding([12, 12])
        .width(Length::Fixed(620.0))
        .max_height(520)
        .style(theme::popover)
        .into(),
    )
}
