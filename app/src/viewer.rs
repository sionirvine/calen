//! Full-window image viewer: scroll to zoom (around the cursor), drag to pan,
//! ←/→ to flip through an event's images, Esc to close.

use iced::keyboard::{Key, key::Named};
use iced::widget::image::{self, Handle};
use iced::widget::{Space, button, column, container, mouse_area, opaque, row, text};
use iced::{Alignment, Color, Element, Length, Subscription, Task};

use crate::app::{App, Message};
use crate::icons;
use crate::theme::{self, Palette};

pub struct Viewer {
    pub urls: Vec<String>,
    pub index: usize,
    pub handle: Option<Handle>,
    pub loading: bool,
    pub error: Option<String>,
    /// Bumped on every navigation so late loads of other images are ignored.
    generation: u64,
}

#[derive(Debug, Clone)]
pub enum ViewerMsg {
    Loaded(u64, Result<calen_images::Rgba, String>),
    Prev,
    Next,
    Close,
    OpenInBrowser,
}

pub fn is_open(app: &App) -> bool {
    app.viewer.is_some()
}

pub fn open(app: &mut App, urls: Vec<String>, index: usize) -> Task<Message> {
    if urls.is_empty() {
        return Task::none();
    }
    app.viewer = Some(Viewer { urls, index, handle: None, loading: false, error: None, generation: 0 });
    load(app)
}

fn load(app: &mut App) -> Task<Message> {
    let Some(v) = &mut app.viewer else { return Task::none() };
    v.index = v.index.min(v.urls.len() - 1);
    v.generation += 1;
    v.loading = true;
    v.error = None;
    // Show the thumbnail immediately while the full image downloads.
    v.handle = app.images.thumb(&v.urls[v.index]).cloned();
    let url = v.urls[v.index].clone();
    let generation = v.generation;
    Task::perform(async move { calen_images::full(&url).await.map_err(|e| e.to_string()) }, move |r| {
        Message::Viewer(ViewerMsg::Loaded(generation, r))
    })
}

pub fn close(app: &mut App) -> bool {
    app.viewer.take().is_some()
}

pub fn update(app: &mut App, m: ViewerMsg) -> Task<Message> {
    match m {
        ViewerMsg::Loaded(generation, r) => {
            if let Some(v) = &mut app.viewer
                && v.generation == generation
            {
                v.loading = false;
                match r {
                    Ok(img) => v.handle = Some(Handle::from_rgba(img.width, img.height, img.pixels)),
                    Err(e) => v.error = Some(e),
                }
            }
        }
        ViewerMsg::Prev | ViewerMsg::Next => {
            if let Some(v) = &mut app.viewer {
                let n = v.urls.len();
                v.index = if matches!(m, ViewerMsg::Next) { (v.index + 1) % n } else { (v.index + n - 1) % n };
                return load(app);
            }
        }
        ViewerMsg::Close => app.viewer = None,
        ViewerMsg::OpenInBrowser => {
            if let Some(v) = &app.viewer {
                crate::app::open_url(&v.urls[v.index]);
            }
        }
    }
    Task::none()
}

pub fn on_key(app: &mut App, key: &Key) -> Option<Task<Message>> {
    app.viewer.as_ref()?;
    Some(match key.as_ref() {
        Key::Named(Named::Escape) => update(app, ViewerMsg::Close),
        Key::Named(Named::ArrowLeft) => update(app, ViewerMsg::Prev),
        Key::Named(Named::ArrowRight) => update(app, ViewerMsg::Next),
        _ => Task::none(),
    })
}

pub fn subscription(_app: &App) -> Subscription<Message> {
    Subscription::none()
}

pub fn view<'a>(app: &'a App, p: Palette) -> Option<Element<'a, Message>> {
    let v = app.viewer.as_ref()?;
    let white = Color::WHITE;
    let btn = |icon: iced::widget::svg::Handle, msg: ViewerMsg| {
        button(icons::icon(icon, 18.0, white))
            .padding(8)
            .style(|_, s| button::Style {
                background: Some(theme::with_alpha(Color::WHITE, if matches!(s, button::Status::Hovered) { 0.18 } else { 0.08 }).into()),
                border: iced::Border { radius: 999.0.into(), ..Default::default() },
                text_color: Color::WHITE,
                ..Default::default()
            })
            .on_press(Message::Viewer(msg))
    };
    let mut bar = row![text(format!("{} / {}", v.index + 1, v.urls.len())).size(13).color(white)].spacing(8).align_y(Alignment::Center);
    if v.loading {
        bar = bar.push(text("Loading full resolution…").size(12).color(theme::with_alpha(white, 0.7)));
    }
    if let Some(e) = &v.error {
        bar = bar.push(text(format!("Couldn't load: {e}")).size(12).color(p.danger));
    }
    bar = bar.push(Space::new().width(Length::Fill));
    bar = bar.push(text("Scroll to zoom · drag to pan").size(12).color(theme::with_alpha(white, 0.6)));
    bar = bar.push(
        button(text("Open in browser ↗").size(12).color(white))
            .padding([6, 10])
            .style(|_, _| button::Style {
                background: Some(theme::with_alpha(Color::WHITE, 0.08).into()),
                border: iced::Border { radius: 999.0.into(), ..Default::default() },
                text_color: Color::WHITE,
                ..Default::default()
            })
            .on_press(Message::Viewer(ViewerMsg::OpenInBrowser)),
    );
    bar = bar.push(btn(icons::close(), ViewerMsg::Close));

    let body: Element<'a, Message> = match &v.handle {
        Some(h) => image::viewer(h.clone()).width(Length::Fill).height(Length::Fill).min_scale(0.1).max_scale(12.0).scale_step(0.15).into(),
        None => container(text(if v.loading { "Loading…" } else { "No preview" }).size(14).color(white)).center(Length::Fill).into(),
    };
    let mut middle = row![].align_y(Alignment::Center).height(Length::Fill);
    if v.urls.len() > 1 {
        middle = middle.push(container(btn(icons::chevron_left(), ViewerMsg::Prev)).padding(12));
    }
    middle = middle.push(container(body).width(Length::Fill).height(Length::Fill));
    if v.urls.len() > 1 {
        middle = middle.push(container(btn(icons::chevron_right(), ViewerMsg::Next)).padding(12));
    }
    Some(opaque(
        mouse_area(
            container(column![container(bar).padding([10, 14]), middle].spacing(0))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style { background: Some(Color::from_rgba(0.04, 0.04, 0.05, 0.94).into()), ..Default::default() }),
        )
        .on_right_press(Message::Viewer(ViewerMsg::Close)),
    ))
}
