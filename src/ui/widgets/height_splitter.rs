//! Drag handle on the top edge of a bottom panel with a pixel height. Works
//! from where the drag started, so it needs nothing from the surrounding
//! layout: moving up by N pixels makes the panel N pixels taller.
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{Clipboard, Layout, Shell, layout, mouse, renderer};
use iced::{Element, Event, Length, Rectangle, Size};

const THICKNESS: f32 = 8.0;
const MIN_HEIGHT: f32 = 100.0;
/// What the panel leaves to the rest of the window at most.
const MIN_LEFT_ABOVE: f32 = 220.0;

#[derive(Clone, Default)]
struct State {
    /// The cursor's y and the panel's height when the drag began.
    drag: Option<(f32, f32)>,
}

pub struct HeightSplitter<'a, Message> {
    height: f32,
    on_resize: Box<dyn Fn(f32) -> Message + 'a>,
}

pub fn height_splitter<'a, Message>(
    height: f32,
    on_resize: impl Fn(f32) -> Message + 'a,
) -> HeightSplitter<'a, Message> {
    HeightSplitter {
        height,
        on_resize: Box::new(on_resize),
    }
}

/// The height a drag asks for, kept usable in a window this tall.
fn dragged_height(start_y: f32, start_height: f32, y: f32, window_height: f32) -> f32 {
    let most = (window_height - MIN_LEFT_ABOVE).max(MIN_HEIGHT);
    (start_height + start_y - y).clamp(MIN_HEIGHT, most)
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for HeightSplitter<'_, Message>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(THICKNESS))
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.resolve(Length::Fill, Length::Fixed(THICKNESS), Size::ZERO))
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let dragging = tree.state.downcast_ref::<State>().drag.is_some();
        crate::ui::widgets::seam::draw(
            renderer,
            layout.bounds(),
            false,
            crate::ui::widgets::seam::LineAt::Center,
            dragging || cursor.is_over(layout.bounds()),
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let dragging = tree.state.downcast_ref::<State>().drag.is_some();
        if dragging || cursor.is_over(layout.bounds()) {
            mouse::Interaction::ResizingRow
        } else {
            mouse::Interaction::None
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(position) = cursor.position_over(layout.bounds()) {
                    state.drag = Some((position.y, self.height));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let Some((start_y, start_height)) = state.drag {
                    let height = dragged_height(start_y, start_height, position.y, viewport.height);
                    if height != self.height {
                        self.height = height;
                        shell.publish((self.on_resize)(height));
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.drag.take().is_some() {
                    shell.capture_event();
                }
            }
            _ => {}
        }
    }
}

impl<'a, Message, Theme, Renderer> From<HeightSplitter<'a, Message>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(splitter: HeightSplitter<'a, Message>) -> Self {
        Element::new(splitter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_up_grows_the_panel_within_limits() {
        assert_eq!(dragged_height(500.0, 280.0, 400.0, 900.0), 380.0);
        assert_eq!(dragged_height(500.0, 280.0, 600.0, 900.0), 180.0);
        assert_eq!(dragged_height(500.0, 280.0, 900.0, 900.0), MIN_HEIGHT);
        assert_eq!(
            dragged_height(500.0, 280.0, 0.0, 900.0),
            900.0 - MIN_LEFT_ABOVE
        );
        assert_eq!(
            dragged_height(100.0, 280.0, 50.0, 200.0),
            MIN_HEIGHT,
            "tiny window"
        );
    }
}
