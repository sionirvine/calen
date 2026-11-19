//! Dark sidebar: mini-month with event dots + agenda (also used by the tray flyout).

use chrono::{Datelike, Duration, Local, NaiveDate};
use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::{Space, button, checkbox, column, container, mouse_area, operation, row, scrollable, sensor, text};
use iced::{Alignment, Color, Element, Font, Length, Padding, Task, font};
use std::collections::HashMap;
use std::time::{Duration as StdDuration, Instant};

use crate::app::{App, Message, first_of_month, start_of_week};
use crate::icons;
use crate::model::Occ;
use crate::theme::{self, Palette};
use crate::views::grid::fmt_time;

const BOLD: Font = Font { weight: font::Weight::Bold, ..Font::DEFAULT };
const SEMI: Font = Font { weight: font::Weight::Semibold, ..Font::DEFAULT };

/// Which window the mini-month is drawn in; decides what a day click does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MiniTarget {
    /// Main sidebar: a click moves the main view to that day.
    Main,
    /// Tray flyout: a click lists that day's week below.
    Flyout,
}

pub fn mini_month<'a>(app: &'a App, p: Palette, target: MiniTarget) -> Element<'a, Message> {
    let compact = target == MiniTarget::Flyout;
    let month = app.mini_month;
    let header = row![
        text(month.format("%B").to_string()).size(if compact { 24 } else { 26 }).font(BOLD).color(p.sidebar_text),
        text(format!(" {}", month.year())).size(if compact { 24 } else { 26 }).font(BOLD).color(p.accent),
        Space::new().width(Length::Fill),
        button(icons::icon(icons::chevron_left(), 16.0, p.sidebar_muted))
            .style(theme::side_ghost)
            .padding(4)
            .on_press(Message::MiniShift(-1)),
        button(icons::icon(icons::chevron_right(), 16.0, p.sidebar_muted))
            .style(theme::side_ghost)
            .padding(4)
            .on_press(Message::MiniShift(1)),
    ]
    .align_y(Alignment::Center);

    let first = start_of_week(first_of_month(month), app.settings.week_start_day());
    let mut dots: HashMap<NaiveDate, Vec<Color>> = HashMap::new();
    for o in &app.occs {
        let mut d = o.start.date_naive();
        let last = o.last_day().min(d + Duration::days(40));
        while d <= last {
            let v = dots.entry(d).or_default();
            if v.len() < 3 && !v.contains(&o.color) {
                v.push(o.color);
            }
            d += Duration::days(1);
        }
    }

    let today = Local::now().date_naive();
    // The picked day: the main view's date, or the flyout's picked day.
    let picked = match target {
        MiniTarget::Main => Some(app.anchor),
        MiniTarget::Flyout => app.flyout.week.map(|_| app.flyout.active),
    };
    let mut weekdays = row![].spacing(0);
    for i in 0..7 {
        let d = first + Duration::days(i);
        let is_today_wd = d.weekday() == today.weekday();
        weekdays = weekdays.push(
            text(d.format("%a").to_string().to_uppercase())
                .size(9.5)
                .font(SEMI)
                .color(if is_today_wd { p.today } else { p.sidebar_muted })
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
        );
    }

    let mut grid = column![weekdays].spacing(1);
    for w in 0..6 {
        let mut r = row![].spacing(0);
        let week_start = first + Duration::days(w * 7);
        let selected_week = match target {
            MiniTarget::Main => {
                app.view == crate::settings::ViewKind::Week && start_of_week(app.anchor, app.settings.week_start_day()) == week_start
            }
            MiniTarget::Flyout => app.flyout.week == Some(week_start),
        };
        for i in 0..7 {
            let d = week_start + Duration::days(i);
            let in_month = d.month() == month.month();
            let mut dot_row = row![].spacing(2);
            for c in dots.get(&d).map(|v| v.as_slice()).unwrap_or(&[]) {
                dot_row = dot_row.push(container(Space::new().width(4).height(4)).style(theme::dot(if in_month {
                    *c
                } else {
                    theme::with_alpha(*c, 0.4)
                })));
            }
            let cell = column![
                text(d.day().to_string()).size(12.5).font(if d == today { BOLD } else { Font::DEFAULT }),
                container(dot_row).height(5),
            ]
            .align_x(Alignment::Center)
            .spacing(1);
            r = r.push(
                button(container(cell).center_x(Length::Fill))
                    .style(theme::mini_day(picked == Some(d), d == today, in_month))
                    .padding(Padding::from([3, 0]))
                    .width(Length::Fill)
                    .on_press(match target {
                        MiniTarget::Main => Message::SelectDate(d),
                        MiniTarget::Flyout => Message::FlyoutWeek(Some(d)),
                    }),
            );
        }
        let row_el: Element<'a, Message> = if selected_week {
            container(r)
                .style(move |_| container::Style {
                    background: Some(theme::with_alpha(p.sidebar_hover, 0.8).into()),
                    border: iced::Border { radius: 999.0.into(), ..Default::default() },
                    ..Default::default()
                })
                .into()
        } else {
            r.into()
        };
        grid = grid.push(row_el);
    }
    column![header, grid].spacing(8).into()
}

fn day_label(d: NaiveDate, today: NaiveDate) -> (String, bool) {
    let date = d.format("%-d/%-m/%Y").to_string();
    if d == today {
        (format!("TODAY  {date}"), true)
    } else if d == today + Duration::days(1) {
        (format!("TOMORROW  {date}"), false)
    } else {
        (format!("{}  {date}", d.format("%A").to_string().to_uppercase()), false)
    }
}

pub fn occ_time_label(o: &Occ, day: NaiveDate, h24: bool) -> String {
    if o.all_day {
        return "all-day".into();
    }
    let starts_today = o.start.date_naive() == day;
    let ends_today = o.last_day() == day;
    match (starts_today, ends_today) {
        (true, true) => format!("{} – {}", fmt_time(o.start, h24), fmt_time(o.end, h24)),
        (true, false) => format!("{} →", fmt_time(o.start, h24)),
        (false, true) => format!("→ {}", fmt_time(o.end, h24)),
        (false, false) => "all day".into(),
    }
}

/// Agenda list starting at `from`, covering `days` days. `whole_days` keeps
/// events that already ended (a picked week); otherwise today hides finished ones.
/// `empty` is shown when nothing is listed. The `active` (picked) day is always
/// listed, with a red heading on a band; today's heading stays blue.
pub fn agenda<'a>(
    app: &'a App,
    p: Palette,
    from: NaiveDate,
    days: i64,
    whole_days: bool,
    empty: Option<&'static str>,
    active: Option<NaiveDate>,
) -> Element<'a, Message> {
    let today = Local::now().date_naive();
    let h24 = app.settings.time_24h;
    let mut col = column![].spacing(2);
    let mut any = false;
    for i in 0..days {
        let d = from + Duration::days(i);
        let items: Vec<&Occ> = app
            .occs
            .iter()
            .filter(|o| o.start.date_naive() <= d && o.last_day() >= d)
            .filter(|o| whole_days || d != today || o.all_day || o.end > app.now || o.start.date_naive() != d)
            .collect();
        let picked = active == Some(d);
        if items.is_empty() && d != today && !picked {
            continue;
        }
        any = true;
        let (label, is_today) = day_label(d, today);
        let color = if is_today {
            p.today
        } else if picked {
            p.accent
        } else {
            p.sidebar_muted
        };
        let heading = text(label).size(11.5).font(BOLD).color(color);
        col = col.push(if picked {
            container(container(heading).width(Length::Fill).padding([3, 6]).style(move |_| container::Style {
                background: Some(theme::with_alpha(p.accent, 0.14).into()),
                border: iced::Border { radius: 6.0.into(), ..Default::default() },
                ..Default::default()
            }))
            .padding(Padding { top: 7.0, bottom: 1.0, left: 0.0, right: 0.0 })
        } else {
            container(heading).padding(Padding { top: 10.0, bottom: 4.0, left: 0.0, right: 0.0 })
        });
        if items.is_empty() {
            col = col.push(text("Nothing scheduled").size(12).color(p.sidebar_muted));
        }
        for o in items {
            col = col.push(agenda_item(app, p, o, d, h24));
        }
    }
    if let (false, Some(msg)) = (any, empty) {
        col = col.push(text(msg).size(12).color(p.sidebar_muted));
    }
    col.into()
}

fn agenda_item<'a>(app: &'a App, p: Palette, o: &'a Occ, day: NaiveDate, h24: bool) -> Element<'a, Message> {
    let selected = app.selected.as_ref().is_some_and(|k| *k == o.key);
    let ev = app.data.event(&o.key, o.event_index);
    let links = ev.map(calen_core::links::detect).unwrap_or_default();

    if o.all_day {
        let chip =
            container(text(o.title.clone()).size(12.5).font(SEMI).color(Color::WHITE)).padding([2, 8]).style(move |_| container::Style {
                background: Some(o.color.into()),
                border: iced::Border { radius: 5.0.into(), ..Default::default() },
                ..Default::default()
            });
        return mouse_area(container(chip).padding([3, 0]))
            .on_press(Message::AgendaClick(o.key.clone(), day))
            .on_double_click(Message::Grid(crate::views::grid::GridMsg::Open(o.key.clone())))
            .interaction(iced::mouse::Interaction::Pointer)
            .into();
    }

    let mut time_row = row![text(occ_time_label(o, day, h24)).size(12).color(p.sidebar_muted)].spacing(6).align_y(Alignment::Center);
    if o.has_meeting {
        time_row = time_row.push(icons::icon(icons::video(), 12.0, p.sidebar_muted));
    }
    if o.has_images {
        time_row = time_row.push(icons::icon(icons::image(), 12.0, p.sidebar_muted));
    }
    let mut details = column![time_row, text(o.title.clone()).size(13.5).font(BOLD).color(p.sidebar_text)].spacing(1);
    if let Some(m) = &links.maps {
        let name = if o.location.is_empty() { m.place.clone().unwrap_or_else(|| "Map".into()) } else { o.location.clone() };
        details = details.push(
            button(row![icons::icon(icons::map_pin(), 11.0, p.today), text(name).size(12)].spacing(4).align_y(Alignment::Center))
                .style(theme::link)
                .padding(0)
                .on_press(Message::OpenUrl(m.url.clone())),
        );
    } else if !o.location.is_empty() {
        details = details.push(text(o.location.clone()).size(12).color(p.sidebar_muted));
    }

    let mut line = row![
        container(container(Space::new().width(9).height(9)).style(theme::dot(o.color)))
            .padding(Padding { top: 5.0, ..Default::default() }),
        details.width(Length::Fill),
    ]
    .spacing(10);
    if let Some(m) = &links.meeting {
        let soon = o.start - app.now < Duration::minutes(15) && o.end > app.now;
        if o.end > app.now {
            line = line.push(
                button(text("Join").size(11.5).font(BOLD))
                    .style(if soon { theme::join } else { theme::join_outline })
                    .padding([2, 10])
                    .on_press(Message::OpenUrl(m.url.clone())),
            );
        }
    }
    let bg = if selected { Some(theme::with_alpha(o.color, 0.22)) } else { None };
    mouse_area(container(line).padding([5, 8]).style(move |_| container::Style {
        background: bg.map(Into::into),
        border: iced::Border { radius: 8.0.into(), ..Default::default() },
        ..Default::default()
    }))
    .on_press(Message::AgendaClick(o.key.clone(), day))
    .on_double_click(Message::Grid(crate::views::grid::GridMsg::Open(o.key.clone())))
    .interaction(iced::mouse::Interaction::Pointer)
    .into()
}

/// A week list whose measured parts let it open with the picked day at the top.
#[derive(Default)]
pub struct WeekPane {
    /// Heights of the days before the picked one, the picked day onward, and
    /// the visible list.
    past_h: Option<f32>,
    tail_h: Option<f32>,
    view_h: Option<f32>,
    /// Until then, new measurements re-apply the scroll.
    scroll_until: Option<Instant>,
}

/// Which window a week list is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Main,
    Flyout,
}

impl Pane {
    fn scroll_id(self) -> &'static str {
        match self {
            Pane::Main => "side-list",
            Pane::Flyout => "flyout-list",
        }
    }
}

/// A part of a week list whose height was measured.
#[derive(Debug, Clone, Copy)]
pub enum Part {
    Past,
    Tail,
    Viewport,
}

/// Days of `week` before `active`, or None when `active` isn't in that week.
fn days_before(week: NaiveDate, active: NaiveDate) -> Option<i64> {
    let n = (active - week).num_days();
    (0..7).contains(&n).then_some(n)
}

/// How long a picked day keeps being scrolled into place while it's measured.
pub const SETTLE: StdDuration = StdDuration::from_millis(800);

impl WeekPane {
    /// Scroll so `active`'s heading is the first line (the top when it isn't in
    /// `week`), and keep doing so for `settle` while the parts are measured.
    pub fn scroll_to(&mut self, pane: Pane, week: Option<NaiveDate>, active: NaiveDate, settle: StdDuration) -> Task<Message> {
        self.scroll_until = Some(Instant::now() + settle);
        self.apply(pane, week, active)
    }

    fn apply(&self, pane: Pane, week: Option<NaiveDate>, active: NaiveDate) -> Task<Message> {
        let y = match week.and_then(|w| days_before(w, active)) {
            Some(n) if n > 0 => self.past_h.unwrap_or(0.0),
            _ => 0.0,
        };
        operation::scroll_to(pane.scroll_id(), AbsoluteOffset { x: 0.0, y })
    }

    pub fn measured(&mut self, part: Part, h: f32, pane: Pane, week: Option<NaiveDate>, active: NaiveDate) -> Task<Message> {
        let slot = match part {
            Part::Past => &mut self.past_h,
            Part::Tail => &mut self.tail_h,
            Part::Viewport => &mut self.view_h,
        };
        *slot = Some(h);
        if self.scroll_until.is_some_and(|t| Instant::now() < t) {
            return self.apply(pane, week, active);
        }
        Task::none()
    }
}

/// "Week of 16 – 22 November".
pub fn week_label(start: NaiveDate) -> String {
    let end = start + Duration::days(6);
    if start.month() == end.month() {
        format!("Week of {} – {}", start.format("%-d"), end.format("%-d %B"))
    } else {
        format!("Week of {} – {}", start.format("%-d %b"), end.format("%-d %b"))
    }
}

/// The week from `start`: the days before `active`, then `active` onward, each
/// measured so the list can open with `active` at the top. A spacer below lets
/// the list scroll that far even when the rest of the week is short.
pub fn week_list<'a>(app: &'a App, p: Palette, start: NaiveDate, active: NaiveDate, pane: Pane) -> Element<'a, Message> {
    let state = match pane {
        Pane::Main => &app.side_pane,
        Pane::Flyout => &app.flyout.pane,
    };
    let (past, tail, room): (Element<'a, Message>, Element<'a, Message>, f32) = match days_before(start, active) {
        Some(n) => {
            let past = if n > 0 { agenda(app, p, start, n, true, None, Some(active)) } else { Space::new().into() };
            let tail = agenda(app, p, start + Duration::days(n), 7 - n, true, None, Some(active));
            let room = match (state.view_h, state.tail_h) {
                // 8 = the list's bottom padding.
                (Some(v), Some(t)) if n > 0 => (v - t - 8.0).max(0.0),
                _ => 0.0,
            };
            (past, tail, room)
        }
        None => (Space::new().into(), agenda(app, p, start, 7, true, Some("Nothing this week"), None), 0.0),
    };
    // Same structure for every week, so the sensors keep their state.
    column![
        sensor(past).on_show(move |s| Message::PaneMeasured(pane, Part::Past, s.height)).on_resize(move |s| Message::PaneMeasured(
            pane,
            Part::Past,
            s.height
        )),
        sensor(tail).on_show(move |s| Message::PaneMeasured(pane, Part::Tail, s.height)).on_resize(move |s| Message::PaneMeasured(
            pane,
            Part::Tail,
            s.height
        )),
        Space::new().height(room),
    ]
    .into()
}

/// The scrollable around a week list, measured for the spacer.
pub fn week_scroller<'a>(content: Element<'a, Message>, pane: Pane) -> Element<'a, Message> {
    // The background matters for the CPU renderer: it repaints where shapes
    // move, and without one, bits of text scrolled away were left behind.
    let list = container(content).padding(Padding { top: 0.0, right: 14.0, bottom: 8.0, left: 14.0 }).style(theme::sidebar);
    sensor(scrollable(list).id(iced::widget::Id::new(pane.scroll_id())).style(theme::side_scroll).height(Length::Fill))
        .on_show(move |s| Message::PaneMeasured(pane, Part::Viewport, s.height))
        .on_resize(move |s| Message::PaneMeasured(pane, Part::Viewport, s.height))
        .into()
}

fn calendars_list<'a>(app: &'a App, p: Palette) -> Element<'a, Message> {
    let mut col = column![text("CALENDARS").size(11.5).font(BOLD).color(p.sidebar_muted)].spacing(6);
    let accounts = app.store.accounts().unwrap_or_default();
    let mut last_account: Option<Option<i64>> = None;
    for c in &app.data.calendars {
        if last_account != Some(c.account_id) {
            let name = match c.account_id {
                None => "On this computer".to_string(),
                Some(id) => accounts.iter().find(|a| a.id == id).map(|a| a.name.clone()).unwrap_or_else(|| "Account".into()),
            };
            col = col.push(container(text(name).size(11).color(p.sidebar_muted)).padding(Padding { top: 6.0, ..Default::default() }));
            last_account = Some(c.account_id);
        }
        let color = theme::parse_hex(&c.color).unwrap_or(p.today);
        let id = c.id.clone();
        col = col.push(
            checkbox(c.visible)
                .label(c.name.clone())
                .on_toggle(move |v| Message::ToggleCalendar(id.clone(), v))
                .size(15)
                .text_size(13)
                .style(move |_, status| {
                    let checked =
                        matches!(status, checkbox::Status::Active { is_checked: true } | checkbox::Status::Hovered { is_checked: true });
                    checkbox::Style {
                        background: if checked { color.into() } else { Color::TRANSPARENT.into() },
                        icon_color: Color::WHITE,
                        border: iced::Border { color, width: 1.5, radius: 4.0.into() },
                        text_color: Some(p.sidebar_text),
                    }
                }),
        );
    }
    col.into()
}

pub fn view<'a>(app: &'a App) -> Element<'a, Message> {
    let p = app.palette();
    let top = crate::views::title_drag_area(
        row![
            button(icons::icon(icons::sidebar(), 16.0, p.sidebar_muted))
                .style(theme::side_ghost)
                .padding(6)
                .on_press(Message::ToggleSidebar),
            Space::new().width(Length::Fill),
            button(icons::icon(icons::plus(), 18.0, p.sidebar_text))
                .style(theme::side_ghost)
                .padding(5)
                .on_press(Message::OpenQuickAdd(None)),
        ]
        .align_y(Alignment::Center),
    );

    let week = start_of_week(app.anchor, app.settings.week_start_day());
    let list: Element<'a, Message> = if app.show_calendars {
        scrollable(container(calendars_list(app, p)).padding([0, 14])).style(theme::side_scroll).height(Length::Fill).into()
    } else {
        column![
            container(text(week_label(week)).size(12).font(BOLD).color(p.sidebar_text)).padding([0, 14]),
            week_scroller(week_list(app, p, week, app.anchor, Pane::Main), Pane::Main),
        ]
        .spacing(2)
        .into()
    };

    let dnd = app.settings.dnd_active(chrono::Utc::now().timestamp());
    let footer = row![
        button(
            row![
                icons::icon(if dnd { icons::bell_off() } else { icons::bell() }, 15.0, if dnd { p.accent } else { p.sidebar_muted }),
                text(if dnd { "Do Not Disturb" } else { "Alerts on" }).size(12).color(if dnd { p.accent } else { p.sidebar_muted }),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
        )
        .style(theme::side_ghost)
        .padding([5, 6])
        .on_press(Message::Palette(crate::palette::Msg::Run(crate::palette::Command::ToggleDnd))),
        Space::new().width(Length::Fill),
        crate::updater::badge(app, p),
        button(
            row![
                text(if app.show_calendars { "Agenda" } else { "Calendars" }).size(12).color(p.sidebar_muted),
                icons::icon(icons::chevron_down(), 12.0, p.sidebar_muted)
            ]
            .spacing(4)
            .align_y(Alignment::Center)
        )
        .style(theme::side_ghost)
        .padding([5, 6])
        .on_press(Message::ToggleCalendarList),
        button(icons::icon(icons::settings(), 15.0, p.sidebar_muted))
            .style(theme::side_ghost)
            .padding(5)
            .on_press(Message::Palette(crate::palette::Msg::OpenWith(">".into()))),
    ]
    .spacing(2)
    .align_y(Alignment::Center);

    container(
        column![
            container(top).padding([8, 10]),
            container(mini_month(app, p, MiniTarget::Main)).padding([0, 14]),
            container(list).height(Length::Fill).padding(Padding { top: 6.0, ..Default::default() }),
            container(footer).padding([6, 8]),
        ]
        .spacing(4),
    )
    .width(Length::Fixed(300.0))
    .height(Length::Fill)
    .style(theme::sidebar)
    .into()
}
