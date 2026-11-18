//! Tray flyout: a small borderless always-on-top window with the mini-month
//! and upcoming agenda, shown on tray left-click.

use chrono::Local;
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
    pub closed_at: Option<Instant>,
}

pub fn is_flyout(app: &App, id: window::Id) -> bool {
    app.flyout.id == Some(id)
}

pub fn is_open(app: &App) -> bool {
    app.flyout.id.is_some()
}

pub fn toggle(app: &mut App, x: f64, y: f64, rect: Option<(f64, f64, f64, f64)>) -> Task<Message> {
    if let Some(id) = app.flyout.id.take() {
        app.flyout.closed_at = Some(Instant::now());
        return window::close(id);
    }
    // The click that unfocused (and closed) us shouldn't immediately reopen.
    if app.flyout.closed_at.is_some_and(|t| t.elapsed().as_millis() < 350) {
        return Task::none();
    }
    app.mini_month = crate::app::first_of_month(Local::now().date_naive());
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
    Task::batch([open.discard(), window::gain_focus(id)])
}

pub fn close(app: &mut App) -> Task<Message> {
    match app.flyout.id.take() {
        Some(id) => {
            app.flyout.closed_at = Some(Instant::now());
            window::close(id)
        }
        None => Task::none(),
    }
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
        text(Local::now().format("%A, %-d %B").to_string()).size(13).font(bold).color(p.sidebar_text),
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

    container(
        column![
            container(header).padding([10, 12]),
            container(sidebar::mini_month(app, p, true)).padding([0, 14]),
            scrollable(container(sidebar::agenda(app, p, Local::now().date_naive(), 14)).padding(Padding {
                top: 0.0,
                right: 14.0,
                bottom: 8.0,
                left: 14.0
            }))
            .style(theme::side_scroll)
            .height(Length::Fill),
            container(footer).padding([6, 8]),
        ]
        .spacing(6),
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
