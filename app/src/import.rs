//! Importing `.ics` files (invites, exports): opened from the file manager or
//! a mail client, dropped on the window, or picked with `> Import .ics file…`.
//! The dialog lists what's in the files and which calendar they go to.

use std::path::PathBuf;

use calen_core::CalendarObject;
use iced::widget::{Space, button, column, container, pick_list, row, scrollable, text};
use iced::{Alignment, Element, Font, Length, Task, font};

use crate::app::{App, Message};
use crate::theme::{self, Palette};
use crate::views::editor::CalChoice;

const BOLD: Font = Font { weight: font::Weight::Semibold, ..Font::DEFAULT };
/// Calendar files are small; anything bigger is not one.
const MAX_FILE: u64 = 10 * 1024 * 1024;

pub struct ImportDialog {
    pub objects: Vec<CalendarObject>,
    pub calendar: CalChoice,
}

#[derive(Debug, Clone)]
pub enum ImportMsg {
    Open(Vec<PathBuf>),
    Calendar(CalChoice),
    Confirm,
    Close,
}

pub fn is_calendar_file(path: &std::path::Path) -> bool {
    crate::platform::instance::is_calendar_file(&path.to_string_lossy())
}

/// Ask for `.ics` files with the file chooser.
pub fn pick() -> Task<Message> {
    Task::perform(
        async {
            rfd::AsyncFileDialog::new()
                .set_title("Import calendar file")
                .add_filter("Calendar files", &["ics", "ical", "icalendar"])
                .pick_files()
                .await
                .map(|files| files.into_iter().map(|f| f.path().to_path_buf()).collect::<Vec<_>>())
                .unwrap_or_default()
        },
        |files| Message::Import(ImportMsg::Open(files)),
    )
}

fn read(path: &std::path::Path) -> Result<Vec<CalendarObject>, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
    let len = std::fs::metadata(path).map_err(|e| format!("Couldn't open {name}: {e}"))?.len();
    if len > MAX_FILE {
        return Err(format!("{name} is too large to be a calendar file"));
    }
    let text = std::fs::read(path).map_err(|e| format!("Couldn't open {name}: {e}"))?;
    let objects = CalendarObject::split_import(&String::from_utf8_lossy(&text))
        .map_err(|e| format!("{name} isn't a calendar file ({e})"))?;
    if objects.is_empty() {
        return Err(format!("{name} has no events"));
    }
    Ok(objects)
}

pub fn update(app: &mut App, msg: ImportMsg) -> Task<Message> {
    match msg {
        ImportMsg::Open(paths) => {
            if paths.is_empty() {
                return Task::none();
            }
            let mut objects: Vec<CalendarObject> = Vec::new();
            for p in &paths {
                match read(p) {
                    Ok(objs) => {
                        for o in objs {
                            // The same event in two files: keep the later one.
                            objects.retain(|x| x.uid() != o.uid());
                            objects.push(o);
                        }
                    }
                    Err(e) => {
                        log::warn!("import {}: {e}", p.display());
                        app.flash(e);
                    }
                }
            }
            log::info!("import: {} file(s), {} event(s)", paths.len(), objects.len());
            let show = app.open_main();
            if objects.is_empty() {
                return show;
            }
            let calendar = existing_calendar(app, &objects).unwrap_or_else(|| app.default_calendar());
            app.editor = None;
            app.quick = None;
            crate::palette::close(app);
            app.import = Some(ImportDialog { objects, calendar });
            return show;
        }
        ImportMsg::Calendar(c) => {
            if let Some(d) = &mut app.import {
                d.calendar = c;
            }
        }
        ImportMsg::Close => app.import = None,
        ImportMsg::Confirm => {
            let Some(d) = app.import.take() else { return Task::none() };
            let cal = d.calendar.id.clone();
            let mut snaps = Vec::with_capacity(d.objects.len());
            let mut saved = 0;
            for obj in &d.objects {
                snaps.push(app.snapshot(&cal, obj.uid()));
                match app.store.save_local(&cal, obj.uid(), &obj.original_ics()) {
                    Ok(()) => saved += 1,
                    Err(e) => {
                        log::error!("import {}: {e}", obj.uid());
                        snaps.pop();
                    }
                }
            }
            if saved == 0 {
                app.flash("Couldn't import the events");
                return Task::none();
            }
            let label = if saved == 1 { "Import event".to_string() } else { format!("Import {saved} events") };
            app.push_undo(label, snaps);
            app.after_local_change();
            let first = d.objects.iter().filter_map(|o| o.master()).map(|e| e.start.local()).min();
            if let Some(start) = first {
                app.jump_to(start.date_naive());
                if let Some(o) = d.objects.first() {
                    app.select_uid(&cal, o.uid());
                }
            }
            let what = if saved == 1 { "1 event".to_string() } else { format!("{saved} events") };
            app.flash(format!("Imported {what} into {} · Ctrl+Z to undo", d.calendar.name));
            return first.map(crate::app::scroll_to_time).unwrap_or_else(Task::none);
        }
    }
    Task::none()
}

/// An invite that's already in Calen (an update to it) goes back to the
/// calendar that has it.
fn existing_calendar(app: &App, objects: &[CalendarObject]) -> Option<CalChoice> {
    let choices = app.calendar_choices();
    objects.iter().find_map(|o| {
        app.data.objects.keys().find(|(_, uid)| uid == o.uid()).and_then(|(cal, _)| choices.iter().find(|c| &c.id == cal).cloned())
    })
}

fn describe(app: &App, obj: &CalendarObject) -> (String, String) {
    let Some(ev) = obj.master() else { return ("(No title)".into(), String::new()) };
    let title = if ev.summary.is_empty() { "(No title)".to_string() } else { ev.summary.clone() };
    let start = ev.start.local();
    let mut detail = start.format("%a %-d %b %Y").to_string();
    if !ev.all_day() {
        detail.push_str(&format!(" · {}", crate::views::grid::fmt_time(start, app.settings.time_24h)));
    }
    if ev.rrule.is_some() {
        detail.push_str(" · repeats");
    }
    if !ev.location.is_empty() {
        detail.push_str(&format!(" · {}", calen_core::notes::display_location(ev)));
    }
    (title, detail)
}

pub fn view<'a>(app: &'a App, p: Palette) -> Option<Element<'a, Message>> {
    let d = app.import.as_ref()?;
    let n = d.objects.len();
    let updates = d.objects.iter().filter(|o| app.data.object(&d.calendar.id, o.uid()).is_some()).count();

    let mut list = column![].spacing(8);
    for obj in &d.objects {
        let (title, detail) = describe(app, obj);
        list = list.push(
            row![
                container(Space::new().width(8).height(8)).style(theme::dot(d.calendar.color)),
                column![text(title).size(13).color(p.text), text(detail).size(12).color(p.muted)].spacing(2),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
    }

    let pick_style = move |theme: &iced::Theme, status| {
        let mut s = pick_list::default(theme, status);
        s.background = p.bg.into();
        s.border = iced::Border { color: p.grid_strong, width: 1.0, radius: 8.0.into() };
        s.text_color = p.text;
        s
    };
    let choices = app.calendar_choices();
    let selected = choices.iter().find(|c| c.id == d.calendar.id).cloned();
    let note = match updates {
        0 => String::new(),
        u if u == n => "Already in this calendar: it will be updated.".into(),
        u => format!("{u} already in this calendar will be updated."),
    };

    let title = if n == 1 { "Import event".to_string() } else { format!("Import {n} events") };
    Some(
        container(
            column![
                text(title).size(17).font(BOLD),
                scrollable(list).height(Length::Shrink).style(theme::main_scroll),
                row![
                    text("Calendar").size(12).color(p.muted),
                    pick_list(choices, selected, |c| Message::Import(ImportMsg::Calendar(c))).text_size(13).padding([5, 8]).style(pick_style),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
                row![
                    text(note).size(12).color(p.muted).width(Length::Fill),
                    button(text("Cancel").size(13)).style(theme::ghost).padding([6, 12]).on_press(Message::Import(ImportMsg::Close)),
                    button(text("Import").size(13)).style(theme::primary).padding([6, 14]).on_press(Message::Import(ImportMsg::Confirm)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(14),
        )
        .padding(20)
        .width(Length::Fixed(440.0))
        .max_height(520)
        .style(theme::popover)
        .into(),
    )
}
