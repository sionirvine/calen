//! Full event editor (opened with Enter / double-click / Tab from quick add).

use calen_core::quickadd::{self, DateOrder};
use calen_core::{Event, When, links, notes};
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveTime};
use iced::widget::{
    Space, button, checkbox, column, container, image, mouse_area, pick_list, row, scrollable, text, text_editor, text_input,
};
use iced::{Alignment, Element, Font, Length, font};

use crate::edit::{Fields, Target};
use crate::icons;
use crate::images::ImageCache;
use crate::theme::{self, Palette};

#[derive(Debug, Clone, PartialEq)]
pub struct CalChoice {
    pub id: String,
    pub name: String,
    pub color: iced::Color,
}

impl std::fmt::Display for CalChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AlertChoice {
    None,
    Minutes(i64),
}

impl AlertChoice {
    pub const ALL: [AlertChoice; 9] = [
        AlertChoice::None,
        AlertChoice::Minutes(0),
        AlertChoice::Minutes(5),
        AlertChoice::Minutes(10),
        AlertChoice::Minutes(15),
        AlertChoice::Minutes(30),
        AlertChoice::Minutes(60),
        AlertChoice::Minutes(120),
        AlertChoice::Minutes(1440),
    ];
}

impl std::fmt::Display for AlertChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlertChoice::None => f.write_str("None"),
            AlertChoice::Minutes(0) => f.write_str("At start"),
            AlertChoice::Minutes(m) if m % 1440 == 0 => write!(f, "{} day before", m / 1440),
            AlertChoice::Minutes(m) if m % 60 == 0 => write!(f, "{} h before", m / 60),
            AlertChoice::Minutes(m) => write!(f, "{m} min before"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RepeatChoice {
    Never,
    Daily,
    Weekdays,
    Weekly,
    Biweekly,
    Monthly,
    Yearly,
    Custom(String),
}

impl RepeatChoice {
    pub fn from_rule(r: Option<&str>) -> Self {
        match r.map(|s| s.trim().to_ascii_uppercase()) {
            None => RepeatChoice::Never,
            Some(s) => match s.as_str() {
                "" => RepeatChoice::Never,
                "FREQ=DAILY" => RepeatChoice::Daily,
                "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR" => RepeatChoice::Weekdays,
                "FREQ=WEEKLY" => RepeatChoice::Weekly,
                "FREQ=WEEKLY;INTERVAL=2" => RepeatChoice::Biweekly,
                "FREQ=MONTHLY" => RepeatChoice::Monthly,
                "FREQ=YEARLY" => RepeatChoice::Yearly,
                _ => RepeatChoice::Custom(s),
            },
        }
    }

    pub fn rule(&self) -> Option<String> {
        Some(
            match self {
                RepeatChoice::Never => return None,
                RepeatChoice::Daily => "FREQ=DAILY",
                RepeatChoice::Weekdays => "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR",
                RepeatChoice::Weekly => "FREQ=WEEKLY",
                RepeatChoice::Biweekly => "FREQ=WEEKLY;INTERVAL=2",
                RepeatChoice::Monthly => "FREQ=MONTHLY",
                RepeatChoice::Yearly => "FREQ=YEARLY",
                RepeatChoice::Custom(s) => s,
            }
            .to_string(),
        )
    }

    pub fn options(current: &RepeatChoice) -> Vec<RepeatChoice> {
        let mut v = vec![
            RepeatChoice::Never,
            RepeatChoice::Daily,
            RepeatChoice::Weekdays,
            RepeatChoice::Weekly,
            RepeatChoice::Biweekly,
            RepeatChoice::Monthly,
            RepeatChoice::Yearly,
        ];
        if let RepeatChoice::Custom(_) = current {
            v.push(current.clone());
        }
        v
    }
}

impl std::fmt::Display for RepeatChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            RepeatChoice::Never => "Never",
            RepeatChoice::Daily => "Every day",
            RepeatChoice::Weekdays => "Every weekday",
            RepeatChoice::Weekly => "Every week",
            RepeatChoice::Biweekly => "Every 2 weeks",
            RepeatChoice::Monthly => "Every month",
            RepeatChoice::Yearly => "Every year",
            RepeatChoice::Custom(_) => "Custom",
        })
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    Title(String),
    Location(String),
    Url(String),
    Notes(text_editor::Action),
    AllDay(bool),
    StartDate(String),
    StartTime(String),
    EndDate(String),
    EndTime(String),
    Calendar(CalChoice),
    Alert(AlertChoice),
    Repeat(RepeatChoice),
    Save,
    Cancel,
    Delete,
    PickImages,
    RemoveImage(usize),
    ViewImage(usize),
    Open(String),
}

pub struct Editor {
    pub target: Option<Target>,
    pub original_cal: Option<String>,
    pub calendar: CalChoice,
    pub title: String,
    pub location: String,
    pub url: String,
    pub notes: text_editor::Content,
    pub images: Vec<String>,
    pub all_day: bool,
    pub start_date: String,
    pub start_time: String,
    pub end_date: String,
    pub end_time: String,
    pub alert: AlertChoice,
    pub repeat: RepeatChoice,
    pub repeat_original: RepeatChoice,
    pub color: Option<String>,
    pub geo: Option<(f64, f64)>,
    pub error: Option<String>,
    pub uploading: usize,
    pub time_24h: bool,
    pub date_order: DateOrder,
}

pub fn fmt_date(d: NaiveDate) -> String {
    d.format("%-d %b %Y").to_string()
}

pub fn fmt_time_input(t: DateTime<Local>, h24: bool) -> String {
    if h24 { t.format("%H:%M").to_string() } else { t.format("%-I:%M %p").to_string() }
}

impl Editor {
    /// Editor for an existing event occurrence.
    pub fn for_event(ev: &Event, target: Target, cal: CalChoice, h24: bool, order: DateOrder) -> Self {
        let (body, images) = notes::split(&ev.description);
        let all_day = ev.all_day();
        let (s, e) = (target.occ_start, target.occ_end);
        let end_shown = if all_day { e - Duration::days(1) } else { e };
        let repeat = RepeatChoice::from_rule(ev.rrule.as_deref());
        Editor {
            original_cal: Some(cal.id.clone()),
            calendar: cal,
            title: ev.summary.clone(),
            location: ev.location.clone(),
            url: ev.url.clone().unwrap_or_default(),
            notes: text_editor::Content::with_text(&body),
            images,
            all_day,
            start_date: fmt_date(s.date_naive()),
            start_time: fmt_time_input(s, h24),
            end_date: fmt_date(end_shown.date_naive()),
            end_time: fmt_time_input(e, h24),
            alert: ev.alarms.first().map(|m| AlertChoice::Minutes(*m)).unwrap_or(AlertChoice::None),
            repeat_original: repeat.clone(),
            repeat,
            color: ev.color.clone(),
            geo: ev.geo,
            error: None,
            uploading: 0,
            time_24h: h24,
            date_order: order,
            target: Some(target),
        }
    }

    /// Editor for a new event.
    #[allow(clippy::too_many_arguments)]
    pub fn new_event(title: &str, start: When, end: When, cal: CalChoice, alert: Option<i64>, h24: bool, order: DateOrder) -> Self {
        let all_day = start.is_date();
        let s = start.local();
        let e = end.local();
        let end_shown = if all_day { e - Duration::days(1) } else { e };
        Editor {
            target: None,
            original_cal: None,
            calendar: cal,
            title: title.to_string(),
            location: String::new(),
            url: String::new(),
            notes: text_editor::Content::new(),
            images: Vec::new(),
            all_day,
            start_date: fmt_date(s.date_naive()),
            start_time: fmt_time_input(s, h24),
            end_date: fmt_date(end_shown.date_naive()),
            end_time: fmt_time_input(e, h24),
            alert: alert.map(AlertChoice::Minutes).unwrap_or(AlertChoice::None),
            repeat: RepeatChoice::Never,
            repeat_original: RepeatChoice::Never,
            color: None,
            geo: None,
            error: None,
            uploading: 0,
            time_24h: h24,
            date_order: order,
        }
    }

    fn parse_date(&self, s: &str) -> Option<NaiveDate> {
        let today = Local::now().date_naive();
        quickadd::parse(s, today, self.date_order).date
    }

    fn parse_time(&self, s: &str) -> Option<NaiveTime> {
        let today = Local::now().date_naive();
        quickadd::parse(&format!("at {s}"), today, self.date_order).start_time
    }

    /// Validate inputs into fields ready for saving.
    pub fn fields(&self) -> Result<Fields, String> {
        let sd = self.parse_date(&self.start_date).ok_or("Couldn't read the start date")?;
        let ed = self.parse_date(&self.end_date).ok_or("Couldn't read the end date")?;
        let (start, end) = if self.all_day {
            if ed < sd {
                return Err("The event ends before it starts".into());
            }
            (When::Date(sd), When::Date(ed + Duration::days(1)))
        } else {
            let st = self.parse_time(&self.start_time).ok_or("Couldn't read the start time")?;
            let et = self.parse_time(&self.end_time).ok_or("Couldn't read the end time")?;
            let s = calen_core::event::local_midnight(sd) + (st - NaiveTime::MIN);
            let e = calen_core::event::local_midnight(ed) + (et - NaiveTime::MIN);
            if e < s {
                return Err("The event ends before it starts".into());
            }
            (When::at_local(s), When::at_local(e))
        };
        let mut location = self.location.trim().to_string();
        let mut url = self.url.trim().to_string();
        let mut geo = self.geo;
        // A pasted Google Maps link becomes a real place name + coordinates.
        if let Some(m) = links::find_maps(&location) {
            if let Some(place) = m.place.clone() {
                location = place;
                if url.is_empty() {
                    url = m.url.clone();
                }
            }
            geo = m.geo.or(geo);
        } else if let Some(m) = links::find_maps(&url) {
            geo = m.geo.or(geo);
            if location.is_empty()
                && let Some(place) = m.place
            {
                location = place;
            }
        }
        // Image links typed/pasted into the notes as a block count too.
        let (body, typed_images) = notes::split(&self.notes.text());
        let mut images = self.images.clone();
        for url in typed_images {
            if !images.contains(&url) {
                images.push(url);
            }
        }
        Ok(Fields {
            title: self.title.trim().to_string(),
            location,
            url: (!url.is_empty()).then_some(url),
            description: notes::join(body.trim_end(), &images),
            images,
            start,
            end,
            alarms: match self.alert {
                AlertChoice::None => vec![],
                AlertChoice::Minutes(m) => vec![m],
            },
            rrule: (self.repeat != self.repeat_original).then(|| self.repeat.rule()),
            color: self.color.clone(),
            geo,
        })
    }

    /// Returns true if the editor wants the app to handle the message (save etc).
    pub fn update(&mut self, msg: &Msg) {
        match msg {
            Msg::Title(s) => self.title = s.clone(),
            Msg::Location(s) => self.location = s.clone(),
            Msg::Url(s) => self.url = s.clone(),
            Msg::Notes(a) => self.notes.perform(a.clone()),
            Msg::AllDay(b) => self.all_day = *b,
            Msg::StartDate(s) => {
                // Keep the event length when moving the start date.
                if let (Some(old), Some(new), Some(end)) =
                    (self.parse_date(&self.start_date), self.parse_date(s), self.parse_date(&self.end_date))
                {
                    let len = end - old;
                    if new != old {
                        self.end_date = fmt_date(new + len);
                    }
                }
                self.start_date = s.clone();
            }
            Msg::StartTime(s) => {
                if let (Some(old), Some(new), Some(end)) =
                    (self.parse_time(&self.start_time), self.parse_time(s), self.parse_time(&self.end_time))
                {
                    let len = end - old;
                    if len > Duration::zero() && new != old {
                        let e = new + len;
                        let dt = Local::now().date_naive().and_time(e);
                        self.end_time = if self.time_24h { dt.format("%H:%M").to_string() } else { dt.format("%-I:%M %p").to_string() };
                    }
                }
                self.start_time = s.clone();
            }
            Msg::EndDate(s) => self.end_date = s.clone(),
            Msg::EndTime(s) => self.end_time = s.clone(),
            Msg::Calendar(c) => self.calendar = c.clone(),
            Msg::Alert(a) => self.alert = *a,
            Msg::Repeat(r) => self.repeat = r.clone(),
            Msg::RemoveImage(i) if *i < self.images.len() => {
                self.images.remove(*i);
            }
            _ => {}
        }
    }

    pub fn view<'a>(
        &'a self,
        p: Palette,
        calendars: Vec<CalChoice>,
        cache: &'a ImageCache,
        drop_hover: bool,
        uploads: bool,
    ) -> Element<'a, Msg> {
        let label =
            |s: &'static str| text(s).size(13).color(p.muted).width(Length::Fixed(72.0)).align_x(iced::alignment::Horizontal::Right);
        let title = text_input("New event", &self.title)
            .id(iced::widget::Id::new("editor-title"))
            .on_input(Msg::Title)
            .on_submit(Msg::Save)
            .size(20)
            .font(Font { weight: font::Weight::Semibold, ..Font::DEFAULT })
            .style(theme::bare_input)
            .padding([4, 0]);

        let loc_links = links::find_maps(&self.location).or_else(|| links::find_maps(&self.url));
        let mut location_row = row![
            icons::icon(icons::map_pin(), 15.0, p.muted),
            text_input("Add location or paste a Google Maps link", &self.location)
                .on_input(Msg::Location)
                .on_submit(Msg::Save)
                .style(theme::bare_input)
                .size(14)
                .padding([4, 0]),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        if let Some(m) = &loc_links {
            let name = m.place.clone().unwrap_or_else(|| "Open map".into());
            location_row = location_row
                .push(button(text(format!("{name} ↗")).size(12)).style(theme::link).padding(0).on_press(Msg::Open(m.url.clone())));
        } else if !self.location.trim().is_empty() {
            let url = links::maps_search_url(self.location.trim(), self.geo);
            location_row = location_row.push(button(text("Map ↗").size(12)).style(theme::link).padding(0).on_press(Msg::Open(url)));
        }

        let meeting = links::find_meeting(&format!("{}\n{}\n{}", self.url, self.location, self.notes.text()));
        let mut url_row = row![
            icons::icon(icons::video(), 15.0, p.muted),
            text_input("Add URL or meeting link", &self.url)
                .on_input(Msg::Url)
                .on_submit(Msg::Save)
                .style(theme::bare_input)
                .size(14)
                .padding([4, 0]),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        if let Some(m) = &meeting {
            url_row = url_row.push(
                button(text(format!("Join {}", m.kind.label())).size(12))
                    .style(theme::join)
                    .padding([3, 10])
                    .on_press(Msg::Open(m.url.clone())),
            );
        }

        let input = |placeholder: &'static str, value: &str, f: fn(String) -> Msg, w: f32| {
            text_input(placeholder, value)
                .on_input(f)
                .on_submit(Msg::Save)
                .style(theme::input)
                .size(13)
                .padding([5, 8])
                .width(Length::Fixed(w))
        };
        let mut starts =
            row![label("starts"), input("date", &self.start_date, Msg::StartDate, 130.0)].spacing(10).align_y(Alignment::Center);
        let mut ends = row![label("ends"), input("date", &self.end_date, Msg::EndDate, 130.0)].spacing(10).align_y(Alignment::Center);
        if !self.all_day {
            starts = starts.push(input("time", &self.start_time, Msg::StartTime, 90.0));
            ends = ends.push(input("time", &self.end_time, Msg::EndTime, 90.0));
        }

        let selected_cal = calendars.iter().find(|c| c.id == self.calendar.id).cloned();
        let cal_dot = container(Space::new().width(10).height(10)).style(theme::dot(self.calendar.color));
        let pick_style = move |theme: &iced::Theme, status| {
            let mut s = pick_list::default(theme, status);
            s.background = p.bg.into();
            s.border = iced::Border { color: p.grid_strong, width: 1.0, radius: 8.0.into() };
            s.text_color = p.text;
            s
        };

        let fields = column![
            row![label("all-day"), checkbox(self.all_day).on_toggle(Msg::AllDay).size(16)].spacing(10).align_y(Alignment::Center),
            starts,
            ends,
            row![
                label("calendar"),
                cal_dot,
                pick_list(calendars, selected_cal, Msg::Calendar).text_size(13).padding([5, 8]).style(pick_style)
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            row![label("alert"), pick_list(AlertChoice::ALL, Some(self.alert), Msg::Alert).text_size(13).padding([5, 8]).style(pick_style)]
                .spacing(10)
                .align_y(Alignment::Center),
            row![
                label("repeat"),
                pick_list(RepeatChoice::options(&self.repeat), Some(self.repeat.clone()), Msg::Repeat)
                    .text_size(13)
                    .padding([5, 8])
                    .style(pick_style)
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        ]
        .spacing(8);

        let notes_editor = text_editor(&self.notes).placeholder("Notes").on_action(Msg::Notes).height(Length::Fixed(92.0)).size(13).style(
            move |theme, status| {
                let mut s = text_editor::default(theme, status);
                s.background = p.bg.into();
                s.border = iced::Border { color: p.grid_strong, width: 1.0, radius: 8.0.into() };
                s.placeholder = p.faint;
                s.value = p.text;
                s
            },
        );

        // Images
        let mut thumbs = row![].spacing(8).align_y(Alignment::Center);
        for (i, url) in self.images.iter().enumerate() {
            let content: Element<'a, Msg> = match cache.thumb(url) {
                Some(h) => image(h.clone()).width(64).height(64).content_fit(iced::ContentFit::Cover).border_radius(6).into(),
                None => container(icons::icon(icons::image(), 20.0, p.muted))
                    .center(64)
                    .style(move |_| container::Style {
                        background: Some(p.surface_alt.into()),
                        border: iced::Border { radius: 6.0.into(), ..Default::default() },
                        ..Default::default()
                    })
                    .into(),
            };
            let remove = button(icons::icon(icons::close(), 10.0, iced::Color::WHITE))
                .padding(3)
                .style(move |_, _| button::Style {
                    background: Some(theme::with_alpha(iced::Color::BLACK, 0.6).into()),
                    border: iced::Border { radius: 999.0.into(), ..Default::default() },
                    ..Default::default()
                })
                .on_press(Msg::RemoveImage(i));
            thumbs = thumbs.push(iced::widget::stack![
                mouse_area(content).on_press(Msg::ViewImage(i)).interaction(iced::mouse::Interaction::Pointer),
                container(remove).align_right(64).padding(2),
            ]);
        }
        for _ in 0..self.uploading {
            thumbs = thumbs.push(container(text("…").size(18).color(p.muted)).center(64).style(move |_| container::Style {
                background: Some(p.surface_alt.into()),
                border: iced::Border { radius: 6.0.into(), color: p.today, width: 1.0 },
                ..Default::default()
            }));
        }
        let add_btn = button(
            row![
                icons::icon(icons::image(), 14.0, p.muted),
                text(if !uploads {
                    "Set up image uploads…"
                } else if drop_hover {
                    "Drop to attach"
                } else {
                    "Add image"
                })
                .size(12)
                .color(p.muted)
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .style(theme::ghost)
        .padding([6, 8])
        .on_press(Msg::PickImages);
        let images = column![
            scrollable(thumbs).direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::new().width(4).scroller_width(4))),
            add_btn
        ]
        .spacing(6);

        let mut footer = row![].spacing(8).align_y(Alignment::Center);
        if self.target.is_some() {
            footer = footer.push(
                button(row![icons::icon(icons::trash(), 14.0, p.danger), text("Delete").size(13)].spacing(6).align_y(Alignment::Center))
                    .style(theme::danger_ghost)
                    .padding([6, 10])
                    .on_press(Msg::Delete),
            );
        }
        footer = footer.push(Space::new().width(Length::Fill));
        if let Some(err) = &self.error {
            footer = footer.push(text(err.clone()).size(12).color(p.danger));
        }
        footer = footer.push(button(text("Cancel").size(13)).style(theme::ghost).padding([6, 12]).on_press(Msg::Cancel)).push(
            button(text(if self.target.is_some() { "Save" } else { "Add Event" }).size(13))
                .style(theme::primary)
                .padding([6, 14])
                .on_press(Msg::Save),
        );

        let body = column![
            title,
            location_row,
            url_row,
            container(Space::new().height(1).width(Length::Fill)).style(theme::divider),
            fields,
            notes_editor,
            images,
            footer
        ]
        .spacing(12);

        container(scrollable(body).style(theme::main_scroll))
            .padding(18)
            .width(Length::Fixed(480.0))
            .max_height(680)
            .style(theme::popover)
            .into()
    }
}
