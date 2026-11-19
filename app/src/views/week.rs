//! Day/Week time grid: drag to move, drag bottom edge to resize, drag empty
//! space to create, double-click to open.

use chrono::{DateTime, Duration, Local, NaiveDate};
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Event, Frame, Geometry, Path, Program, Stroke};
use iced::{Point, Rectangle, Renderer, Size, Theme};
use std::time::Instant;

use super::grid::{self, BOLD, GridMsg};
use crate::model::{Occ, OccKey};
use crate::theme::{self, Palette};
use calen_core::event::local_midnight;

pub const HOUR_H: f32 = 52.0;
pub const GUTTER: f32 = 58.0;
const SNAP_MIN: i64 = 15;
const RESIZE_GRIP: f32 = 7.0;

pub struct WeekGrid<'a> {
    pub days: Vec<NaiveDate>,
    pub occs: Vec<&'a Occ>,
    pub palette: Palette,
    pub selected: Option<&'a OccKey>,
    pub now: DateTime<Local>,
    pub time_24h: bool,
}

#[derive(Debug, Clone, Copy)]
enum Drag {
    Move { idx: usize, grab_min: i64, origin: Point, moved: bool, day: usize, start_min: i64 },
    Resize { idx: usize, end_min: i64 },
    Create { day: usize, anchor: i64, cur: i64, moved: bool },
}

#[derive(Default)]
pub struct State {
    drag: Option<Drag>,
    last_click: Option<(Instant, Point)>,
}

#[derive(Debug, Clone, Copy)]
struct Block {
    idx: usize,
    day: usize,
    rect: Rectangle,
}

impl<'a> WeekGrid<'a> {
    pub fn height() -> f32 {
        HOUR_H * 24.0
    }

    fn col_w(&self, bounds: &Rectangle) -> f32 {
        ((bounds.width - GUTTER) / self.days.len() as f32).max(1.0)
    }

    fn day_at(&self, x: f32, bounds: &Rectangle) -> usize {
        (((x - GUTTER) / self.col_w(bounds)).floor().max(0.0) as usize).min(self.days.len() - 1)
    }

    fn min_at(y: f32) -> i64 {
        ((y / HOUR_H) * 60.0).round() as i64
    }

    fn snap(m: i64) -> i64 {
        ((m as f64 / SNAP_MIN as f64).round() as i64 * SNAP_MIN).clamp(0, 24 * 60)
    }

    fn at(&self, day: usize, min: i64) -> DateTime<Local> {
        local_midnight(self.days[day]) + Duration::minutes(min)
    }

    /// Lay out timed occurrences per day with side-by-side columns for overlaps.
    fn layout(&self, bounds: &Rectangle) -> Vec<Block> {
        let col_w = self.col_w(bounds);
        let mut out = Vec::new();
        for (d, day) in self.days.iter().enumerate() {
            let day_start = local_midnight(*day);
            let day_end = local_midnight(*day + Duration::days(1));
            let mut items: Vec<(usize, i64, i64)> = self
                .occs
                .iter()
                .enumerate()
                .filter(|(_, o)| o.start < day_end && o.end.max(o.start + Duration::minutes(1)) > day_start)
                .map(|(i, o)| {
                    let s = (o.start.max(day_start) - day_start).num_minutes();
                    let e = (o.end.min(day_end) - day_start).num_minutes().max(s + 20);
                    (i, s, e)
                })
                .collect();
            items.sort_by_key(|&(_, s, e)| (s, -e));

            let mut cluster: Vec<(usize, i64, i64, usize)> = Vec::new();
            let mut col_ends: Vec<i64> = Vec::new();
            let mut cluster_end = i64::MIN;
            let flush = |cluster: &mut Vec<(usize, i64, i64, usize)>, ncols: usize, out: &mut Vec<Block>| {
                for &(i, s, e, c) in cluster.iter() {
                    let w = col_w / ncols as f32;
                    let x = GUTTER + d as f32 * col_w + c as f32 * w + 1.0;
                    let y = s as f32 / 60.0 * HOUR_H + 1.0;
                    let h = ((e - s) as f32 / 60.0 * HOUR_H - 2.0).max(16.0);
                    out.push(Block { idx: i, day: d, rect: Rectangle::new(Point::new(x, y), Size::new((w - 3.0).max(4.0), h)) });
                }
                cluster.clear();
            };
            for (i, s, e) in items {
                if s >= cluster_end && !cluster.is_empty() {
                    flush(&mut cluster, col_ends.len(), &mut out);
                    col_ends.clear();
                }
                let col = match col_ends.iter().position(|&end| end <= s) {
                    Some(c) => {
                        col_ends[c] = e;
                        c
                    }
                    None => {
                        col_ends.push(e);
                        col_ends.len() - 1
                    }
                };
                cluster_end = cluster_end.max(e);
                cluster.push((i, s, e, col));
            }
            if !cluster.is_empty() {
                flush(&mut cluster, col_ends.len(), &mut out);
            }
        }
        out
    }

    fn hit(&self, blocks: &[Block], p: Point) -> Option<Block> {
        blocks.iter().rev().find(|b| b.rect.contains(p)).copied()
    }

    fn dragged_times(&self, drag: &Drag) -> Option<(usize, DateTime<Local>, DateTime<Local>)> {
        match *drag {
            Drag::Move { idx, moved: true, day, start_min, .. } => {
                let o = self.occs[idx];
                let start = self.at(day, start_min);
                Some((idx, start, start + (o.end - o.start)))
            }
            Drag::Resize { idx, end_min } => {
                let o = self.occs[idx];
                let day = o.start.date_naive();
                let end = local_midnight(day) + Duration::minutes(end_min);
                Some((idx, o.start, end.max(o.start + Duration::minutes(SNAP_MIN))))
            }
            _ => None,
        }
    }

    fn draw_block(&self, frame: &mut Frame, o: &Occ, r: Rectangle, ghost: bool) {
        let p = &self.palette;
        let selected = self.selected.is_some_and(|k| *k == o.key);
        let mut fill = if selected { o.color } else { theme::event_fill(p, o.color) };
        if ghost {
            fill = theme::with_alpha(fill, 0.85);
        }
        grid::rounded(frame, r, 5.0, fill);
        grid::rounded(frame, Rectangle::new(r.position(), Size::new(3.0, r.height)), 1.5, o.color);
        if ghost {
            let path = Path::rounded_rectangle(r.position(), r.size(), 5.0.into());
            frame.stroke(&path, Stroke::default().with_color(o.color).with_width(1.5));
        }
        let fg = if selected { iced::Color::WHITE } else { theme::event_text(p, o.color) };
        let sub = if selected { theme::with_alpha(iced::Color::WHITE, 0.85) } else { theme::with_alpha(fg, 0.75) };
        let x = r.x + 7.0;
        let w = r.width - 10.0;
        let mut y = r.y + 3.0;
        let line = 14.0;
        let time = format!("{} – {}", grid::fmt_time(o.start, self.time_24h), grid::fmt_time(o.end, self.time_24h));
        if r.height < 30.0 {
            // One line: "Title, 10:00"
            let t = format!("{}  {}", o.title, grid::fmt_time(o.start, self.time_24h));
            grid::text(frame, &t, Point::new(x, y), 11.5, fg, BOLD, w);
            return;
        }
        grid::text(frame, &time, Point::new(x, y), 11.0, sub, iced::Font::DEFAULT, w - if o.has_meeting { 14.0 } else { 0.0 });
        if o.has_meeting && w > 40.0 {
            grid::rounded(frame, Rectangle::new(Point::new(r.x + r.width - 13.0, y + 3.0), Size::new(8.0, 8.0)), 4.0, o.color);
        }
        y += line;
        // Title wraps onto as many lines as fit.
        let max_lines = (((r.y + r.height - 4.0) - y) / line).floor().max(1.0) as usize;
        let title_lines = wrap(&o.title, w, 12.0, max_lines.saturating_sub(if o.location.is_empty() { 0 } else { 1 }).max(1));
        for l in &title_lines {
            if y + line > r.y + r.height + 2.0 {
                break;
            }
            grid::text(frame, l, Point::new(x, y), 12.0, fg, BOLD, w);
            y += line;
        }
        if !o.location.is_empty() && y + line <= r.y + r.height + 2.0 {
            grid::text(frame, &o.location, Point::new(x, y), 11.0, sub, iced::Font::DEFAULT, w);
        }
    }
}

/// Greedy word wrap into at most `max_lines` lines (last line ellipsized).
pub fn wrap(s: &str, max_w: f32, size: f32, max_lines: usize) -> Vec<String> {
    let per_line = ((max_w / (size * 0.56)).floor() as usize).max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let candidate = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if candidate.chars().count() <= per_line || cur.is_empty() {
            cur = candidate;
        } else {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.len() > max_lines {
        let rest = lines[max_lines - 1..].join(" ");
        lines.truncate(max_lines - 1);
        lines.push(rest);
    }
    lines.into_iter().map(|l| grid::ellipsize(&l, max_w, size)).collect()
}

impl<'a> Program<GridMsg> for WeekGrid<'a> {
    type State = State;

    fn update(&self, state: &mut State, event: &Event, bounds: Rectangle, cursor: Cursor) -> Option<canvas::Action<GridMsg>> {
        let pos = cursor.position_in(bounds);
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = pos?;
                if p.x < GUTTER {
                    return None;
                }
                let blocks = self.layout(&bounds);
                let now = Instant::now();
                let double = state.last_click.is_some_and(|(t, lp)| now.duration_since(t).as_millis() < 450 && lp.distance(p) < 5.0);
                state.last_click = Some((now, p));
                let hit = self.hit(&blocks, p);
                if double {
                    state.drag = None;
                    state.last_click = None;
                    return Some(
                        match hit {
                            Some(b) => canvas::Action::publish(GridMsg::Open(self.occs[b.idx].key.clone())),
                            None => {
                                let day = self.day_at(p.x, &bounds);
                                let m = (Self::min_at(p.y) / 30) * 30;
                                let start = self.at(day, m);
                                canvas::Action::publish(GridMsg::Create { start, end: start + Duration::hours(1), all_day: false })
                            }
                        }
                        .and_capture(),
                    );
                }
                state.drag = Some(match hit {
                    Some(b) => {
                        let o = self.occs[b.idx];
                        if o.read_only {
                            return Some(canvas::Action::publish(GridMsg::Select(Some(o.key.clone()))).and_capture());
                        }
                        let near_bottom = p.y > b.rect.y + b.rect.height - RESIZE_GRIP && b.rect.height > 18.0;
                        if near_bottom && o.end.date_naive() == o.start.date_naive()
                            || near_bottom && o.end == local_midnight(o.start.date_naive() + Duration::days(1))
                        {
                            Drag::Resize { idx: b.idx, end_min: (o.end - local_midnight(o.start.date_naive())).num_minutes() }
                        } else {
                            let start_of_day = local_midnight(self.days[b.day]);
                            let start_min = (o.start - start_of_day).num_minutes();
                            Drag::Move {
                                idx: b.idx,
                                grab_min: Self::min_at(p.y) - start_min,
                                origin: p,
                                moved: false,
                                day: b.day,
                                start_min,
                            }
                        }
                    }
                    None => {
                        let m = Self::snap(Self::min_at(p.y) - SNAP_MIN / 2);
                        Drag::Create { day: self.day_at(p.x, &bounds), anchor: m, cur: m, moved: false }
                    }
                });
                Some(canvas::Action::request_redraw().and_capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let drag = state.drag.as_mut()?;
                let p = cursor.position_from(bounds.position())?;
                match drag {
                    Drag::Move { grab_min, origin, moved, day, start_min, .. } => {
                        if !*moved && origin.distance(p) < 4.0 {
                            return None;
                        }
                        *moved = true;
                        *day = self.day_at(p.x, &bounds);
                        *start_min = Self::snap(Self::min_at(p.y) - *grab_min).clamp(0, 24 * 60 - SNAP_MIN);
                    }
                    Drag::Resize { idx, end_min } => {
                        let o = self.occs[*idx];
                        let start_min = (o.start - local_midnight(o.start.date_naive())).num_minutes();
                        *end_min = Self::snap(Self::min_at(p.y)).max(start_min + SNAP_MIN);
                    }
                    Drag::Create { cur, moved, anchor, .. } => {
                        *cur = Self::snap(Self::min_at(p.y));
                        if (*cur - *anchor).abs() >= SNAP_MIN {
                            *moved = true;
                        }
                    }
                }
                Some(canvas::Action::request_redraw())
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let drag = state.drag.take()?;
                let msg = match drag {
                    Drag::Move { idx, moved: false, .. } => GridMsg::Select(Some(self.occs[idx].key.clone())),
                    Drag::Move { idx, .. } | Drag::Resize { idx, .. } => {
                        let (_, start, end) = self.dragged_times(&drag)?;
                        let o = self.occs[idx];
                        if start == o.start && end == o.end {
                            GridMsg::Select(Some(o.key.clone()))
                        } else {
                            GridMsg::Change { key: o.key.clone(), start, end }
                        }
                    }
                    Drag::Create { day, moved: false, .. } => {
                        let _ = day;
                        GridMsg::Select(None)
                    }
                    Drag::Create { day, anchor, cur, .. } => {
                        let (a, b) = if cur < anchor { (cur, anchor) } else { (anchor, cur) };
                        GridMsg::Create { start: self.at(day, a), end: self.at(day, b.max(a + SNAP_MIN)), all_day: false }
                    }
                };
                Some(canvas::Action::publish(msg).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, state: &State, renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: Cursor) -> Vec<Geometry> {
        let p = &self.palette;
        let mut frame = Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let col_w = self.col_w(&bounds);
        let today = self.now.date_naive();

        // Gutter background. Besides the colour, it makes the CPU renderer repaint
        // the hour labels when the grid scrolls: it repaints where shapes are, and
        // compares text without the scroll offset, so labels on an empty gutter
        // kept their old positions.
        grid::rect(&mut frame, Rectangle::new(Point::ORIGIN, Size::new(GUTTER, bounds.height)), p.bg);

        // Column backgrounds
        for (d, day) in self.days.iter().enumerate() {
            let x = GUTTER + d as f32 * col_w;
            let bg = if *day == today {
                theme::with_alpha(p.today, if p.dark { 0.07 } else { 0.05 })
            } else if matches!(chrono::Datelike::weekday(day), chrono::Weekday::Sat | chrono::Weekday::Sun) {
                p.weekend
            } else {
                p.bg
            };
            grid::rect(&mut frame, Rectangle::new(Point::new(x, 0.0), Size::new(col_w, bounds.height)), bg);
        }

        // Hour lines + labels
        for h in 0..24 {
            let y = h as f32 * HOUR_H;
            grid::hline(&mut frame, y, GUTTER, w, p.grid);
            grid::hline(&mut frame, y + HOUR_H / 2.0, GUTTER, w, theme::with_alpha(p.grid, 0.45));
            if h > 0 {
                let label = if self.time_24h {
                    format!("{h:02}:00")
                } else {
                    match h {
                        12 => "noon".to_string(),
                        h if h < 12 => format!("{h} AM"),
                        h => format!("{} PM", h - 12),
                    }
                };
                grid::text_right(&mut frame, &label, Point::new(GUTTER - 8.0, y), 11.0, p.muted, iced::Font::DEFAULT);
            }
        }
        for d in 0..=self.days.len() {
            grid::vline(&mut frame, GUTTER + d as f32 * col_w, 0.0, bounds.height, p.grid);
        }

        // Events
        let blocks = self.layout(&bounds);
        let dragging_idx = state.drag.as_ref().and_then(|d| self.dragged_times(d)).map(|(i, _, _)| i);
        for b in &blocks {
            let o = self.occs[b.idx];
            if dragging_idx == Some(b.idx) {
                // Leave a faint placeholder where it was.
                grid::rounded(&mut frame, b.rect, 5.0, theme::with_alpha(o.color, 0.18));
                continue;
            }
            self.draw_block(&mut frame, o, b.rect, false);
        }

        // Drag ghost
        if let Some((idx, start, end)) = state.drag.as_ref().and_then(|d| self.dragged_times(d)) {
            let o = self.occs[idx];
            if let Some(day) = self.days.iter().position(|d| *d == start.date_naive()) {
                let s = (start - local_midnight(self.days[day])).num_minutes() as f32;
                let e = (end - local_midnight(self.days[day])).num_minutes() as f32;
                let r = Rectangle::new(
                    Point::new(GUTTER + day as f32 * col_w + 2.0, s / 60.0 * HOUR_H + 1.0),
                    Size::new(col_w - 5.0, ((e - s) / 60.0 * HOUR_H - 2.0).max(16.0)),
                );
                let mut ghost = o.clone();
                ghost.start = start;
                ghost.end = end;
                self.draw_block(&mut frame, &ghost, r, true);
            }
        }

        // New-event selection
        if let Some(Drag::Create { day, anchor, cur, moved: true }) = state.drag {
            let (a, b) = if cur < anchor { (cur, anchor) } else { (anchor, cur) };
            let r = Rectangle::new(
                Point::new(GUTTER + day as f32 * col_w + 2.0, a as f32 / 60.0 * HOUR_H),
                Size::new(col_w - 5.0, ((b - a) as f32 / 60.0 * HOUR_H).max(4.0)),
            );
            grid::rounded(&mut frame, r, 5.0, theme::with_alpha(p.today, 0.25));
            let label = format!("{} – {}", grid::fmt_time(self.at(day, a), self.time_24h), grid::fmt_time(self.at(day, b), self.time_24h));
            grid::text(&mut frame, &label, Point::new(r.x + 6.0, r.y + 3.0), 11.0, p.text, BOLD, r.width - 8.0);
        }

        // Now line
        if let Some(d) = self.days.iter().position(|d| *d == today) {
            let mins = (self.now - local_midnight(today)).num_minutes() as f32;
            let y = mins / 60.0 * HOUR_H;
            let x0 = GUTTER + d as f32 * col_w;
            grid::rect(
                &mut frame,
                Rectangle::new(Point::new(GUTTER, y - 0.5), Size::new(w - GUTTER, 1.0)),
                theme::with_alpha(p.now_line, 0.35),
            );
            grid::rect(&mut frame, Rectangle::new(Point::new(x0, y - 1.0), Size::new(col_w, 2.0)), p.now_line);
            frame.fill(&Path::circle(Point::new(x0, y), 4.5), p.now_line);
            let label = grid::fmt_time(self.now, self.time_24h);
            grid::rounded(&mut frame, Rectangle::new(Point::new(4.0, y - 8.0), Size::new(GUTTER - 10.0, 16.0)), 8.0, p.now_line);
            grid::text_centered(&mut frame, &label, Point::new(4.0 + (GUTTER - 10.0) / 2.0, y), 10.5, iced::Color::WHITE, BOLD);
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, state: &State, bounds: Rectangle, cursor: Cursor) -> mouse::Interaction {
        match state.drag {
            Some(Drag::Move { moved: true, .. }) => return mouse::Interaction::Grabbing,
            Some(Drag::Resize { .. }) => return mouse::Interaction::ResizingVertically,
            Some(Drag::Create { .. }) => return mouse::Interaction::Crosshair,
            _ => {}
        }
        let Some(p) = cursor.position_in(bounds) else { return mouse::Interaction::default() };
        let blocks = self.layout(&bounds);
        match self.hit(&blocks, p) {
            Some(b) if self.occs[b.idx].read_only => mouse::Interaction::Pointer,
            Some(b) if p.y > b.rect.y + b.rect.height - RESIZE_GRIP && b.rect.height > 18.0 => mouse::Interaction::ResizingVertically,
            Some(_) => mouse::Interaction::Grab,
            None => mouse::Interaction::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Header: day names + all-day / multi-day lane bars (drag horizontally).

pub struct AllDayHeader<'a> {
    pub days: Vec<NaiveDate>,
    pub occs: Vec<&'a Occ>,
    pub palette: Palette,
    pub selected: Option<&'a OccKey>,
    pub today: NaiveDate,
}

pub const HEADER_H: f32 = 52.0;
pub const LANE_H: f32 = 22.0;

#[derive(Default)]
pub struct HeaderState {
    drag: Option<(usize, usize, usize, bool, Point)>, // occ idx, grab col, current col, moved, origin
    last_click: Option<(Instant, Point)>,
}

impl<'a> AllDayHeader<'a> {
    pub fn height(occs: &[&Occ], days: &[NaiveDate]) -> f32 {
        let lanes = grid::bar_lanes(occs, days[0], days.len()).iter().map(|l| l.3 + 1).max().unwrap_or(0);
        HEADER_H + (lanes.max(1) as f32) * LANE_H + 6.0
    }

    fn col_w(&self, b: &Rectangle) -> f32 {
        ((b.width - GUTTER) / self.days.len() as f32).max(1.0)
    }

    fn bars(&self, b: &Rectangle) -> Vec<(usize, Rectangle)> {
        let cw = self.col_w(b);
        grid::bar_lanes(&self.occs, self.days[0], self.days.len())
            .into_iter()
            .map(|(i, c0, c1, lane)| {
                let r = Rectangle::new(
                    Point::new(GUTTER + c0 as f32 * cw + 2.0, HEADER_H + lane as f32 * LANE_H + 1.0),
                    Size::new((c1 - c0 + 1) as f32 * cw - 5.0, LANE_H - 3.0),
                );
                (i, r)
            })
            .collect()
    }

    fn col_at(&self, x: f32, b: &Rectangle) -> usize {
        (((x - GUTTER) / self.col_w(b)).floor().max(0.0) as usize).min(self.days.len() - 1)
    }
}

impl<'a> Program<GridMsg> for AllDayHeader<'a> {
    type State = HeaderState;

    fn update(&self, state: &mut HeaderState, event: &Event, bounds: Rectangle, cursor: Cursor) -> Option<canvas::Action<GridMsg>> {
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = cursor.position_in(bounds)?;
                if p.x < GUTTER {
                    return None;
                }
                let col = self.col_at(p.x, &bounds);
                let now = Instant::now();
                let double = state.last_click.is_some_and(|(t, lp)| now.duration_since(t).as_millis() < 450 && lp.distance(p) < 5.0);
                state.last_click = Some((now, p));
                let hit = self.bars(&bounds).into_iter().find(|(_, r)| r.contains(p));
                if double {
                    state.drag = None;
                    return Some(
                        canvas::Action::publish(match hit {
                            Some((i, _)) => GridMsg::Open(self.occs[i].key.clone()),
                            None => {
                                let start = local_midnight(self.days[col]);
                                GridMsg::Create { start, end: start + Duration::days(1), all_day: true }
                            }
                        })
                        .and_capture(),
                    );
                }
                if p.y < HEADER_H {
                    return Some(canvas::Action::publish(GridMsg::SelectDate(self.days[col])).and_capture());
                }
                match hit {
                    Some((i, _)) => {
                        if self.occs[i].read_only {
                            return Some(canvas::Action::publish(GridMsg::Select(Some(self.occs[i].key.clone()))).and_capture());
                        }
                        state.drag = Some((i, col, col, false, p));
                        Some(canvas::Action::request_redraw().and_capture())
                    }
                    None => Some(canvas::Action::publish(GridMsg::Select(None)).and_capture()),
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let (_, _, cur, moved, origin) = state.drag.as_mut()?;
                let p = cursor.position_from(bounds.position())?;
                if !*moved && origin.distance(p) < 4.0 {
                    return None;
                }
                *moved = true;
                *cur = self.col_at(p.x, &bounds);
                Some(canvas::Action::request_redraw())
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let (i, grab, cur, moved, _) = state.drag.take()?;
                let o = self.occs[i];
                let delta = cur as i64 - grab as i64;
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

    fn draw(&self, state: &HeaderState, renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: Cursor) -> Vec<Geometry> {
        let p = &self.palette;
        let mut frame = Frame::new(renderer, bounds.size());
        let cw = self.col_w(&bounds);
        grid::rect(&mut frame, Rectangle::new(Point::ORIGIN, bounds.size()), p.bg);

        for (d, day) in self.days.iter().enumerate() {
            let x = GUTTER + d as f32 * cw;
            let is_today = *day == self.today;
            let wd = day.format("%a").to_string().to_uppercase();
            grid::text(&mut frame, &wd, Point::new(x + 8.0, 8.0), 11.0, if is_today { p.today } else { p.muted }, BOLD, cw - 10.0);
            let num = day.format("%-d").to_string();
            if is_today {
                frame.fill(&Path::circle(Point::new(x + 22.0, 34.0), 14.0), p.accent);
                grid::text_centered(&mut frame, &num, Point::new(x + 22.0, 34.0), 16.0, iced::Color::WHITE, BOLD);
            } else {
                grid::text(&mut frame, &num, Point::new(x + 8.0, 23.0), 20.0, p.text, iced::Font::DEFAULT, cw - 10.0);
            }
            grid::vline(&mut frame, x, HEADER_H - 4.0, bounds.height, p.grid);
        }
        grid::text_right(&mut frame, "all-day", Point::new(GUTTER - 8.0, HEADER_H + LANE_H / 2.0), 10.5, p.muted, iced::Font::DEFAULT);
        grid::hline(&mut frame, bounds.height - 1.0, 0.0, bounds.width, p.grid_strong);

        let drag = state.drag.filter(|d| d.3);
        for (i, r) in self.bars(&bounds) {
            let o = self.occs[i];
            let mut r = r;
            if let Some((di, grab, cur, _, _)) = drag
                && di == i
            {
                grid::rounded(&mut frame, r, 4.0, theme::with_alpha(o.color, 0.18));
                r.x += (cur as f32 - grab as f32) * cw;
            }
            let selected = self.selected.is_some_and(|k| *k == o.key);
            let fill = if selected { o.color } else { theme::event_fill(p, o.color) };
            grid::rounded(&mut frame, r, 4.0, fill);
            let fg = if selected { iced::Color::WHITE } else { theme::event_text(p, o.color) };
            grid::text(&mut frame, &o.title, Point::new(r.x + 7.0, r.y + 3.0), 12.0, fg, BOLD, r.width - 10.0);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, state: &HeaderState, bounds: Rectangle, cursor: Cursor) -> mouse::Interaction {
        if state.drag.is_some_and(|d| d.3) {
            return mouse::Interaction::Grabbing;
        }
        let Some(p) = cursor.position_in(bounds) else { return mouse::Interaction::default() };
        if self.bars(&bounds).iter().any(|(_, r)| r.contains(p)) || (p.y < HEADER_H && p.x > GUTTER) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}
