//! Tray flyout: a small borderless always-on-top window with the mini-month
//! and upcoming agenda, shown on tray left-click.

use chrono::Datelike;
use chrono::{Local, NaiveDate};
use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::{Space, button, column, container, operation, row, scrollable, sensor, text};
use iced::{Alignment, Element, Font, Length, Padding, Size, Task, font, window};
use std::time::{Duration, Instant};

use crate::app::{App, Message};
use crate::icons;
use crate::theme;
use crate::views::sidebar;

pub const SIZE: Size = Size { width: 360.0, height: 640.0 };

#[derive(Default)]
pub struct FlyoutState {
    /// The flyout window. Created on first use, then hidden/shown rather than
    /// destroyed (destroying it flashed white and rebuilt it on every open).
    pub id: Option<window::Id>,
    /// Native window handle once known (focus + watching clicks outside it).
    pub raw: Option<u64>,
    /// Currently on screen.
    pub shown: bool,
    pub closed_at: Option<Instant>,
    /// First day of the week listed; None = upcoming agenda.
    pub week: Option<NaiveDate>,
    /// Measured heights: the days of this week before today, today onward, and
    /// the visible list. Used to open the list with today at the top.
    pub past_h: Option<f32>,
    pub tail_h: Option<f32>,
    pub view_h: Option<f32>,
    /// Until then, new measurements re-apply the scroll to today.
    pub scroll_until: Option<Instant>,
}

/// A part of the flyout list whose height was measured.
#[derive(Debug, Clone, Copy)]
pub enum Part {
    Past,
    Tail,
    Viewport,
}

const LIST_SCROLL: &str = "flyout-list";

const FOCUS_ATTEMPTS: u32 = 12;

pub fn is_flyout(app: &App, id: window::Id) -> bool {
    app.flyout.id == Some(id)
}

pub fn is_open(app: &App) -> bool {
    app.flyout.shown
}

pub fn toggle(app: &mut App, x: f64, y: f64, rect: Option<(f64, f64, f64, f64)>) -> Task<Message> {
    if app.flyout.shown {
        return close(app);
    }
    // The click that unfocused (and closed) us shouldn't immediately reopen.
    if app.flyout.closed_at.is_some_and(|t| t.elapsed().as_millis() < 350) {
        return Task::none();
    }
    app.mini_month = crate::app::first_of_month(Local::now().date_naive());
    app.flyout.week = Some(this_week(app));
    app.refresh_occurrences(false);
    let scroll = scroll_to_today(app);
    let pos = crate::platform::flyout_position(x, y, rect, SIZE);
    app.flyout.shown = true;

    if let Some(id) = app.flyout.id {
        let mut tasks = vec![window::move_to(id, pos), window::set_mode(id, window::Mode::Windowed), scroll];
        if let Some(raw) = app.flyout.raw {
            crate::platform::watch_outside_clicks(raw);
            tasks.push(focus_later(id, 0));
        }
        return Task::batch(tasks);
    }

    let (id, open) = window::open(window::Settings {
        size: SIZE,
        position: window::Position::Specific(pos),
        resizable: false,
        decorations: false,
        level: window::Level::AlwaysOnTop,
        exit_on_close_request: false,
        icon: crate::platform::window_icon(),
        platform_specific: crate::platform::flyout_platform(),
        ..Default::default()
    });
    app.flyout.id = Some(id);
    app.flyout.raw = None;
    // Ask for the native handle only once the window exists; asked earlier, the
    // request finds no window and silently yields nothing.
    open.then(|id| window::raw_id::<Message>(id).map(move |raw| Message::FlyoutReady(id, raw)))
}

fn this_week(app: &App) -> NaiveDate {
    crate::app::start_of_week(Local::now().date_naive(), app.settings.week_start_day())
}

/// Days of `week` before today, or None when the week doesn't contain today.
fn days_before_today(week: NaiveDate) -> Option<i64> {
    let n = (Local::now().date_naive() - week).num_days();
    (0..7).contains(&n).then_some(n)
}

/// Scroll the list so today's heading is the first line (the top for other
/// weeks and the upcoming list), and keep doing so for a moment while the
/// parts are measured.
fn scroll_to_today(app: &mut App) -> Task<Message> {
    app.flyout.scroll_until = Some(Instant::now() + Duration::from_millis(800));
    apply_scroll(app)
}

fn apply_scroll(app: &App) -> Task<Message> {
    let y = match app.flyout.week.and_then(days_before_today) {
        Some(n) if n > 0 => app.flyout.past_h.unwrap_or(0.0),
        _ => 0.0,
    };
    operation::scroll_to(LIST_SCROLL, AbsoluteOffset { x: 0.0, y })
}

pub fn measured(app: &mut App, part: Part, h: f32) -> Task<Message> {
    let slot = match part {
        Part::Past => &mut app.flyout.past_h,
        Part::Tail => &mut app.flyout.tail_h,
        Part::Viewport => &mut app.flyout.view_h,
    };
    *slot = Some(h);
    if app.flyout.shown && app.flyout.scroll_until.is_some_and(|t| Instant::now() < t) {
        return apply_scroll(app);
    }
    Task::none()
}

/// List the week of `day` (None = the upcoming list).
pub fn show_week(app: &mut App, day: Option<NaiveDate>) -> Task<Message> {
    app.flyout.week = day.map(|d| crate::app::start_of_week(d, app.settings.week_start_day()));
    if let Some(d) = day {
        let first = crate::app::first_of_month(d);
        if first != app.mini_month {
            app.mini_month = first;
        }
    }
    app.refresh_occurrences(false);
    scroll_to_today(app)
}

/// The flyout window exists: watch clicks outside it and pull it to the front.
pub fn ready(app: &mut App, id: window::Id, raw: u64) -> Task<Message> {
    if app.flyout.id != Some(id) {
        return Task::none();
    }
    app.flyout.raw = Some(raw);
    if !app.flyout.shown {
        return Task::none();
    }
    crate::platform::watch_outside_clicks(raw);
    focus_later(id, 0)
}

fn focus_later(id: window::Id, attempt: u32) -> Task<Message> {
    Task::perform(tokio::time::sleep(std::time::Duration::from_millis(50)), move |_| Message::FlyoutFocus(id, attempt))
}

/// Keep asking for focus until the window is actually visible and focused. iced
/// shows new windows only after their first frame, and Windows ignores focus
/// requests for hidden windows, so a single early request isn't enough.
pub fn focus(app: &mut App, id: window::Id, attempt: u32) -> Task<Message> {
    let (Some(raw), true) = (app.flyout.raw, app.flyout.shown && app.flyout.id == Some(id)) else {
        return Task::none();
    };
    if crate::platform::try_focus(raw) {
        log::debug!("flyout focused after {} attempt(s)", attempt + 1);
        return Task::none();
    }
    if attempt + 1 < FOCUS_ATTEMPTS {
        return focus_later(id, attempt + 1);
    }
    log::warn!("flyout: couldn't take focus; closing relies on the click watcher");
    Task::none()
}

/// Seconds a hidden flyout is kept for instant reopening before it's destroyed.
const KEEP_HIDDEN_SECS: u64 = 60;

/// Called on the app tick: destroy a flyout that has been hidden for a while,
/// so an idle Calen in the tray doesn't hold its surface. It's recreated on the
/// next click (hidden until its first frame, so there's no flash).
pub fn idle(app: &mut App) -> Task<Message> {
    let stale = app.flyout.closed_at.is_some_and(|t| t.elapsed().as_secs() >= KEEP_HIDDEN_SECS);
    match app.flyout.id {
        Some(id) if !app.flyout.shown && stale => {
            app.flyout.id = None;
            app.flyout.raw = None;
            Task::batch([window::close(id), crate::app::trim_memory_later()])
        }
        _ => Task::none(),
    }
}

pub fn outside_click(app: &mut App, raw: u64) -> Task<Message> {
    if app.flyout.shown && app.flyout.raw == Some(raw) {
        log::info!("flyout: outside click, closing");
        return close(app);
    }
    Task::none()
}

/// Hide the flyout (fading out where supported). The window is kept for next time.
pub fn close(app: &mut App) -> Task<Message> {
    let Some(id) = app.flyout.id.filter(|_| app.flyout.shown) else { return Task::none() };
    app.flyout.shown = false;
    app.flyout.closed_at = Some(Instant::now());
    crate::platform::unwatch_outside_clicks();
    if let Some(raw) = app.flyout.raw {
        crate::platform::fade_out(raw);
    }
    window::set_mode(id, window::Mode::Hidden)
}

pub fn close_requested(app: &mut App, id: window::Id) -> Task<Message> {
    if app.flyout.id == Some(id) {
        // Alt+F4 hides it like any other dismissal.
        return close(app);
    }
    window::close(id)
}

pub fn unfocused(app: &mut App, id: window::Id) -> Task<Message> {
    if app.flyout.id == Some(id) && app.flyout.shown {
        return close(app);
    }
    Task::none()
}

pub fn view(app: &App) -> Element<'_, Message> {
    let p = app.palette();
    let bold = Font { weight: font::Weight::Bold, ..Font::DEFAULT };
    let dnd = app.settings.dnd_active(chrono::Utc::now().timestamp());
    let header = row![
        icons::icon(icons::calendar(), 16.0, p.sidebar_muted),
        button(text(Local::now().format("%A, %-d %B").to_string()).size(13).font(bold).color(p.sidebar_text))
            .style(theme::side_ghost)
            .padding([2, 4])
            .on_press(Message::FlyoutWeek(None)),
        Space::new().width(Length::Fill),
        crate::sync_hook::status_dot(app, p),
        button(icons::icon(icons::plus(), 16.0, p.sidebar_text)).style(theme::side_ghost).padding(5).on_press(Message::FlyoutNew),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let footer = row![
        button(
            row![
                icons::icon(if dnd { icons::bell_off() } else { icons::bell() }, 14.0, if dnd { p.accent } else { p.sidebar_muted }),
                text(if dnd { "Do Not Disturb" } else { "Alerts on" }).size(12).color(if dnd { p.accent } else { p.sidebar_muted })
            ]
            .spacing(6)
            .align_y(Alignment::Center)
        )
        .style(theme::side_ghost)
        .padding([5, 6])
        .on_press(Message::Palette(crate::palette::Msg::Run(crate::palette::Command::ToggleDnd))),
        Space::new().width(Length::Fill),
        button(text("Open Calen").size(12).color(p.sidebar_text)).style(theme::side_ghost).padding([5, 8]).on_press(Message::ShowMain),
    ]
    .align_y(Alignment::Center);

    let today = Local::now().date_naive();
    let (list, head): (Element<'_, Message>, Element<'_, Message>) = match app.flyout.week {
        Some(start) => {
            let end = start + chrono::Duration::days(6);
            let label = if start.month() == end.month() {
                format!("Week of {} – {}", start.format("%-d"), end.format("%-d %B"))
            } else {
                format!("Week of {} – {}", start.format("%-d %b"), end.format("%-d %b"))
            };
            let head = row![
                text(label).size(12).font(bold).color(p.sidebar_text),
                Space::new().width(Length::Fill),
                crate::views::underlined_link("Upcoming", 11.5, p.sidebar_muted, Message::FlyoutWeek(None)),
            ]
            .align_y(Alignment::Center);
            (week_list(app, p, start), head.into())
        }
        None => {
            let head = row![
                text("Upcoming").size(12).font(bold).color(p.sidebar_text),
                Space::new().width(Length::Fill),
                crate::views::underlined_link("This week", 11.5, p.sidebar_muted, Message::FlyoutWeek(Some(today))),
            ]
            .align_y(Alignment::Center);
            (sidebar::agenda(app, p, today, 14, false, Some("No upcoming events")), head.into())
        }
    };

    let body = column![
        container(header).padding([10, 12]),
        container(sidebar::mini_month(app, p, sidebar::MiniTarget::Flyout)).padding([0, 14]),
        container(head).padding([0, 14]),
    ]
    .spacing(6);
    let banner: Element<'_, Message> = match crate::updater::banner(app, p) {
        Some(b) => container(b).center_x(Length::Fill).padding([0, 8]).into(),
        None => Space::new().into(),
    };
    container(
        body.push(
            sensor(
                scrollable(container(list).padding(Padding { top: 0.0, right: 14.0, bottom: 8.0, left: 14.0 }))
                    .id(iced::widget::Id::new(LIST_SCROLL))
                    .style(theme::side_scroll)
                    .height(Length::Fill),
            )
            .on_show(|s| Message::FlyoutMeasured(Part::Viewport, s.height))
            .on_resize(|s| Message::FlyoutMeasured(Part::Viewport, s.height)),
        )
        .push(banner)
        .push(container(footer).padding([6, 8])),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |t| {
        let mut s = theme::sidebar(t);
        s.border = iced::Border { color: theme::with_alpha(p.sidebar_muted, 0.25), width: 1.0, radius: 10.0.into() };
        s
    })
    .into()
}

/// The listed week: the days before today, then today onward, each measured so
/// the list can open with today at the top. A spacer below lets the list scroll
/// that far even when the rest of the week is short.
fn week_list(app: &App, p: theme::Palette, start: NaiveDate) -> Element<'_, Message> {
    let (past, tail, room): (Element<'_, Message>, Element<'_, Message>, f32) = match days_before_today(start) {
        Some(n) => {
            let past = if n > 0 { sidebar::agenda(app, p, start, n, true, None) } else { Space::new().into() };
            let tail = sidebar::agenda(app, p, start + chrono::Duration::days(n), 7 - n, true, None);
            let room = match (app.flyout.view_h, app.flyout.tail_h) {
                // 8 = the list's bottom padding.
                (Some(v), Some(t)) if n > 0 => (v - t - 8.0).max(0.0),
                _ => 0.0,
            };
            (past, tail, room)
        }
        None => (Space::new().into(), sidebar::agenda(app, p, start, 7, true, Some("Nothing this week")), 0.0),
    };
    // Same structure for every week, so the sensors keep their state.
    column![
        sensor(past)
            .on_show(|s| Message::FlyoutMeasured(Part::Past, s.height))
            .on_resize(|s| Message::FlyoutMeasured(Part::Past, s.height)),
        sensor(tail)
            .on_show(|s| Message::FlyoutMeasured(Part::Tail, s.height))
            .on_resize(|s| Message::FlyoutMeasured(Part::Tail, s.height)),
        Space::new().height(room),
    ]
    .into()
}
