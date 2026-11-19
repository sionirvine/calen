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
    pub id: Option<window::Id>,
    /// Native window handle once known (used to watch clicks outside it).
    pub raw: Option<u64>,
    pub closed_at: Option<Instant>,
    /// First day of the week picked in the mini-month; None = upcoming agenda.
    pub week: Option<NaiveDate>,
}

pub fn is_flyout(app: &App, id: window::Id) -> bool {
    app.flyout.id == Some(id)
}

pub fn is_open(app: &App) -> bool {
    app.flyout.id.is_some()
}

pub fn toggle(app: &mut App, x: f64, y: f64, rect: Option<(f64, f64, f64, f64)>) -> Task<Message> {
    if app.flyout.id.is_some() {
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
    Task::batch([open.discard(), window::gain_focus(id), window::raw_id::<Message>(id).map(move |raw| Message::FlyoutReady(id, raw))])
}

/// The flyout window exists: pull it to the front and close it on outside clicks.
/// Opening from a tray click often leaves it without focus, so focus loss alone
/// isn't a reliable "clicked elsewhere" signal on Windows.
pub fn ready(app: &mut App, id: window::Id, raw: u64) -> Task<Message> {
    if app.flyout.id != Some(id) {
        return Task::none();
    }
    app.flyout.raw = Some(raw);
    crate::platform::force_foreground(raw);
    crate::platform::watch_outside_clicks(raw);
    Task::none()
}

pub fn outside_click(app: &mut App, raw: u64) -> Task<Message> {
    if app.flyout.raw == Some(raw) {
        return close(app);
    }
    Task::none()
}

pub fn close(app: &mut App) -> Task<Message> {
    match app.flyout.id.take() {
        Some(id) => {
            crate::platform::unwatch_outside_clicks();
            app.flyout.raw = None;
            app.flyout.closed_at = Some(Instant::now());
            window::close(id)
        }
        None => Task::none(),
    }
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
        return close(app);
    }
    window::close(id)
}

pub fn unfocused(app: &mut App, id: window::Id) -> Task<Message> {
    if app.flyout.id == Some(id) {
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
    container(
        body.extend([
            scrollable(container(list).padding(Padding { top: 0.0, right: 14.0, bottom: 8.0, left: 14.0 }))
                .style(theme::side_scroll)
                .height(Length::Fill)
                .into(),
            container(footer).padding([6, 8]).into(),
        ]),
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
