pub mod context;
pub mod editor;
pub mod grid;
pub mod month;
pub mod sidebar;
pub mod week;

use chrono::{Datelike, Local};
use iced::widget::{Space, button, canvas, center, column, container, mouse_area, opaque, row, scrollable, stack, text, text_input};
use iced::{Alignment, Element, Font, Length, Padding, font, window};

use crate::app::{App, Message, QUICK_INPUT, WEEK_SCROLL, first_of_month, start_of_week};
use crate::edit::Scope;
use crate::icons;
use crate::model::Occ;
use crate::settings::ViewKind;
use crate::theme::{self, Palette};

const BOLD: Font = Font { weight: font::Weight::Bold, ..Font::DEFAULT };

pub fn view(app: &App, id: window::Id) -> Element<'_, Message> {
    if Some(id) == app.main_window {
        main_window(app)
    } else if crate::flyout::is_flyout(app, id) {
        crate::flyout::view(app)
    } else if crate::alert_popup::is_popup(app, id) {
        crate::alert_popup::view(app, id)
    } else {
        Space::new().into()
    }
}

fn toolbar(app: &App, p: Palette) -> Element<'_, Message> {
    // The Day view's header carries the full date, and the left pane's
    // mini-month names the month, so the title only shows when it's needed.
    let title: Element<'_, Message> = match app.view {
        ViewKind::Day => Space::new().into(),
        _ if app.settings.sidebar_visible => Space::new().into(),
        ViewKind::Week => {
            let s = start_of_week(app.anchor, app.settings.week_start_day());
            let e = s + chrono::Duration::days(6);
            let label =
                if s.month() == e.month() { s.format("%B").to_string() } else { format!("{} – {}", s.format("%b"), e.format("%b")) };
            row![text(label).size(20).font(BOLD), text(format!(" {}", e.year())).size(20).font(BOLD).color(p.accent)].into()
        }
        ViewKind::Month => row![
            text(app.anchor.format("%B").to_string()).size(20).font(BOLD),
            text(format!(" {}", app.anchor.year())).size(20).font(BOLD).color(p.accent)
        ]
        .into(),
    };

    let seg = |label: &'static str, v: ViewKind| {
        button(text(label).size(13)).style(theme::segment(app.view == v)).padding([4, 12]).on_press(Message::SetView(v))
    };

    let mut left = row![].spacing(4).align_y(Alignment::Center);
    if !app.settings.sidebar_visible {
        left =
            left.push(button(icons::icon(icons::sidebar(), 16.0, p.muted)).style(theme::ghost).padding(6).on_press(Message::ToggleSidebar));
    }
    left = left
        .push(button(icons::icon(icons::chevron_left(), 16.0, p.text)).style(theme::ghost).padding(5).on_press(Message::Prev))
        .push(button(text("Today").size(13)).style(theme::ghost).padding([4, 8]).on_press(Message::Today))
        .push(button(icons::icon(icons::chevron_right(), 16.0, p.text)).style(theme::ghost).padding(5).on_press(Message::Next))
        .push(Space::new().width(10))
        .push(title);

    let right = row![
        crate::sync_hook::status_pill(app, p),
        row![seg("Day", ViewKind::Day), seg("Week", ViewKind::Week), seg("Month", ViewKind::Month)].spacing(2),
        button(icons::icon(icons::search(), 16.0, p.muted))
            .style(theme::ghost)
            .padding(6)
            .on_press(Message::Palette(crate::palette::Msg::OpenWith(String::new()))),
        button(icons::icon(icons::plus(), 17.0, p.muted)).style(theme::ghost).padding(5).on_press(Message::OpenQuickAdd(None)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let bar = row![
        container(row![left, Space::new().width(Length::Fill), right].align_y(Alignment::Center)).padding(Padding {
            top: 8.0,
            bottom: 8.0,
            left: 12.0,
            right: 10.0
        }),
        window_controls(app, p),
    ];
    // Empty parts of the bar move the window; buttons capture their own clicks.
    title_drag_area(container(bar).style(move |_| container::Style {
        background: Some(p.bg.into()),
        border: iced::Border { color: p.grid, width: 0.0, radius: 0.0.into() },
        ..Default::default()
    }))
}

/// Invisible grab strips along the window edges (the window has no system frame).
/// The middle is empty space, so clicks there reach the layers below.
fn resize_edges<'a>() -> Element<'a, Message> {
    use iced::mouse::Interaction;
    use iced::window::Direction;
    const E: f32 = 5.0;
    let grip = |w: Length, h: Length, dir: Direction, cursor: Interaction| -> Element<'a, Message> {
        mouse_area(Space::new().width(w).height(h)).on_press(Message::WinResize(dir)).interaction(cursor).into()
    };
    let (fill, e) = (Length::Fill, Length::Fixed(E));
    column![
        row![
            grip(e, e, Direction::NorthWest, Interaction::ResizingDiagonallyDown),
            grip(fill, e, Direction::North, Interaction::ResizingVertically),
            grip(e, e, Direction::NorthEast, Interaction::ResizingDiagonallyUp),
        ],
        row![
            grip(e, fill, Direction::West, Interaction::ResizingHorizontally),
            Space::new().width(fill).height(fill),
            grip(e, fill, Direction::East, Interaction::ResizingHorizontally),
        ]
        .height(fill),
        row![
            grip(e, e, Direction::SouthWest, Interaction::ResizingDiagonallyUp),
            grip(fill, e, Direction::South, Interaction::ResizingVertically),
            grip(e, e, Direction::SouthEast, Interaction::ResizingDiagonallyDown),
        ],
    ]
    .into()
}

/// Makes `content` behave like a title bar: drag to move, double-click to maximize.
pub fn title_drag_area<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    mouse_area(content).on_press(Message::WinDrag).on_double_click(Message::WinToggleMaximize).into()
}

/// A text link with an underline. iced text has no underline, so a 1 px rule is
/// drawn under the label.
pub fn underlined_link<'a>(label: &'a str, size: f32, color: iced::Color, msg: Message) -> Element<'a, Message> {
    button(
        column![
            text(label).size(size).color(color),
            container(Space::new().width(Length::Fill).height(1))
                .style(move |_| container::Style { background: Some(color.into()), ..Default::default() }),
        ]
        .width(Length::Shrink),
    )
    .style(theme::link)
    .padding([2, 2])
    .on_press(msg)
    .into()
}

/// Minimize / maximize-restore / close, flush with the window's top-right corner.
fn window_controls(app: &App, p: Palette) -> Element<'_, Message> {
    let cap = |icon: iced::widget::svg::Handle, close: bool, msg: Message| {
        button(container(icons::icon(icon, 15.0, p.text)).center(Length::Fill))
            .style(theme::caption_button(close))
            .padding(0)
            .width(Length::Fixed(46.0))
            .height(Length::Fixed(40.0))
            .on_press(msg)
    };
    let close = match app.main_window {
        Some(id) => Message::CloseRequested(id),
        None => Message::ShowMain,
    };
    row![
        cap(icons::win_min(), false, Message::WinMinimize),
        cap(if app.main_maximized { icons::win_restore() } else { icons::win_max() }, false, Message::WinToggleMaximize),
        cap(icons::win_close(), true, close),
    ]
    .into()
}

/// Everything a grid's static layer is drawn from, hashed.
fn grid_key(app: &App, p: Palette, occs: &[&Occ], days: &[chrono::NaiveDate]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    days.hash(&mut h);
    grid::occs_key(&mut h, occs);
    app.selected.hash(&mut h);
    app.anchor.hash(&mut h);
    app.now.date_naive().hash(&mut h);
    app.settings.time_24h.hash(&mut h);
    app.settings.date_format.hash(&mut h);
    std::mem::discriminant(&app.view).hash(&mut h);
    format!("{p:?}").hash(&mut h);
    h.finish()
}

fn content(app: &App, p: Palette) -> Element<'_, Message> {
    let days = app.visible_days();
    let (first, last) = (days[0], *days.last().unwrap());
    let in_range: Vec<&Occ> = app.occs.iter().filter(|o| o.start.date_naive() <= last && o.last_day() >= first).collect();
    let el: Element<'_, grid::GridMsg> = match app.view {
        ViewKind::Month => {
            let first = start_of_week(first_of_month(app.anchor), app.settings.week_start_day());
            let key = grid_key(app, p, &in_range, &[first]);
            let g = month::MonthGrid {
                first,
                month: app.anchor.month(),
                occs: in_range,
                palette: p,
                selected: app.selected.as_ref(),
                selected_date: app.anchor,
                today: app.now.date_naive(),
                time_24h: app.settings.time_24h,
                cache: app.grid_caches.month.with_key(key),
            };
            canvas(g).width(Length::Fill).height(Length::Fill).into()
        }
        ViewKind::Week | ViewKind::Day => {
            let (spanning, timed): (Vec<&Occ>, Vec<&Occ>) = in_range.into_iter().partition(|o| o.spans_days());
            let header_h = week::AllDayHeader::height(&spanning, &days);
            let header = week::AllDayHeader {
                cache: app.grid_caches.header.with_key(grid_key(app, p, &spanning, &days)),
                days: days.clone(),
                occs: spanning,
                palette: p,
                selected: app.selected.as_ref(),
                today: app.now.date_naive(),
                active: app.anchor,
                date_format: app.settings.date_format,
            };
            let grid = week::WeekGrid {
                cache: app.grid_caches.week.with_key(grid_key(app, p, &timed, &days)),
                days,
                occs: timed,
                palette: p,
                selected: app.selected.as_ref(),
                now: app.now,
                time_24h: app.settings.time_24h,
            };
            column![
                canvas(header).width(Length::Fill).height(Length::Fixed(header_h)),
                scrollable(canvas(grid).width(Length::Fill).height(Length::Fixed(week::WeekGrid::height())))
                    .id(iced::widget::Id::new(WEEK_SCROLL))
                    .style(theme::main_scroll)
                    .height(Length::Fill),
            ]
            .into()
        }
    };
    el.map(Message::Grid)
}

fn main_window(app: &App) -> Element<'_, Message> {
    let p = app.palette();
    let main = container(column![toolbar(app, p), content(app, p)]).style(theme::main_area).width(Length::Fill).height(Length::Fill);
    let base: Element<'_, Message> = if app.settings.sidebar_visible { row![sidebar::view(app), main].into() } else { main.into() };

    let mut layers = vec![base];
    if !app.main_maximized {
        layers.push(resize_edges());
    }
    if let Some(el) = context::view(app, p) {
        layers.push(el);
    }
    if let Some(q) = &app.quick {
        layers.push(modal(quick_add(app, q, p), Message::CloseOverlay, false));
    }
    if let Some(ed) = &app.editor {
        let card = ed.view(p, app.calendar_choices(), &app.images, app.drop_hover, crate::storage::enabled(app)).map(Message::Editor);
        layers.push(modal(card, Message::CloseOverlay, true));
    }
    if let Some(pc) = &app.scope_prompt {
        layers.push(modal(scope_prompt(&pc.title, matches!(pc.change, crate::edit::Change::Delete), p), Message::ScopeChosen(None), true));
    }
    if let Some(el) = crate::storage::view(app, p) {
        layers.push(modal(el, Message::Storage(crate::storage::StorageMsg::Close), true));
    }
    if let Some(el) = crate::sync_hook::dialog(app, p) {
        layers.push(modal(el, Message::Sync(crate::sync_hook::SyncMsg::CloseDialog), true));
    }
    if let Some(el) = crate::palette::view(app, p) {
        layers.push(modal(el, Message::Palette(crate::palette::Msg::Close), false));
    }
    if app.show_shortcuts {
        layers.push(modal(shortcuts(p), Message::CloseShortcuts, true));
    }
    if let Some(el) = crate::viewer::view(app, p) {
        layers.push(el);
    }
    if let Some(b) = crate::updater::banner(app, p) {
        // Above the transient toast so the two never overlap.
        layers.push(
            container(b).align_bottom(Length::Fill).center_x(Length::Fill).padding(Padding { bottom: 64.0, ..Default::default() }).into(),
        );
    }
    if let Some((msg, _)) = &app.toast {
        layers.push(
            container(
                container(text(msg.clone()).size(13).color(iced::Color::WHITE))
                    .padding([8, 14])
                    .style(theme::pill(theme::with_alpha(iced::Color::from_rgb8(0x22, 0x22, 0x26), 0.95), iced::Color::WHITE)),
            )
            .align_bottom(Length::Fill)
            .center_x(Length::Fill)
            .padding(24)
            .into(),
        );
    }
    context::tracker(stack(layers), &app.context_at)
}

/// Dim backdrop + content. `centered` = middle of the window, else near the top.
fn modal<'a>(content: Element<'a, Message>, on_dismiss: Message, centered: bool) -> Element<'a, Message> {
    let positioned: Element<'a, Message> = if centered {
        center(opaque(content)).into()
    } else {
        container(opaque(content)).center_x(Length::Fill).padding(Padding { top: 90.0, ..Default::default() }).into()
    };
    opaque(mouse_area(container(positioned).width(Length::Fill).height(Length::Fill).style(theme::backdrop)).on_press(on_dismiss))
}

fn quick_add<'a>(app: &'a App, q: &'a crate::app::QuickAddState, p: Palette) -> Element<'a, Message> {
    let input = text_input("Lunch with Sam tomorrow 12:30 at Café Luna /work !15m", &q.input)
        .id(iced::widget::Id::new(QUICK_INPUT))
        .on_input(Message::QuickInput)
        .on_submit(Message::QuickSubmit)
        .size(18)
        .padding([6, 2])
        .style(theme::bare_input);

    let mut chips = row![].spacing(6).align_y(Alignment::Center);
    let chip = |s: String, c: iced::Color| container(text(s).size(12)).padding([3, 8]).style(theme::pill(theme::with_alpha(c, 0.16), c));
    let pv = &q.preview;
    let today = Local::now().date_naive();
    let date = pv.date.or(q.date_hint).unwrap_or(today);
    let date_label = if date == today {
        "Today".to_string()
    } else if date == today + chrono::Duration::days(1) {
        "Tomorrow".to_string()
    } else {
        date.format("%a %-d %b").to_string()
    };
    if !q.input.trim().is_empty() {
        chips = chips.push(chip(date_label, p.today));
        let (s, e) = pv.resolve(date, app.settings.default_duration);
        if pv.all_day || pv.start_time.is_none() {
            chips = chips.push(chip("All day".into(), p.today));
        } else {
            chips = chips.push(chip(
                format!("{} – {}", grid::fmt_time(s.local(), app.settings.time_24h), grid::fmt_time(e.local(), app.settings.time_24h)),
                p.today,
            ));
        }
        if let Some(l) = &pv.location {
            chips = chips.push(chip(format!("@ {l}"), p.success));
        }
        if pv.rrule.is_some() {
            chips = chips.push(chip("Repeats".into(), p.muted));
        }
        if let Some(a) = pv.alert_minutes {
            chips = chips.push(chip(format!("Alert {a} min"), p.muted));
        }
        if let Some(u) = &pv.url
            && calen_core::links::find_meeting(u).is_some()
        {
            chips = chips.push(chip("Video call".into(), p.today));
        }
        let cal = pv
            .calendar
            .as_deref()
            .and_then(|h| app.calendar_choices().into_iter().find(|c| c.name.to_lowercase().starts_with(&h.to_lowercase())));
        if let Some(c) = cal {
            chips = chips.push(chip(c.name.clone(), c.color));
        }
    }

    let hint = text("Enter to add · Tab for details · Esc to cancel").size(11.5).color(p.faint);
    container(
        column![
            row![icons::icon(icons::calendar(), 18.0, p.muted), input].spacing(10).align_y(Alignment::Center),
            chips,
            row![
                text(if pv.title.is_empty() { String::new() } else { format!("“{}”", pv.title) }).size(12).color(p.muted),
                Space::new().width(Length::Fill),
                hint
            ]
        ]
        .spacing(10),
    )
    .padding([14, 16])
    .width(Length::Fixed(600.0))
    .style(theme::popover)
    .into()
}

fn shortcuts<'a>(p: Palette) -> Element<'a, Message> {
    let rows: [(&str, &str); 16] = [
        ("N  /  Ctrl+N", "Quick add an event"),
        ("Tab (in quick add)", "Open the full editor"),
        ("Ctrl+P  /  Ctrl+F  /  /", "Search events"),
        ("Ctrl+P, then >", "Commands & settings"),
        ("Ctrl+,", "Settings"),
        ("T", "Today"),
        ("D  /  W  /  M", "Day / week / month view"),
        ("←  /  →", "Previous / next period"),
        ("J  /  K  (↓ / ↑)", "Select next / previous event"),
        ("Enter", "Open selected event"),
        ("Delete", "Delete selected event"),
        ("Ctrl+Z", "Undo"),
        ("Ctrl+S  /  Ctrl+Enter", "Save in the editor"),
        ("Double-click", "Open event / create one"),
        ("Drag  /  drag bottom edge", "Move / change length"),
        ("Esc", "Close / deselect"),
    ];
    let mut col = column![text("Keyboard shortcuts").size(16).font(BOLD)].spacing(6);
    for (k, d) in rows {
        col = col.push(
            row![container(text(k).size(12.5).color(p.text)).width(Length::Fixed(190.0)), text(d).size(12.5).color(p.muted)]
                .align_y(Alignment::Center),
        );
    }
    col = col.push(row![
        Space::new().width(Length::Fill),
        button(text("Close").size(13)).style(theme::ghost).padding([6, 12]).on_press(Message::CloseShortcuts)
    ]);
    container(col).padding(20).width(Length::Fixed(460.0)).style(theme::popover).into()
}

fn scope_prompt<'a>(title: &str, delete: bool, p: Palette) -> Element<'a, Message> {
    let heading = if delete { "Delete a repeating event" } else { "Change a repeating event" };
    container(
        column![
            text(heading).size(16).font(BOLD),
            text(format!("“{title}” repeats. Apply this to:")).size(13).color(p.muted),
            row![
                button(text("Cancel").size(13)).style(theme::ghost).padding([6, 12]).on_press(Message::ScopeChosen(None)),
                Space::new().width(Length::Fill),
                button(text("All events").size(13)).style(theme::ghost).padding([6, 12]).on_press(Message::ScopeChosen(Some(Scope::All))),
                button(text("This event only").size(13))
                    .style(theme::primary)
                    .padding([6, 12])
                    .on_press(Message::ScopeChosen(Some(Scope::This))),
            ]
            .spacing(8)
        ]
        .spacing(12),
    )
    .padding(18)
    .width(Length::Fixed(420.0))
    .style(theme::popover)
    .into()
}
