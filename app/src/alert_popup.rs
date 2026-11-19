//! In-app reminder popups, used when the OS won't show a notification
//! (e.g. Windows notifications switched off, or no notification server on Linux).
//! Small always-on-top windows stacked in the screen corner.

use iced::widget::{Space, button, column, container, mouse_area, row, text};
use iced::{Alignment, Element, Font, Length, Size, Task, font, window};
use std::time::{Duration, Instant};

use crate::app::{App, Message};
use crate::icons;
use crate::platform::{Notice, NotificationAction, PlatformEvent};
use crate::theme;

const SIZE: Size = Size { width: 340.0, height: 112.0 };
const SIZE_WITH_HINT: Size = Size { width: 340.0, height: 146.0 };
const MAX_OPEN: usize = 3;
const LIFETIME: Duration = Duration::from_secs(10 * 60);

pub struct Popup {
    id: window::Id,
    slot: usize,
    notice: Notice,
    opened: Instant,
    /// Why the OS refused, shown with a link to its settings.
    blocked: Option<String>,
}

#[derive(Default)]
pub struct PopupState {
    list: Vec<Popup>,
}

#[derive(Debug, Clone)]
pub enum PopupMsg {
    Join(window::Id),
    Snooze(window::Id),
    Open(window::Id),
    Dismiss(window::Id),
    Settings,
}

pub fn is_popup(app: &App, id: window::Id) -> bool {
    app.popups.list.iter().any(|p| p.id == id)
}

/// Show `n` as an in-app popup. `blocked` explains why the OS didn't.
pub fn show(app: &mut App, n: Notice, blocked: Option<String>) -> Task<Message> {
    let mut tasks = Vec::new();
    // Same alert again (e.g. a snooze firing) replaces the old popup.
    if let Some(i) = app.popups.list.iter().position(|p| p.notice.key == n.key) {
        tasks.push(window::close(app.popups.list.remove(i).id));
    }
    if app.popups.list.len() >= MAX_OPEN {
        tasks.push(window::close(app.popups.list.remove(0).id));
    }
    let slot = (0..).find(|s| !app.popups.list.iter().any(|p| p.slot == *s)).unwrap_or(0);
    let size = if blocked.is_some() { SIZE_WITH_HINT } else { SIZE };
    let (id, open) = window::open(window::Settings {
        size,
        position: crate::platform::popup_position(slot, SIZE_WITH_HINT),
        resizable: false,
        decorations: false,
        level: window::Level::AlwaysOnTop,
        exit_on_close_request: false,
        icon: crate::platform::window_icon(),
        platform_specific: crate::platform::flyout_platform(),
        ..Default::default()
    });
    log::info!("showing in-app reminder for {:?}", n.title);
    app.popups.list.push(Popup { id, slot, notice: n, opened: Instant::now(), blocked });
    tasks.push(open.discard());
    Task::batch(tasks)
}

fn take(app: &mut App, id: window::Id) -> Option<(Notice, Task<Message>)> {
    let i = app.popups.list.iter().position(|p| p.id == id)?;
    let p = app.popups.list.remove(i);
    Some((p.notice, window::close(id)))
}

pub fn update(app: &mut App, m: PopupMsg) -> Task<Message> {
    let (id, action): (window::Id, fn(Notice) -> Option<NotificationAction>) = match m {
        PopupMsg::Settings => {
            if let Some(url) = crate::platform::notification_settings_url() {
                crate::app::open_url(url);
            }
            return Task::none();
        }
        PopupMsg::Dismiss(id) => (id, |_| None),
        PopupMsg::Join(id) => (id, |n| n.join_url.map(NotificationAction::Join)),
        PopupMsg::Snooze(id) => (id, |n| Some(NotificationAction::Snooze(n.key))),
        PopupMsg::Open(id) => (id, |n| Some(NotificationAction::Open(n.key))),
    };
    let Some((notice, close)) = take(app, id) else { return Task::none() };
    match action(notice) {
        Some(a) => Task::batch([close, Task::done(Message::Platform(PlatformEvent::Notification(a)))]),
        None => close,
    }
}

/// The user closed a popup some other way (Alt+F4).
pub fn close_requested(app: &mut App, id: window::Id) -> Option<Task<Message>> {
    take(app, id).map(|(_, t)| t)
}

/// Close popups that have been up for a long time.
pub fn tick(app: &mut App) -> Task<Message> {
    let (old, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut app.popups.list).into_iter().partition(|p| p.opened.elapsed() > LIFETIME);
    app.popups.list = keep;
    Task::batch(old.into_iter().map(|p| window::close(p.id)))
}

pub fn view(app: &App, id: window::Id) -> Element<'_, Message> {
    let Some(pop) = app.popups.list.iter().find(|p| p.id == id) else { return Space::new().into() };
    let p = app.palette();
    let bold = Font { weight: font::Weight::Bold, ..Font::DEFAULT };
    let msg = |m: PopupMsg| Message::Popup(m);

    let close =
        button(text("×").size(16).color(p.sidebar_muted)).style(theme::side_ghost).padding([0, 6]).on_press(msg(PopupMsg::Dismiss(id)));
    let head = row![
        icons::icon(icons::bell(), 14.0, p.accent),
        text(pop.notice.title.clone()).size(14).font(bold).color(p.sidebar_text).width(Length::Fill),
        close,
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let body = mouse_area(column![head, text(pop.notice.body.clone()).size(12).color(p.sidebar_muted)].spacing(4).width(Length::Fill))
        .on_press(msg(PopupMsg::Open(id)))
        .interaction(iced::mouse::Interaction::Pointer);

    let mut actions = row![].spacing(6).align_y(Alignment::Center);
    if pop.notice.join_url.is_some() {
        actions = actions.push(button(text("Join").size(12)).style(theme::primary).padding([4, 12]).on_press(msg(PopupMsg::Join(id))));
    }
    actions = actions
        .push(
            button(text("Snooze 5 min").size(12).color(p.sidebar_text))
                .style(theme::side_ghost)
                .padding([4, 10])
                .on_press(msg(PopupMsg::Snooze(id))),
        )
        .push(Space::new().width(Length::Fill))
        .push(
            button(text("Dismiss").size(12).color(p.sidebar_text))
                .style(theme::side_ghost)
                .padding([4, 10])
                .on_press(msg(PopupMsg::Dismiss(id))),
        );

    let mut col = column![body, actions].spacing(10);
    if let Some(reason) = &pop.blocked {
        let mut hint =
            row![text(format!("{reason}, so Calen shows its own reminders.")).size(11).color(p.sidebar_muted).width(Length::Fill)]
                .spacing(6)
                .align_y(Alignment::Center);
        if crate::platform::notification_settings_url().is_some() {
            hint = hint.push(
                button(text("Settings").size(11).color(p.accent))
                    .style(theme::side_ghost)
                    .padding([2, 6])
                    .on_press(msg(PopupMsg::Settings)),
            );
        }
        col = col.push(hint);
    }

    container(col)
        .padding([10, 12])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |t| {
            let mut s = theme::sidebar(t);
            s.border = iced::Border { color: theme::with_alpha(p.accent, 0.55), width: 1.0, radius: 10.0.into() };
            s
        })
        .into()
}
