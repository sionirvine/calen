//! Right-click menu for events.
//!
//! The grids sit inside scrollables, so the cursor they see is in scrolled
//! coordinates. [`Tracker`] wraps the whole window instead and notes where each
//! right-button press happened, in window coordinates, without capturing it.

use std::cell::Cell;

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Shell, Widget, overlay, renderer};
use iced::widget::{Space, button, column, container, mouse_area, row, text};
use iced::{Alignment, Element, Event, Length, Padding, Point, Rectangle, Size, Vector, mouse};

use crate::app::{App, Message};
use crate::icons;
use crate::theme::{self, Palette};

pub const MENU_W: f32 = 168.0;
const MENU_H: f32 = 74.0;

pub struct Tracker<'a> {
    content: Element<'a, Message>,
    at: &'a Cell<Point>,
}

pub fn tracker<'a>(content: impl Into<Element<'a, Message>>, at: &'a Cell<Point>) -> Element<'a, Message> {
    Element::new(Tracker { content: content.into(), at })
}

impl Widget<Message, iced::Theme, iced::Renderer> for Tracker<'_> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &iced::Renderer, operation: &mut dyn Operation) {
        self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) = event
            && let Some(p) = cursor.position()
        {
            self.at.set(p);
        }
        self.content.as_widget_mut().update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}

/// An open event menu.
pub struct ContextMenu {
    pub key: crate::model::OccKey,
    pub at: Point,
    pub read_only: bool,
}

#[derive(Debug, Clone)]
pub enum MenuMsg {
    Edit,
    Delete,
    Close,
}

/// The menu at its point (kept inside the window), over a click-catcher that
/// closes it.
pub fn view<'a>(app: &'a App, p: Palette) -> Option<Element<'a, Message>> {
    let m = app.context_menu.as_ref()?;
    let item = |icon, label: &'static str, color, msg| {
        button(row![icons::icon(icon, 14.0, color), text(label).size(13).color(color)].spacing(8).align_y(Alignment::Center))
            .width(Length::Fill)
            .padding([6, 10])
            .style(theme::ghost)
            .on_press(Message::Context(msg))
    };
    let mut items = column![item(icons::pencil(), if m.read_only { "Open…" } else { "Edit…" }, p.text, MenuMsg::Edit)].spacing(2);
    if !m.read_only {
        items = items.push(item(icons::trash(), "Delete", p.danger, MenuMsg::Delete));
    }
    let menu = container(items).padding(4).width(Length::Fixed(MENU_W)).style(theme::popover);

    let size = app.main_size;
    let h = if m.read_only { MENU_H / 2.0 + 4.0 } else { MENU_H };
    let x = m.at.x.min(size.width - MENU_W - 6.0).max(0.0);
    let y = if m.at.y + h + 6.0 > size.height { (m.at.y - h).max(0.0) } else { m.at.y };
    let placed = column![Space::new().height(y), row![Space::new().width(x), iced::widget::opaque(menu)]];
    Some(
        mouse_area(container(placed).width(Length::Fill).height(Length::Fill).padding(Padding::ZERO))
            .on_press(Message::Context(MenuMsg::Close))
            .on_right_press(Message::Context(MenuMsg::Close))
            .on_scroll(|_| Message::Context(MenuMsg::Close))
            .into(),
    )
}
