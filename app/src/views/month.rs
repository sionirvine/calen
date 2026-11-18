//! Month grid with multi-day bars, timed event rows, and drag-to-another-day.

use chrono::{Datelike, Duration, NaiveDate};
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Event, Frame, Geometry, Path, Program};
use iced::{Point, Rectangle, Renderer, Size, Theme};
use std::time::Instant;

use super::grid::{self, BOLD, GridMsg};
use crate::model::{Occ, OccKey};
use crate::theme::{self, Palette};

pub const HEAD_H: f32 = 30.0;
const LANE: f32 = 19.0;
const TOP: f32 = 26.0;

pub struct MonthGrid<'a> {
    /// First day shown (start of the first week row).
    pub first: NaiveDate,
    pub month: u32,
    pub occs: Vec<&'a Occ>,
    pub palette: Palette,
    pub selected: Option<&'a OccKey>,
    pub selected_date: NaiveDate,
    pub today: NaiveDate,
    pub time_24h: bool,
}

#[derive(Default)]
pub struct State {
    drag: Option<(usize, NaiveDate, NaiveDate, bool, Point)>, // occ, grab day, current day, moved, origin
    last_click: Option<(Instant, Point)>,
}

enum Item {
    Bar { idx: usize, rect: Rectangle },
    Row { idx: usize, rect: Rectangle },
    More { date: NaiveDate, count: usize, rect: Rectangle },
}

impl<'a> MonthGrid<'a> {
    fn cell(&self, b: &Rectangle) -> Size {
        Size::new(b.width / 7.0, (b.height - HEAD_H) / 6.0)
    }

    fn date_at(&self, p: Point, b: &Rectangle) -> NaiveDate {
        let c = self.cell(b);
        let col = ((p.x / c.width).floor().max(0.0) as i64).min(6);
        let row = (((p.y - HEAD_H) / c.height).floor().max(0.0) as i64).min(5);
        self.first + Duration::days(row * 7 + col)
    }

    fn items(&self, b: &Rectangle) -> Vec<Item> {
        let c = self.cell(b);
        let lanes_fit = (((c.height - TOP - 2.0) / LANE).floor().max(1.0)) as usize;
        let mut out = Vec::new();
        for row in 0..6 {
            let week_start = self.first + Duration::days(row as i64 * 7);
            let y0 = HEAD_H + row as f32 * c.height + TOP;
            let spanning: Vec<&Occ> = self.occs.iter().copied().filter(|o| o.spans_days()).collect();
            let span_idx: Vec<usize> = self.occs.iter().enumerate().filter(|(_, o)| o.spans_days()).map(|(i, _)| i).collect();
            let lanes = grid::bar_lanes(&spanning, week_start, 7);
            // used[col][lane]
            let mut used = vec![vec![false; 64]; 7];
            let mut hidden = [0usize; 7];
            for (si, c0, c1, lane) in lanes {
                // Keep the last lane free for timed rows / "+N more".
                if lane + 1 >= lanes_fit {
                    for h in &mut hidden[c0..=c1] {
                        *h += 1;
                    }
                    continue;
                }
                for u in &mut used[c0..=c1] {
                    u[lane] = true;
                }
                out.push(Item::Bar {
                    idx: span_idx[si],
                    rect: Rectangle::new(
                        Point::new(c0 as f32 * c.width + 3.0, y0 + lane as f32 * LANE),
                        Size::new((c1 - c0 + 1) as f32 * c.width - 6.0, LANE - 2.0),
                    ),
                });
            }
            for col in 0..7 {
                let date = week_start + Duration::days(col as i64);
                let timed: Vec<usize> =
                    self.occs.iter().enumerate().filter(|(_, o)| !o.spans_days() && o.start.date_naive() == date).map(|(i, _)| i).collect();
                let mut lane = 0;
                let mut shown = 0;
                for (k, &i) in timed.iter().enumerate() {
                    while lane < 64 && used[col][lane] {
                        lane += 1;
                    }
                    let remaining = timed.len() - k;
                    let last_lane = lanes_fit - 1;
                    if lane > last_lane || (lane == last_lane && remaining > 1) {
                        break;
                    }
                    out.push(Item::Row {
                        idx: i,
                        rect: Rectangle::new(
                            Point::new(col as f32 * c.width + 3.0, y0 + lane as f32 * LANE),
                            Size::new(c.width - 6.0, LANE - 2.0),
                        ),
                    });
                    used[col][lane] = true;
                    lane += 1;
                    shown += 1;
                }
                let more = timed.len() - shown + hidden[col];
                if more > 0 {
                    let mut l = 0;
                    while l < 64 && used[col][l] {
                        l += 1;
                    }
                    let l = l.min(lanes_fit - 1);
                    out.push(Item::More {
                        date,
                        count: more,
                        rect: Rectangle::new(
                            Point::new(col as f32 * c.width + 3.0, y0 + l as f32 * LANE),
                            Size::new(c.width - 6.0, LANE - 2.0),
                        ),
                    });
                }
            }
        }
        out
    }

    fn hit(&self, items: &[Item], p: Point) -> Option<usize> {
        items.iter().find_map(|it| match it {
            Item::Bar { idx, rect } | Item::Row { idx, rect } if rect.contains(p) => Some(*idx),
            _ => None,
        })
    }
}

impl<'a> Program<GridMsg> for MonthGrid<'a> {
    type State = State;

    fn update(&self, state: &mut State, event: &Event, bounds: Rectangle, cursor: Cursor) -> Option<canvas::Action<GridMsg>> {
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = cursor.position_in(bounds)?;
                if p.y < HEAD_H {
                    return None;
                }
                let date = self.date_at(p, &bounds);
                let items = self.items(&bounds);
                let now = Instant::now();
                let double = state.last_click.is_some_and(|(t, lp)| now.duration_since(t).as_millis() < 450 && lp.distance(p) < 5.0);
                state.last_click = Some((now, p));
                let hit = self.hit(&items, p);
                if double {
                    state.drag = None;
                    return Some(
                        canvas::Action::publish(match hit {
                            Some(i) => GridMsg::Open(self.occs[i].key.clone()),
                            None => GridMsg::NewOn(date),
                        })
                        .and_capture(),
                    );
                }
                let more = items.iter().find_map(|it| match it {
                    Item::More { date, rect, .. } if rect.contains(p) => Some(*date),
                    _ => None,
                });
                if let Some(d) = more {
                    return Some(canvas::Action::publish(GridMsg::SelectDate(d)).and_capture());
                }
                match hit {
                    Some(i) if !self.occs[i].read_only => {
                        state.drag = Some((i, date, date, false, p));
                        Some(canvas::Action::request_redraw().and_capture())
                    }
                    Some(i) => Some(canvas::Action::publish(GridMsg::Select(Some(self.occs[i].key.clone()))).and_capture()),
                    None => Some(canvas::Action::publish(GridMsg::SelectDate(date)).and_capture()),
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let (_, _, cur, moved, origin) = state.drag.as_mut()?;
                let p = cursor.position_from(bounds.position())?;
                if !*moved && origin.distance(p) < 4.0 {
                    return None;
                }
                *moved = true;
                *cur = self.date_at(p, &bounds);
                Some(canvas::Action::request_redraw())
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let (i, grab, cur, moved, _) = state.drag.take()?;
                let o = self.occs[i];
                let delta = (cur - grab).num_days();
                let msg = if !moved || delta == 0 {
                    GridMsg::Select(Some(o.key.clone()))
                } else {
                    GridMsg::Change { key: o.key.clone(), start: o.start + Duration::days(delta), end: o.end + Duration::days(delta) }
                };
                Some(canvas::Action::publish(msg).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, state: &State, renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: Cursor) -> Vec<Geometry> {
        let p = &self.palette;
        let mut frame = Frame::new(renderer, bounds.size());
        let c = self.cell(&bounds);
        grid::rect(&mut frame, Rectangle::new(Point::ORIGIN, bounds.size()), p.bg);

        // Weekday header
        for col in 0..7 {
            let d = self.first + Duration::days(col);
            let name = d.format("%a").to_string().to_uppercase();
            grid::text_right(&mut frame, &name, Point::new((col + 1) as f32 * c.width - 10.0, HEAD_H / 2.0), 11.0, p.muted, BOLD);
        }

        let drag_target = state.drag.filter(|d| d.3).map(|d| d.2);
        for row in 0..6 {
            for col in 0..7 {
                let date = self.first + Duration::days(row * 7 + col);
                let x = col as f32 * c.width;
                let y = HEAD_H + row as f32 * c.height;
                let in_month = date.month() == self.month;
                let weekend = matches!(date.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun);
                let mut bg = if !in_month {
                    theme::mix(p.bg, p.surface_alt, if p.dark { 0.35 } else { 0.5 })
                } else if weekend {
                    p.weekend
                } else {
                    p.bg
                };
                if date == self.selected_date {
                    bg = theme::mix(bg, p.selection, 0.6);
                }
                if drag_target == Some(date) {
                    bg = theme::with_alpha(p.today, 0.18);
                }
                grid::rect(&mut frame, Rectangle::new(Point::new(x, y), c), bg);

                let num = if date.day() == 1 { date.format("%b %-d").to_string() } else { date.format("%-d").to_string() };
                let color = if in_month { p.text } else { p.faint };
                if date == self.today {
                    let w = (num.chars().count() as f32 * 7.5 + 14.0).max(24.0);
                    let r = Rectangle::new(Point::new(x + c.width - w - 5.0, y + 4.0), Size::new(w, 19.0));
                    grid::rounded(&mut frame, r, 9.5, p.accent);
                    grid::text_centered(&mut frame, &num, Point::new(r.x + w / 2.0, r.y + 9.5), 12.5, iced::Color::WHITE, BOLD);
                } else {
                    grid::text_right(
                        &mut frame,
                        &num,
                        Point::new(x + c.width - 10.0, y + 13.5),
                        12.5,
                        color,
                        if date.day() == 1 { BOLD } else { iced::Font::DEFAULT },
                    );
                }
            }
        }
        for row in 0..=6 {
            grid::hline(&mut frame, HEAD_H + row as f32 * c.height, 0.0, bounds.width, p.grid);
        }
        for col in 1..7 {
            grid::vline(&mut frame, col as f32 * c.width, HEAD_H, bounds.height, p.grid);
        }

        let dragging = state.drag.filter(|d| d.3).map(|d| d.0);
        for it in self.items(&bounds) {
            match it {
                Item::Bar { idx, rect } => {
                    let o = self.occs[idx];
                    let sel = self.selected.is_some_and(|k| *k == o.key);
                    let mut fill = if sel { o.color } else { theme::event_fill(p, o.color) };
                    if dragging == Some(idx) {
                        fill = theme::with_alpha(fill, 0.4);
                    }
                    grid::rounded(&mut frame, rect, 4.0, fill);
                    let fg = if sel { iced::Color::WHITE } else { theme::event_text(p, o.color) };
                    grid::text(&mut frame, &o.title, Point::new(rect.x + 6.0, rect.y + 2.0), 11.5, fg, BOLD, rect.width - 8.0);
                }
                Item::Row { idx, rect } => {
                    let o = self.occs[idx];
                    let sel = self.selected.is_some_and(|k| *k == o.key);
                    if sel {
                        grid::rounded(&mut frame, rect, 4.0, o.color);
                    } else if dragging == Some(idx) {
                        grid::rounded(&mut frame, rect, 4.0, theme::with_alpha(o.color, 0.2));
                    }
                    let fg = if sel { iced::Color::WHITE } else { p.text };
                    frame.fill(
                        &Path::circle(Point::new(rect.x + 6.0, rect.y + rect.height / 2.0), 3.5),
                        if sel { iced::Color::WHITE } else { o.color },
                    );
                    let time = grid::fmt_time(o.start, self.time_24h);
                    let time_w = time.chars().count() as f32 * 6.2 + 6.0;
                    let title_w = rect.width - 16.0 - time_w;
                    grid::text(&mut frame, &o.title, Point::new(rect.x + 14.0, rect.y + 2.0), 11.5, fg, BOLD, title_w);
                    grid::text_right(
                        &mut frame,
                        &time,
                        Point::new(rect.x + rect.width - 4.0, rect.y + rect.height / 2.0),
                        10.5,
                        if sel { fg } else { p.muted },
                        iced::Font::DEFAULT,
                    );
                }
                Item::More { count, rect, .. } => {
                    grid::text(
                        &mut frame,
                        &format!("+{count} more"),
                        Point::new(rect.x + 6.0, rect.y + 2.0),
                        11.0,
                        p.muted,
                        BOLD,
                        rect.width - 8.0,
                    );
                }
            }
        }

        // Ghost under cursor while dragging
        if let Some((idx, grab, cur, true, _)) = state.drag {
            let o = self.occs[idx];
            let delta = (cur - grab).num_days();
            let d = o.start.date_naive() + Duration::days(delta);
            let off = (d - self.first).num_days();
            if (0..42).contains(&off) {
                let r = Rectangle::new(
                    Point::new((off % 7) as f32 * c.width + 3.0, HEAD_H + (off / 7) as f32 * c.height + TOP),
                    Size::new(c.width - 6.0, LANE - 2.0),
                );
                grid::rounded(&mut frame, r, 4.0, o.color);
                grid::text(&mut frame, &o.title, Point::new(r.x + 6.0, r.y + 2.0), 11.5, iced::Color::WHITE, BOLD, r.width - 8.0);
            }
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, state: &State, bounds: Rectangle, cursor: Cursor) -> mouse::Interaction {
        if state.drag.is_some_and(|d| d.3) {
            return mouse::Interaction::Grabbing;
        }
        let Some(p) = cursor.position_in(bounds) else { return mouse::Interaction::default() };
        let items = self.items(&bounds);
        if self.hit(&items, p).is_some() || items.iter().any(|i| matches!(i, Item::More { rect, .. } if rect.contains(p))) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}
