//! Tray flyout: a small borderless always-on-top window with the mini-month
//! and upcoming agenda, shown on tray left-click.

use chrono::Datelike;
use chrono::{Local, NaiveDate};
use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Font, Length, Padding, Size, Task, font, window};
use std::time::Instant;

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
    /// First day of the week picked in the mini-month; None = upcoming agenda.
    pub week: Option<NaiveDate>,
}

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
    app.flyout.week = None;
    app.refresh_occurrences(false);
    let pos = crate::platform::flyout_position(x, y, rect, SIZE);
    app.flyout.shown = true;

    if let Some(id) = app.flyout.id {
        let mut tasks = vec![window::move_to(id, pos), window::set_mode(id, window::Mode::Windowed)];
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

/// A day in the flyout's mini-month was clicked: list that week.
pub fn pick_week(app: &mut App, d: NaiveDate) {
    let start = crate::app::start_of_week(d, app.settings.week_start_day());
    app.flyout.week = if app.flyout.week == Some(start) { None } else { Some(start) };
    let first = crate::app::first_of_month(d);
    if first != app.mini_month {
        app.mini_month = first;
    }
    app.refresh_occurrences(false);
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
    let (list, list_header): (Element<'_, Message>, Option<Element<'_, Message>>) = match app.flyout.week {
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
                button(text("Upcoming").size(11.5).color(p.sidebar_muted))
                    .style(theme::side_ghost)
                    .padding([3, 8])
                    .on_press(Message::FlyoutWeek(None)),
            ]
            .align_y(Alignment::Center);
            (sidebar::agenda(app, p, start, 7, true), Some(container(head).padding([0, 14]).into()))
        }
        None => (sidebar::agenda(app, p, today, 14, false), None),
    };

    let mut body =
        column![container(header).padding([10, 12]), container(sidebar::mini_month(app, p, sidebar::MiniTarget::Flyout)).padding([0, 14])]
            .spacing(6);
    if let Some(h) = list_header {
        body = body.push(h);
    }
    let banner: Element<'_, Message> = match crate::updater::banner(app, p) {
        Some(b) => container(b).center_x(Length::Fill).padding([0, 8]).into(),
        None => Space::new().into(),
    };
    container(
        body.extend([scrollable(container(list).padding(Padding { top: 0.0, right: 14.0, bottom: 8.0, left: 14.0 }))
            .style(theme::side_scroll)
            .height(Length::Fill)
            .into()])
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
