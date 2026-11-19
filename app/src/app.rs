//! Application state and update loop.

use calen_core::quickadd::{self, QuickAdd};
use calen_core::{CalendarObject, Event, When};
use calen_store::{LOCAL_CALENDAR, Store};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate};
use iced::keyboard::{self, Key, key::Named};
use iced::widget::{operation, scrollable::AbsoluteOffset, text_editor};
use iced::{Subscription, Task, window};
use std::path::PathBuf;

use crate::edit::{self, Change, Outcome, Scope, Target};
use crate::images::ImageCache;
use crate::model::{Data, Occ, OccKey};
use crate::settings::{Settings, ThemeChoice, ViewKind};
use crate::theme::{self, Palette};
use crate::views::editor::{self, CalChoice, Editor};
use crate::views::grid::GridMsg;
use crate::views::sidebar::Pane;
use crate::views::week::HOUR_H;

pub const WEEK_SCROLL: &str = "week-scroll";
pub const QUICK_INPUT: &str = "quick-add";

#[derive(Debug, Clone)]
pub enum Message {
    // navigation
    Prev,
    Next,
    Today,
    SetView(ViewKind),
    SelectDate(NaiveDate),
    MiniShift(i32),
    ToggleSidebar,
    ToggleCalendarList,
    ToggleCalendar(String, bool),
    // grids
    Grid(GridMsg),
    AgendaClick(OccKey, NaiveDate),
    // quick add
    OpenQuickAdd(Option<NaiveDate>),
    QuickInput(String),
    QuickSubmit,
    QuickExpand,
    // editor
    Editor(editor::Msg),
    // recurring scope prompt
    ScopeChosen(Option<Scope>),
    // misc
    OpenUrl(String),
    Key(keyboard::Key, keyboard::Modifiers),
    Tick,
    DeleteSelected,
    OpenSelected,
    CloseOverlay,
    Palette(crate::palette::Msg),
    Sync(crate::sync_hook::SyncMsg),
    Platform(crate::platform::PlatformEvent),
    Update(crate::updater::UpdateMsg),
    Images(crate::image_tasks::ImageMsg),
    Viewer(crate::viewer::ViewerMsg),
    FlyoutNew,
    /// List the week of that day in the flyout (None = the upcoming list).
    FlyoutWeek(Option<NaiveDate>),
    /// A measured part of a week list (left pane or flyout).
    PaneMeasured(crate::views::sidebar::Pane, crate::views::sidebar::Part, f32),
    FlyoutReady(window::Id, u64),
    FlyoutFocus(window::Id, u32),
    Popup(crate::alert_popup::PopupMsg),
    Storage(crate::storage::StorageMsg),
    ScrollMorning,
    CloseShortcuts,
    // windows
    WindowOpened(window::Id),
    CloseRequested(window::Id),
    WindowEvent(window::Id, window::Event),
    ShowMain,
    // custom title bar
    WinDrag,
    WinMinimize,
    WinToggleMaximize,
    WinResize(window::Direction),
    MaximizedChanged(bool),
    TrimMemory,
    /// Dev aid (`--demo palette|editor`): open that screen.
    Demo(String),
    SystemTheme(iced::theme::Mode),
}

pub struct QuickAddState {
    pub input: String,
    pub preview: QuickAdd,
    pub date_hint: Option<NaiveDate>,
}

pub struct PendingChange {
    pub target: Target,
    pub change: Change,
    /// For editor saves that also move calendars.
    pub to_cal: Option<String>,
    pub title: String,
}

struct UndoEntry {
    label: String,
    /// (cal_id, uid, previous ICS or None if it didn't exist)
    snapshots: Vec<(String, String, Option<String>)>,
}

pub struct App {
    pub store: Store,
    pub data: Data,
    pub settings: Settings,
    pub main_window: Option<window::Id>,
    pub main_visible: bool,
    pub main_maximized: bool,
    /// Last normal size/position of the main window, reused when it's recreated
    /// (it's destroyed while Calen sits in the tray to free memory).
    main_geometry: (Option<iced::Size>, Option<iced::Point>),
    reopen_maximized: bool,
    pub view: ViewKind,
    pub anchor: NaiveDate,
    pub mini_month: NaiveDate,
    pub selected: Option<OccKey>,
    pub now: DateTime<Local>,
    /// Occurrences over a window around `anchor` (recomputed on change).
    pub occs: Vec<Occ>,
    occs_range: (DateTime<Local>, DateTime<Local>),
    pub quick: Option<QuickAddState>,
    pub editor: Option<Editor>,
    pub scope_prompt: Option<PendingChange>,
    undo: Vec<UndoEntry>,
    pub toast: Option<(String, std::time::Instant)>,
    pub show_calendars: bool,
    pub images: ImageCache,
    pub drop_hover: bool,
    pub system_dark: bool,
    pub data_dir: PathBuf,
    pub sync: crate::sync_hook::SyncState,
    pub palette_state: Option<crate::palette::PaletteState>,
    pub show_shortcuts: bool,
    pub flyout: crate::flyout::FlyoutState,
    pub popups: crate::alert_popup::PopupState,
    pub notify: crate::notify::NotifyState,
    pub updater: crate::updater::UpdaterState,
    pub viewer: Option<crate::viewer::Viewer>,
    pub storage: Option<crate::storage::StorageDialog>,
    /// Drawn grid layers, kept across repaints that don't change them.
    pub grid_caches: crate::views::grid::GridCaches,
    /// The left pane's week list.
    pub side_pane: crate::views::sidebar::WeekPane,
}

pub struct Flags {
    pub start_minimized: bool,
    pub data_dir: PathBuf,
}

impl App {
    pub fn boot(flags: Flags) -> (Self, Task<Message>) {
        let db_path = flags.data_dir.join("calen.db");
        let store = Store::open(&db_path).unwrap_or_else(|e| {
            log::error!("cannot open {}: {e}; using in-memory store", db_path.display());
            Store::open_in_memory().expect("in-memory sqlite")
        });
        if std::env::args().any(|a| a == "--seed") && store.objects().map(|o| o.is_empty()).unwrap_or(false) {
            crate::seed::seed(&store, &flags.data_dir.join("thumbs"));
        }
        let settings: Settings = store.setting(Settings::KEY).ok().flatten().unwrap_or_default();
        let data = Data::load(&store);
        let today = Local::now().date_naive();
        let mut app = App {
            store,
            data,
            view: settings.view,
            settings,
            main_window: None,
            main_visible: false,
            main_maximized: false,
            main_geometry: (None, None),
            reopen_maximized: false,
            anchor: today,
            mini_month: first_of_month(today),
            selected: None,
            now: Local::now(),
            occs: Vec::new(),
            occs_range: (Local::now(), Local::now()),
            quick: None,
            editor: None,
            scope_prompt: None,
            undo: Vec::new(),
            toast: None,
            show_calendars: false,
            images: ImageCache::default(),
            drop_hover: false,
            system_dark: true,
            data_dir: flags.data_dir,
            sync: Default::default(),
            palette_state: None,
            show_shortcuts: false,
            flyout: Default::default(),
            popups: Default::default(),
            notify: Default::default(),
            updater: Default::default(),
            viewer: None,
            storage: None,
            grid_caches: Default::default(),
            side_pane: Default::default(),
        };
        app.refresh_occurrences(true);
        let task = if flags.start_minimized { Task::none() } else { app.open_main() };
        let sync = crate::sync_hook::boot(&mut app);
        crate::platform::init_tray(app.settings.dnd_active(chrono::Utc::now().timestamp()));
        if cfg!(debug_assertions) {
            // Dev builds never register themselves to start at login.
        } else if !app.settings.first_run_done {
            app.settings.first_run_done = true;
            if app.settings.start_at_login
                && let Err(e) = crate::platform::set_autostart(true)
            {
                log::warn!("autostart: {e}");
            }
            app.save_settings();
        } else if app.settings.start_at_login && !crate::platform::autostart_enabled() {
            // Keep the entry pointing at the current executable (e.g. after an update moved it).
            let _ = crate::platform::set_autostart(true);
        }
        let upd = crate::updater::boot(&mut app);
        // Dev aids, in debug builds or with CALEN_DEV set:
        // `--flyout` opens the tray flyout 4 s after start, as if the tray icon was
        // clicked (the delay lets you move focus elsewhere first, like a real tray click).
        let dev = cfg!(debug_assertions) || std::env::var_os("CALEN_DEV").is_some();
        let flyout = if dev && std::env::args().any(|a| a == "--flyout") {
            Task::perform(tokio::time::sleep(std::time::Duration::from_secs(4)), |_| {
                Message::Platform(crate::platform::PlatformEvent::TrayClick { x: 100_000.0, y: 100_000.0, rect: None })
            })
        } else {
            Task::none()
        };
        // `--demo palette|editor|pick:+N|flyout-pick:+N` sets that up after start
        // (README screenshots and checks).
        let demo = match std::env::args().skip_while(|a| a != "--demo").nth(1) {
            Some(what) if dev => {
                // After the flyout start aid (4 s) when both are given.
                let secs = if std::env::args().any(|a| a == "--flyout") { 5 } else { 2 };
                Task::perform(tokio::time::sleep(std::time::Duration::from_secs(secs)), move |_| Message::Demo(what.clone()))
            }
            _ => Task::none(),
        };
        (app, Task::batch([task, sync, upd, flyout, demo]))
    }

    pub fn palette(&self) -> Palette {
        let dark = match self.settings.theme {
            ThemeChoice::Dark => true,
            ThemeChoice::Light => false,
            ThemeChoice::System => self.system_dark,
        };
        if dark { theme::DARK } else { theme::LIGHT }
    }

    pub fn theme(&self, _window: window::Id) -> iced::Theme {
        theme::iced_theme(&self.palette())
    }

    pub fn title(&self, _window: window::Id) -> String {
        "Calen".into()
    }

    // ------------------------------------------------------------------
    // windows

    pub fn open_main(&mut self) -> Task<Message> {
        crate::platform::left_background();
        if let Some(id) = self.main_window {
            self.main_visible = true;
            return Task::batch([
                window::set_mode(id, window::Mode::Windowed),
                window::minimize(id, false),
                window::gain_focus(id),
                self.scroll_side(crate::views::sidebar::SETTLE),
            ]);
        }
        let (size, pos) = self.main_geometry;
        let (id, open) = window::open(window::Settings {
            size: size.unwrap_or(iced::Size::new(1240.0, 800.0)),
            position: pos.map(window::Position::Specific).unwrap_or_default(),
            maximized: self.reopen_maximized,
            min_size: Some(iced::Size::new(720.0, 480.0)),
            // Calen draws its own title bar (toolbar + caption buttons).
            decorations: false,
            exit_on_close_request: false,
            icon: crate::platform::window_icon(),
            platform_specific: crate::platform::main_window_platform(),
            ..Default::default()
        });
        self.main_window = Some(id);
        self.main_visible = true;
        open.map(Message::WindowOpened)
    }

    /// Send the main window to the tray. The window is destroyed rather than
    /// hidden: its GPU surface and caches are the bulk of Calen's memory, and all
    /// state lives in `App`, so `open_main` rebuilds it as it was.
    fn hide_main(&mut self) -> Task<Message> {
        self.main_visible = false;
        let Some(id) = self.main_window.take() else { return Task::none() };
        self.reopen_maximized = self.main_maximized;
        self.main_maximized = false;
        self.drop_hover = false;
        self.grid_caches = Default::default();
        if self.editor.is_none() && self.viewer.is_none() {
            self.images.trim();
        }
        crate::platform::entered_background();
        Task::batch([window::close(id), trim_memory_later()])
    }

    // ------------------------------------------------------------------
    // data helpers

    /// Range of days the current view shows.
    pub fn visible_days(&self) -> Vec<NaiveDate> {
        match self.view {
            ViewKind::Day => vec![self.anchor],
            ViewKind::Week => {
                let start = start_of_week(self.anchor, self.settings.week_start_day());
                (0..7).map(|i| start + Duration::days(i)).collect()
            }
            ViewKind::Month => {
                let first = start_of_week(first_of_month(self.anchor), self.settings.week_start_day());
                (0..42).map(|i| first + Duration::days(i)).collect()
            }
        }
    }

    pub fn refresh_occurrences(&mut self, force: bool) {
        let a = first_of_month(self.anchor.min(self.mini_month));
        let from = calen_core::event::local_midnight(a - Duration::days(14));
        let to = calen_core::event::local_midnight(first_of_month(self.anchor.max(self.mini_month)) + Duration::days(31 + 45));
        if !force && from >= self.occs_range.0 && to <= self.occs_range.1 {
            return;
        }
        self.occs = self.data.occurrences(from, to);
        self.occs_range = (from, to);
    }

    pub fn reload(&mut self) {
        self.data = Data::load(&self.store);
        self.refresh_occurrences(true);
        if let Some(sel) = &self.selected
            && !self.occs.iter().any(|o| &o.key == sel)
        {
            self.selected = None;
        }
    }

    pub fn occ(&self, key: &OccKey) -> Option<&Occ> {
        self.occs.iter().find(|o| &o.key == key)
    }

    pub fn calendar_choices(&self) -> Vec<CalChoice> {
        self.data
            .writable_calendars()
            .into_iter()
            .map(|c| CalChoice { id: c.id.clone(), name: c.name.clone(), color: self.data.calendar_color(&c.id) })
            .collect()
    }

    fn default_calendar(&self) -> CalChoice {
        let choices = self.calendar_choices();
        self.settings
            .default_calendar
            .as_ref()
            .and_then(|id| choices.iter().find(|c| &c.id == id).cloned())
            .or_else(|| choices.iter().find(|c| c.id != LOCAL_CALENDAR).cloned())
            .or_else(|| choices.first().cloned())
            .unwrap_or(CalChoice {
                id: LOCAL_CALENDAR.into(),
                name: "On this computer".into(),
                color: iced::Color::from_rgb8(0x4f, 0x8c, 0xff),
            })
    }

    fn calendar_by_hint(&self, hint: &str) -> Option<CalChoice> {
        let h = hint.to_lowercase();
        let choices = self.calendar_choices();
        choices
            .iter()
            .find(|c| c.name.to_lowercase() == h)
            .or_else(|| choices.iter().find(|c| c.name.to_lowercase().starts_with(&h)))
            .or_else(|| choices.iter().find(|c| c.name.to_lowercase().contains(&h)))
            .cloned()
    }

    pub fn save_settings(&self) {
        if let Err(e) = self.store.set_setting(Settings::KEY, &self.settings) {
            log::error!("saving settings: {e}");
        }
    }

    fn target_for(&self, occ: &Occ) -> Target {
        Target {
            key: occ.key.clone(),
            event_index: occ.event_index,
            recurrence_id: occ.recurrence_id.clone(),
            occ_start: occ.start,
            occ_end: occ.end,
        }
    }

    fn snapshot(&self, cal_id: &str, uid: &str) -> (String, String, Option<String>) {
        let prev = self.store.object(cal_id, uid).ok().flatten().map(|r| r.ics);
        (cal_id.to_string(), uid.to_string(), prev)
    }

    fn push_undo(&mut self, label: impl Into<String>, snapshots: Vec<(String, String, Option<String>)>) {
        self.undo.push(UndoEntry { label: label.into(), snapshots });
        if self.undo.len() > 50 {
            self.undo.remove(0);
        }
    }

    pub fn flash(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), std::time::Instant::now()));
    }

    /// Persist a new object. Returns false on failure.
    fn create_object(&mut self, cal_id: &str, obj: CalendarObject) -> bool {
        let uid = obj.uid().to_string();
        let snap = self.snapshot(cal_id, &uid);
        match self.store.save_local(cal_id, &uid, &obj.to_ics()) {
            Ok(()) => {
                self.push_undo("Add event", vec![snap]);
                self.after_local_change();
                true
            }
            Err(e) => {
                self.flash(format!("Couldn't save: {e}"));
                false
            }
        }
    }

    /// Run (or ask scope for) a change to an existing occurrence.
    fn request_change(&mut self, target: Target, change: Change, to_cal: Option<String>) -> Task<Message> {
        let title = self.data.event(&target.key, target.event_index).map(|e| e.summary.clone()).unwrap_or_default();
        if target.is_recurring() {
            // Only fields that don't touch the schedule? Still ask: the user may mean one instance.
            self.scope_prompt = Some(PendingChange { target, change, to_cal, title });
            return Task::none();
        }
        self.apply_change(target, change, Scope::All, to_cal);
        Task::none()
    }

    fn apply_change(&mut self, target: Target, change: Change, scope: Scope, to_cal: Option<String>) {
        let cal_id = target.key.cal_id.clone();
        let uid = target.key.uid.clone();
        let Some(obj) = self.data.object(&cal_id, &uid).cloned() else {
            self.flash("That event no longer exists");
            return;
        };
        let mut snaps = vec![self.snapshot(&cal_id, &uid)];
        let label = match &change {
            Change::Delete => "Delete event",
            Change::Times { .. } => "Move event",
            Change::Fields(_) => "Edit event",
        };
        let result = match edit::apply(obj, &target, &change, scope) {
            Outcome::DeleteObject => self.store.delete_local(&cal_id, &uid),
            Outcome::Save(obj) => match to_cal.filter(|c| *c != cal_id) {
                Some(dest) => {
                    snaps.push(self.snapshot(&dest, &uid));
                    self.store.move_local(&cal_id, &dest, &uid, &obj.to_ics())
                }
                None => self.store.save_local(&cal_id, &uid, &obj.to_ics()),
            },
        };
        match result {
            Ok(()) => {
                self.push_undo(label, snaps);
                if matches!(change, Change::Delete) {
                    self.selected = None;
                }
                self.after_local_change();
            }
            Err(e) => self.flash(format!("Couldn't save: {e}")),
        }
    }

    fn after_local_change(&mut self) {
        self.reload();
        crate::sync_hook::local_changed(self);
    }

    fn undo(&mut self) {
        let Some(entry) = self.undo.pop() else {
            self.flash("Nothing to undo");
            return;
        };
        for (cal, uid, prev) in entry.snapshots.iter().rev() {
            let r = match prev {
                Some(ics) => self.store.save_local(cal, uid, ics),
                None => self.store.delete_local(cal, uid),
            };
            if let Err(e) = r {
                log::error!("undo: {e}");
            }
        }
        self.flash(format!("Undid: {}", entry.label));
        self.after_local_change();
    }

    // ------------------------------------------------------------------
    // quick add / editor

    fn open_quick_add(&mut self, date: Option<NaiveDate>) -> Task<Message> {
        self.editor = None;
        self.quick = Some(QuickAddState { input: String::new(), preview: QuickAdd::default(), date_hint: date });
        Task::batch([self.open_main(), operation::focus(QUICK_INPUT)])
    }

    fn quick_today(&self) -> NaiveDate {
        Local::now().date_naive()
    }

    fn quick_event(&self, q: &QuickAddState) -> (Event, CalChoice) {
        let mut p = q.preview.clone();
        if p.date.is_none() {
            p.date = q.date_hint;
        }
        let (start, end) = p.resolve(self.quick_today(), self.settings.default_duration);
        let title = if p.title.is_empty() { "New event".to_string() } else { p.title.clone() };
        let mut ev = Event::new(&title, start, end);
        if let Some(loc) = &p.location {
            ev.location = loc.clone();
        }
        ev.url = p.url.clone();
        if let Some(m) = p.url.as_deref().and_then(calen_core::links::find_maps) {
            ev.geo = m.geo;
        }
        ev.rrule = p.rrule.clone();
        let alert = p.alert_minutes.or(if ev.all_day() { None } else { self.settings.default_alert });
        ev.alarms = alert.into_iter().collect();
        let cal = p.calendar.as_deref().and_then(|h| self.calendar_by_hint(h)).unwrap_or_else(|| self.default_calendar());
        (ev, cal)
    }

    fn open_editor_for(&mut self, key: &OccKey) -> Task<Message> {
        let Some(occ) = self.occ(key).cloned() else { return Task::none() };
        let Some(ev) = self.data.event(key, occ.event_index).cloned() else { return Task::none() };
        let cal = self.calendar_choices().into_iter().find(|c| c.id == key.cal_id).unwrap_or(CalChoice {
            id: key.cal_id.clone(),
            name: occ.calendar_name.clone(),
            color: occ.color,
        });
        let editor = Editor::for_event(&ev, self.target_for(&occ), cal, self.settings.time_24h, self.settings.date_order);
        let wanted = self.images.wanted(editor.images.iter());
        self.editor = Some(editor);
        self.quick = None;
        self.selected = Some(key.clone());
        Task::batch([crate::image_tasks::load_thumbs(self, wanted), operation::focus("editor-title")])
    }

    fn open_editor_new(&mut self, start: When, end: When) -> Task<Message> {
        let alert = if start.is_date() { None } else { self.settings.default_alert };
        self.editor =
            Some(Editor::new_event("", start, end, self.default_calendar(), alert, self.settings.time_24h, self.settings.date_order));
        self.quick = None;
        operation::focus("editor-title")
    }

    fn save_editor(&mut self) -> Task<Message> {
        let Some(ed) = &mut self.editor else { return Task::none() };
        if ed.uploading > 0 {
            ed.error = Some("Wait for images to finish uploading".into());
            return Task::none();
        }
        let fields = match ed.fields() {
            Ok(f) => f,
            Err(e) => {
                ed.error = Some(e);
                return Task::none();
            }
        };
        let ed = self.editor.take().unwrap();
        match ed.target {
            None => {
                let mut ev = Event::new(&fields.title, fields.start.clone(), fields.end.clone());
                if ev.summary.is_empty() {
                    ev.summary = "New event".into();
                }
                ev.location = fields.location.clone();
                ev.url = fields.url.clone();
                ev.description = fields.description.clone();
                calen_core::notes::set_images(&mut ev, &fields.images);
                ev.alarms = fields.alarms.clone();
                ev.rrule = fields.rrule.clone().flatten();
                ev.geo = fields.geo;
                let mut obj = CalendarObject::new(ev);
                // Route through edit::apply-like location sync.
                if let Some(e) = obj.events.first_mut()
                    && let (Some((lat, lng)), false) = (e.geo, e.location.is_empty())
                {
                    e.component.props.push(
                        calen_core::ics::Property::new("X-APPLE-STRUCTURED-LOCATION", format!("geo:{lat:.6},{lng:.6}"))
                            .with_param("VALUE", "URI")
                            .with_param("X-APPLE-RADIUS", "70")
                            .with_param("X-TITLE", &e.location.replace('"', "'")),
                    );
                }
                let uid = obj.uid().to_string();
                let start = obj.events[0].start.local();
                if self.create_object(&ed.calendar.id, obj) {
                    self.jump_to(start.date_naive());
                    self.select_uid(&ed.calendar.id, &uid);
                }
                Task::none()
            }
            Some(target) => {
                let to_cal = (Some(&ed.calendar.id) != ed.original_cal.as_ref()).then(|| ed.calendar.id.clone());
                self.request_change(target, Change::Fields(Box::new(fields)), to_cal)
            }
        }
    }

    fn select_uid(&mut self, cal_id: &str, uid: &str) {
        self.selected = self.occs.iter().find(|o| o.key.cal_id == cal_id && o.key.uid == uid).map(|o| o.key.clone());
    }

    pub fn jump_to(&mut self, date: NaiveDate) {
        self.anchor = date;
        self.mini_month = first_of_month(date);
        self.refresh_occurrences(false);
    }

    /// Scroll the left pane so the picked day is its first line.
    fn scroll_side(&mut self, settle: std::time::Duration) -> Task<Message> {
        let week = start_of_week(self.anchor, self.settings.week_start_day());
        self.side_pane.scroll_to(Pane::Main, Some(week), self.anchor, settle)
    }

    fn scroll_to_morning(&self) -> Task<Message> {
        let hour = if self.visible_days().contains(&self.now.date_naive()) {
            (self.now.hour_f32() - 2.0).clamp(0.0, 16.0).min(self.settings.day_start_hour as f32).max(0.0)
        } else {
            self.settings.day_start_hour as f32
        };
        operation::scroll_to(WEEK_SCROLL, AbsoluteOffset { x: 0.0, y: hour * HOUR_H })
    }

    // ------------------------------------------------------------------
    // update

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Prev | Message::Next => {
                let dir = if matches!(message, Message::Next) { 1 } else { -1 };
                self.anchor = match self.view {
                    ViewKind::Day => self.anchor + Duration::days(dir),
                    ViewKind::Week => self.anchor + Duration::days(7 * dir),
                    ViewKind::Month => add_months(self.anchor, dir as i32),
                };
                self.mini_month = first_of_month(self.anchor);
                self.refresh_occurrences(false);
                return self.scroll_side(crate::views::sidebar::SETTLE);
            }
            Message::Today => {
                self.jump_to(Local::now().date_naive());
                return Task::batch([self.scroll_to_morning(), self.scroll_side(crate::views::sidebar::SETTLE)]);
            }
            Message::SetView(v) => {
                self.view = v;
                self.settings.view = v;
                self.save_settings();
                return self.scroll_to_morning();
            }
            Message::SelectDate(d) => {
                self.jump_to(d);
                return self.scroll_side(crate::views::sidebar::SETTLE);
            }
            Message::MiniShift(n) => {
                self.mini_month = add_months(self.mini_month, n);
                self.refresh_occurrences(false);
            }
            Message::ToggleSidebar => {
                self.settings.sidebar_visible = !self.settings.sidebar_visible;
                self.save_settings();
            }
            Message::ToggleCalendarList => self.show_calendars = !self.show_calendars,
            Message::ToggleCalendar(id, visible) => {
                let _ = self.store.set_calendar_visible(&id, visible);
                self.reload();
            }
            Message::Grid(g) => return self.on_grid(g),
            Message::AgendaClick(key, date) => {
                self.jump_to(date);
                self.selected = Some(key);
                if crate::flyout::is_open(self) {
                    let close = crate::flyout::close(self);
                    return Task::batch([close, self.open_main()]);
                }
            }
            Message::OpenQuickAdd(d) => return self.open_quick_add(d),
            Message::QuickInput(s) => {
                let today = self.quick_today();
                let order = self.settings.date_order;
                if let Some(q) = &mut self.quick {
                    q.preview = quickadd::parse(&s, today, order);
                    q.input = s;
                }
            }
            Message::QuickSubmit => {
                let Some(q) = self.quick.take() else { return Task::none() };
                if q.input.trim().is_empty() {
                    return Task::none();
                }
                let (ev, cal) = self.quick_event(&q);
                let start = ev.start.local();
                let uid = ev.uid.clone();
                let title = ev.summary.clone();
                if self.create_object(&cal.id, CalendarObject::new(ev)) {
                    self.jump_to(start.date_naive());
                    self.select_uid(&cal.id, &uid);
                    self.flash(format!("Added “{title}” · {}", start.format("%a %-d %b")));
                }
            }
            Message::QuickExpand => {
                let Some(q) = self.quick.take() else { return Task::none() };
                let (ev, cal) = self.quick_event(&q);
                let mut ed = Editor::new_event(
                    if q.input.trim().is_empty() { "" } else { &ev.summary },
                    ev.start.clone(),
                    ev.end.clone(),
                    cal,
                    ev.alarms.first().copied(),
                    self.settings.time_24h,
                    self.settings.date_order,
                );
                ed.location = ev.location.clone();
                ed.url = ev.url.clone().unwrap_or_default();
                ed.geo = ev.geo;
                ed.repeat = editor::RepeatChoice::from_rule(ev.rrule.as_deref());
                self.editor = Some(ed);
                return operation::focus("editor-title");
            }
            Message::Editor(m) => return self.on_editor(m),
            Message::ScopeChosen(scope) => {
                if let (Some(p), Some(scope)) = (self.scope_prompt.take(), scope) {
                    self.apply_change(p.target, p.change, scope, p.to_cal);
                }
            }
            Message::OpenUrl(u) => open_url(&u),
            Message::Key(key, mods) => return self.on_key(key, mods),
            Message::Tick => {
                self.now = Local::now();
                if self.toast.as_ref().is_some_and(|(_, t)| t.elapsed().as_secs() > 4) {
                    self.toast = None;
                }
                // Day rolled over → keep views fresh.
                return Task::batch([crate::notify::tick(self), crate::flyout::idle(self)]);
            }
            Message::DeleteSelected => {
                if let Some(occ) = self.selected.as_ref().and_then(|k| self.occ(k)).cloned() {
                    if occ.read_only {
                        self.flash("This calendar is read-only");
                    } else {
                        let t = self.target_for(&occ);
                        return self.request_change(t, Change::Delete, None);
                    }
                }
            }
            Message::OpenSelected => {
                if let Some(k) = self.selected.clone() {
                    return self.open_editor_for(&k);
                }
            }
            Message::CloseOverlay => {
                // Close the top-most overlay; with none open, clear the selection.
                let closed = std::mem::take(&mut self.show_shortcuts)
                    || self.storage.take().is_some()
                    || self.sync.dialog.take().is_some()
                    || self.scope_prompt.take().is_some()
                    || self.editor.take().is_some()
                    || self.quick.take().is_some()
                    || crate::palette::close(self)
                    || crate::viewer::close(self);
                if !closed {
                    self.selected = None;
                }
            }
            Message::WindowOpened(id) => {
                if Some(id) == self.main_window {
                    // The scrollable only exists after the first layout; scroll a moment later.
                    return Task::perform(tokio::time::sleep(std::time::Duration::from_millis(120)), |_| Message::ScrollMorning);
                }
            }
            // The window was just created: its list is measured over the next frames.
            Message::ScrollMorning => {
                return Task::batch([self.scroll_to_morning(), self.scroll_side(std::time::Duration::from_secs(2))]);
            }
            Message::CloseRequested(id) => {
                if Some(id) == self.main_window {
                    if self.settings.close_to_tray && crate::platform::can_run_in_background() {
                        return self.hide_main();
                    }
                    return iced::exit();
                }
                if let Some(t) = crate::alert_popup::close_requested(self, id) {
                    return t;
                }
                return crate::flyout::close_requested(self, id);
            }
            Message::WindowEvent(id, ev) => return self.on_window_event(id, ev),
            Message::ShowMain => {
                let close = crate::flyout::close(self);
                return Task::batch([close, self.open_main()]);
            }
            Message::Palette(m) => return crate::palette::update(self, m),
            Message::Sync(m) => return crate::sync_hook::update(self, m),
            Message::Platform(e) => return self.on_platform(e),
            Message::Update(m) => return crate::updater::update(self, m),
            Message::Images(m) => return crate::image_tasks::update(self, m),
            Message::Viewer(m) => return crate::viewer::update(self, m),
            Message::Storage(m) => return crate::storage::update(self, m),
            Message::FlyoutNew => {
                let close = crate::flyout::close(self);
                return Task::batch([close, self.open_quick_add(None)]);
            }
            Message::FlyoutWeek(w) => return crate::flyout::show_week(self, w),
            Message::PaneMeasured(Pane::Flyout, part, h) => return crate::flyout::measured(self, part, h),
            Message::PaneMeasured(Pane::Main, part, h) => {
                let week = start_of_week(self.anchor, self.settings.week_start_day());
                return self.side_pane.measured(part, h, Pane::Main, Some(week), self.anchor);
            }
            Message::FlyoutReady(id, raw) => return crate::flyout::ready(self, id, raw),
            Message::FlyoutFocus(id, attempt) => return crate::flyout::focus(self, id, attempt),
            Message::Popup(m) => return crate::alert_popup::update(self, m),
            Message::WinDrag => {
                if let Some(id) = self.main_window {
                    return window::drag(id);
                }
            }
            Message::WinMinimize => {
                // Minimize to the tray / background, like the close button. (On
                // Wayland a minimized window can't be detected or restored.)
                if self.settings.minimize_to_tray && crate::platform::can_run_in_background() {
                    return self.hide_main();
                }
                if let Some(id) = self.main_window {
                    return window::minimize(id, true);
                }
            }
            Message::WinToggleMaximize => {
                if let Some(id) = self.main_window {
                    return window::toggle_maximize(id);
                }
            }
            Message::WinResize(dir) => {
                if let Some(id) = self.main_window {
                    return window::drag_resize(id, dir);
                }
            }
            Message::MaximizedChanged(m) => self.main_maximized = m,
            Message::Demo(what) => match what.as_str() {
                "palette" => return crate::palette::open(self, ">"),
                "editor" => {
                    if let Some(key) = self.occs.iter().find(|o| o.title.contains("photos")).map(|o| o.key.clone()) {
                        return self.open_editor_for(&key);
                    }
                }
                // `pick:+N` / `flyout-pick:+N`: pick the day N days from today, as a
                // click on that date would (main view, or the flyout's mini-month).
                w => {
                    let offset = |prefix: &str| w.strip_prefix(prefix).and_then(|n| n.parse::<i64>().ok());
                    let today = Local::now().date_naive();
                    if let Some(n) = offset("flyout-pick:") {
                        return crate::flyout::show_week(self, Some(today + Duration::days(n)));
                    }
                    if let Some(n) = offset("month-pick:") {
                        let view = self.update(Message::SetView(ViewKind::Month));
                        return Task::batch([view, self.on_grid(GridMsg::SelectDate(today + Duration::days(n)))]);
                    }
                    if let Some(n) = offset("pick:") {
                        return self.on_grid(GridMsg::SelectDate(today + Duration::days(n)));
                    }
                }
            },
            Message::TrimMemory => {
                // Only when nothing is on screen; otherwise pages come right back.
                if !self.main_visible && !crate::flyout::is_open(self) {
                    crate::platform::trim_memory();
                }
            }
            Message::CloseShortcuts => self.show_shortcuts = false,
            Message::SystemTheme(mode) => self.system_dark = !matches!(mode, iced::theme::Mode::Light),
        }
        Task::none()
    }

    fn on_platform(&mut self, e: crate::platform::PlatformEvent) -> Task<Message> {
        use crate::platform::{MenuAction, NotificationAction, PlatformEvent};
        match e {
            PlatformEvent::TrayClick { x, y, rect } => return crate::flyout::toggle(self, x, y, rect),
            PlatformEvent::SecondInstance => return self.update(Message::ShowMain),
            PlatformEvent::Menu(MenuAction::Open) => return self.update(Message::ShowMain),
            PlatformEvent::Menu(MenuAction::NewEvent) => {
                let close = crate::flyout::close(self);
                return Task::batch([close, self.open_quick_add(None)]);
            }
            PlatformEvent::Menu(MenuAction::ToggleDnd) => return crate::palette::run(self, crate::palette::Command::ToggleDnd),
            PlatformEvent::Menu(MenuAction::SyncNow) => return self.update(Message::Sync(crate::sync_hook::SyncMsg::Start)),
            PlatformEvent::Menu(MenuAction::Quit) => return iced::exit(),
            PlatformEvent::OutsideClick(raw) => return crate::flyout::outside_click(self, raw),
            PlatformEvent::NotifyFailed(n) => return crate::notify::failed(self, n),
            PlatformEvent::Notification(NotificationAction::Join(url)) => open_url(&url),
            PlatformEvent::Notification(NotificationAction::Snooze(key)) => crate::notify::snooze(self, &key),
            PlatformEvent::Notification(NotificationAction::Open(key)) => {
                let mut parts = key.split('|');
                if let (Some(cal), Some(uid)) = (parts.next(), parts.next())
                    && let Some(o) =
                        self.occs.iter().find(|o| o.key.cal_id == cal && o.key.uid == uid && o.end >= Local::now() - Duration::hours(2))
                {
                    let (k, d) = (o.key.clone(), o.start.date_naive());
                    self.jump_to(d);
                    self.selected = Some(k);
                }
                return self.update(Message::ShowMain);
            }
        }
        Task::none()
    }

    fn on_window_event(&mut self, id: window::Id, ev: window::Event) -> Task<Message> {
        match ev {
            window::Event::FileHovered(_) if Some(id) == self.main_window => self.drop_hover = true,
            window::Event::FilesHoveredLeft if Some(id) == self.main_window => self.drop_hover = false,
            window::Event::FileDropped(path) if Some(id) == self.main_window => {
                self.drop_hover = false;
                return crate::image_tasks::files_dropped(self, vec![path]);
            }
            window::Event::Unfocused => return crate::flyout::unfocused(self, id),
            window::Event::Resized(size) if Some(id) == self.main_window && size.width < 1.0 && size.height < 1.0 => {
                // Minimized (Windows reports a 0x0 resize) -> tuck into the tray.
                if self.settings.minimize_to_tray && crate::platform::can_run_in_background() && self.main_visible {
                    return self.hide_main();
                }
            }
            // Minimized windows report (-32000, -32000) on Windows; skip those.
            window::Event::Moved(p) if Some(id) == self.main_window && !self.main_maximized && p.x > -10_000.0 => {
                self.main_geometry.1 = Some(p);
            }
            window::Event::Resized(size) if Some(id) == self.main_window => {
                if !self.main_maximized {
                    self.main_geometry.0 = Some(size);
                }
                return window::is_maximized(id).map(Message::MaximizedChanged);
            }
            _ => {}
        }
        Task::none()
    }

    fn on_grid(&mut self, g: GridMsg) -> Task<Message> {
        match g {
            GridMsg::Select(k) => self.selected = k,
            GridMsg::Open(k) => return self.open_editor_for(&k),
            GridMsg::SelectDate(d) => {
                self.jump_to(d);
                return self.scroll_side(crate::views::sidebar::SETTLE);
            }
            GridMsg::NewOn(d) => return self.open_quick_add(Some(d)),
            GridMsg::Create { start, end, all_day } => {
                let (s, e) = if all_day {
                    (When::Date(start.date_naive()), When::Date(end.date_naive()))
                } else {
                    (When::at_local(start), When::at_local(end))
                };
                return self.open_editor_new(s, e);
            }
            GridMsg::Change { key, start, end } => {
                let Some(occ) = self.occ(&key).cloned() else { return Task::none() };
                self.selected = Some(key);
                let t = self.target_for(&occ);
                return self.request_change(t, Change::Times { start, end, all_day: occ.all_day }, None);
            }
        }
        Task::none()
    }

    fn on_editor(&mut self, m: editor::Msg) -> Task<Message> {
        match &m {
            editor::Msg::Save => return self.save_editor(),
            editor::Msg::Cancel => {
                self.editor = None;
                return Task::none();
            }
            editor::Msg::Delete => {
                let Some(ed) = self.editor.take() else { return Task::none() };
                if let Some(t) = ed.target {
                    return self.request_change(t, Change::Delete, None);
                }
                return Task::none();
            }
            editor::Msg::Open(u) => {
                open_url(u);
                return Task::none();
            }
            editor::Msg::PickImages => return crate::image_tasks::pick_images(self),
            editor::Msg::ViewImage(i) => {
                let urls = self.editor.as_ref().map(|e| e.images.clone()).unwrap_or_default();
                return crate::viewer::open(self, urls, *i);
            }
            _ => {}
        }
        if let Some(ed) = &mut self.editor {
            ed.error = None;
            ed.update(&m);
        }
        Task::none()
    }

    fn overlay_open(&self) -> bool {
        self.editor.is_some()
            || self.quick.is_some()
            || self.scope_prompt.is_some()
            || self.storage.is_some()
            || self.sync.dialog.is_some()
            || self.show_shortcuts
            || crate::palette::is_open(self)
            || crate::viewer::is_open(self)
    }

    fn on_key(&mut self, key: Key, mods: keyboard::Modifiers) -> Task<Message> {
        let ctrl = mods.command();
        // The flyout closes on any outside click, so while it's open it has the keyboard.
        if crate::flyout::is_open(self) && key == Key::Named(Named::Escape) {
            return crate::flyout::close(self);
        }
        if let Some(t) = crate::viewer::on_key(self, &key) {
            return t;
        }
        if let Some(t) = crate::palette::on_key(self, &key, mods) {
            return t;
        }
        match key.as_ref() {
            Key::Named(Named::Escape) => return self.update(Message::CloseOverlay),
            Key::Named(Named::Tab) if self.quick.is_some() => return self.update(Message::QuickExpand),
            Key::Named(Named::Tab) if self.overlay_open() => {
                return if mods.shift() { operation::focus_previous() } else { operation::focus_next() };
            }
            Key::Character("z") if ctrl => {
                if !self.overlay_open() {
                    self.undo();
                }
                return Task::none();
            }
            Key::Character("n") if ctrl => return self.open_quick_add(None),
            Key::Character("p") | Key::Character("f") | Key::Character("k") if ctrl => return crate::palette::open(self, ""),
            Key::Character(",") if ctrl => return crate::palette::open(self, ">"),
            Key::Character("s") if ctrl && self.editor.is_some() => return self.save_editor(),
            Key::Named(Named::Enter) if ctrl && self.editor.is_some() => return self.save_editor(),
            _ => {}
        }
        if self.overlay_open() || ctrl || mods.alt() {
            return Task::none();
        }
        match key.as_ref() {
            Key::Character("n") => return self.open_quick_add(None),
            Key::Character("t") => return self.update(Message::Today),
            Key::Character("d") => return self.update(Message::SetView(ViewKind::Day)),
            Key::Character("w") => return self.update(Message::SetView(ViewKind::Week)),
            Key::Character("m") => return self.update(Message::SetView(ViewKind::Month)),
            Key::Named(Named::ArrowLeft) => return self.update(Message::Prev),
            Key::Named(Named::ArrowRight) => return self.update(Message::Next),
            Key::Character("j") | Key::Named(Named::ArrowDown) => self.move_selection(1),
            Key::Character("k") | Key::Named(Named::ArrowUp) => self.move_selection(-1),
            Key::Named(Named::Enter) => return self.update(Message::OpenSelected),
            Key::Named(Named::Delete) | Key::Named(Named::Backspace) => return self.update(Message::DeleteSelected),
            Key::Character("/") => return crate::palette::open(self, ""),
            _ => {}
        }
        Task::none()
    }

    /// Move the selection through events of the visible range (J/K).
    fn move_selection(&mut self, dir: i32) {
        let days = self.visible_days();
        let (first, last) = (days[0], *days.last().unwrap());
        let list: Vec<&Occ> = self.occs.iter().filter(|o| o.start.date_naive() <= last && o.last_day() >= first).collect();
        if list.is_empty() {
            return;
        }
        let cur = self.selected.as_ref().and_then(|k| list.iter().position(|o| &o.key == k));
        let next = match cur {
            None if dir > 0 => 0,
            None => list.len() - 1,
            Some(i) => (i as i32 + dir).clamp(0, list.len() as i32 - 1) as usize,
        };
        self.selected = Some(list[next].key.clone());
    }

    // ------------------------------------------------------------------

    pub fn subscription(&self) -> Subscription<Message> {
        let keys = keyboard::listen().filter_map(|e| match e {
            keyboard::Event::KeyPressed { key, modifiers, .. } => Some(Message::Key(key, modifiers)),
            _ => None,
        });
        Subscription::batch([
            keys,
            window::close_requests().map(Message::CloseRequested),
            // Only the window events we act on. Not `window::events()`: that also
            // forwards RedrawRequested, so every frame became a message that
            // rebuilt every view and drew a second frame.
            iced::event::listen_with(|e, _, id| match e {
                iced::Event::Window(
                    w @ (window::Event::FileHovered(_)
                    | window::Event::FilesHoveredLeft
                    | window::Event::FileDropped(_)
                    | window::Event::Unfocused
                    | window::Event::Resized(_)
                    | window::Event::Moved(_)),
                ) => Some(Message::WindowEvent(id, w)),
                _ => None,
            }),
            iced::time::every(std::time::Duration::from_secs(20)).map(|_| Message::Tick),
            iced::system::theme_changes().map(Message::SystemTheme),
            crate::platform::subscription(),
            crate::sync_hook::subscription(self),
            crate::updater::subscription(self),
            crate::viewer::subscription(self),
        ])
    }
}

trait HourF {
    fn hour_f32(&self) -> f32;
}

impl HourF for DateTime<Local> {
    fn hour_f32(&self) -> f32 {
        use chrono::Timelike;
        self.hour() as f32 + self.minute() as f32 / 60.0
    }
}

/// Scroll the week/day grid so `t` is comfortably in view.
pub fn scroll_to_time(t: DateTime<Local>) -> Task<Message> {
    use chrono::Timelike;
    let h = (t.hour() as f32 - 2.0).max(0.0);
    operation::scroll_to(WEEK_SCROLL, AbsoluteOffset { x: 0.0, y: h * HOUR_H })
}

/// Ask the OS to drop Calen's idle pages shortly after windows went away
/// (giving the window teardown a moment to finish first).
pub fn trim_memory_later() -> Task<Message> {
    Task::perform(tokio::time::sleep(std::time::Duration::from_millis(1500)), |_| Message::TrimMemory)
}

pub fn open_url(u: &str) {
    if let Err(e) = open::that_detached(u) {
        log::error!("open {u}: {e}");
    }
}

pub fn first_of_month(d: NaiveDate) -> NaiveDate {
    d.with_day(1).unwrap()
}

pub fn add_months(d: NaiveDate, n: i32) -> NaiveDate {
    let total = d.year() * 12 + d.month0() as i32 + n;
    let (y, m) = (total.div_euclid(12), total.rem_euclid(12) as u32 + 1);
    let mut day = d.day();
    loop {
        if let Some(v) = NaiveDate::from_ymd_opt(y, m, day) {
            return v;
        }
        day -= 1;
    }
}

pub fn start_of_week(d: NaiveDate, start: chrono::Weekday) -> NaiveDate {
    let diff = (d.weekday().num_days_from_monday() as i64 - start.num_days_from_monday() as i64).rem_euclid(7);
    d - Duration::days(diff)
}

#[allow(dead_code)]
pub fn editor_content(s: &str) -> text_editor::Content {
    text_editor::Content::with_text(s)
}
