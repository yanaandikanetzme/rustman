//! MOCK SERVERS: the sidebar section under Environments. A row shows the
//! mock's state, name, port and collection; Start/Stop shows on hover.
use crate::{
    app::AppState,
    domain::mock::MockServer,
    message::{Message, mock::MockMsg},
    state::mocks::{MockPage, RunStatus},
    ui::{
        components::{
            atoms::{row_action, text as t, tooltip::tooltip},
            molecules::{tree_row, tree_row::RowFill},
        },
        icons,
        theme::{Palette, UI_FONT_MEDIUM},
        tree_label::tree_label_weighted,
    },
};
use iced::{
    Alignment, Background, Border, Color, Element, Length,
    widget::{Space, button, column, container, mouse_area, row, scrollable, text},
};

const MOCK_ROW_H: f32 = 40.0;

pub fn view(state: &AppState) -> Element<'_, Message> {
    let mocks = &state.mocks;
    // The create action stays visible on the header, like Postman's `+`.
    let header = row![
        super::collections::section_header_with(
            "Mock servers",
            mocks.collapsed,
            Message::Mock(MockMsg::SectionToggled)
        ),
        row_action(
            icons::plus(),
            "New mock server",
            Message::Mock(MockMsg::CreateOpened)
        ),
    ]
    .align_y(Alignment::Center);
    let mut list = column![container(header).padding([2, 0])].spacing(0);
    if mocks.collapsed {
        return list.width(Length::Fill).into();
    }
    if mocks.servers.is_empty() {
        list = list.push(
            container(
                column![
                    t::meta("No mock servers yet."),
                    button(
                        row![
                            icons::plus().size(12).color(Palette::accent()),
                            text("Create mock server")
                                .size(12)
                                .font(UI_FONT_MEDIUM)
                                .color(Palette::accent()),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                    )
                    .padding(0)
                    .style(|_, _| button::Style::default())
                    .on_press(Message::Mock(MockMsg::CreateOpened)),
                ]
                .spacing(6),
            )
            .padding([6, 24]),
        );
        return list.width(Length::Fill).into();
    }
    let mut rows = column![].spacing(2);
    for mock in &mocks.servers {
        rows = rows.push(mock_row(state, mock));
    }
    list = list.push(
        container(
            scrollable(container(rows).padding(iced::padding::top(2)))
                .height(Length::Shrink)
                .style(crate::ui::theme::thin_scrollbar),
        )
        .max_height(176),
    );
    list.width(Length::Fill).into()
}

/// Status colour of a mock: running, starting, failed or stopped.
pub fn status_color(status: Option<&RunStatus>) -> Color {
    match status {
        Some(RunStatus::Running) => crate::ui::components::tone::Tone::Success.color(),
        Some(RunStatus::Starting) => crate::ui::components::tone::Tone::Warning.color(),
        Some(RunStatus::Failed(_)) => crate::ui::components::tone::Tone::Danger.color(),
        None => Palette::text_subtle(),
    }
}

/// A small filled square: Lucide's stop glyph is not in the bundled font.
fn stop_button<'a>(id: &str) -> Element<'a, Message> {
    let btn = button(
        container(
            container(Space::new().width(8).height(8)).style(|_| container::Style {
                background: Some(Background::Color(Palette::text_muted())),
                border: Border {
                    radius: 1.5.into(),
                    ..Default::default()
                },
                ..Default::default()
            }),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill),
    )
    .on_press(Message::Mock(MockMsg::Stop(id.to_owned())))
    .width(22)
    .height(22)
    .padding(0)
    .style(|_, s| button::Style {
        background: matches!(s, button::Status::Hovered)
            .then(|| Background::Color(Palette::hover())),
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    });
    tooltip(btn, "Stop mock server")
}

fn mock_row<'a>(state: &'a AppState, mock: &'a MockServer) -> Element<'a, Message> {
    let mocks = &state.mocks;
    let status = mocks.status(&mock.id);
    let live = status.is_some_and(RunStatus::is_live);
    let hovered = mocks.hovered.as_deref() == Some(mock.id.as_str());
    let selected = matches!(&mocks.page, Some(MockPage::Detail(id)) if id == &mock.id);
    let collection = state
        .collections
        .iter()
        .find(|c| c.id == mock.collection_id)
        .map_or("Collection deleted", |c| c.name.as_str());
    let detail = match status {
        Some(RunStatus::Failed(e)) => e.clone(),
        Some(_) => format!(":{} · {collection}", mock.port),
        None => format!("Stopped · {collection}"),
    };
    let action: Element<Message> = if !hovered {
        Space::new().width(22).height(22).into()
    } else if live {
        stop_button(&mock.id)
    } else {
        row_action(
            icons::play(),
            "Start mock server",
            Message::Mock(MockMsg::Start(mock.id.clone())),
        )
    };
    let width = (state.sidebar_width - 24.0 - 64.0).max(24.0);
    let name = tree_label_weighted(
        mock.name.clone(),
        12.0,
        width,
        UI_FONT_MEDIUM,
        if status.is_some() {
            Palette::text()
        } else {
            Palette::text_muted()
        },
    );
    let content = row![
        icons::dot(status_color(status)),
        column![
            name,
            text(detail)
                .size(10.5)
                .font(crate::ui::theme::MONO)
                .color(match status {
                    Some(RunStatus::Failed(_)) => crate::ui::components::tone::Tone::Danger.color(),
                    _ => Palette::text_subtle(),
                })
                .wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(1)
        .width(Length::Fill),
        action,
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let row = tree_row(
        content,
        18.0,
        MOCK_ROW_H,
        if selected || hovered {
            RowFill::NeutralSelected
        } else {
            RowFill::None
        },
        Message::Mock(MockMsg::Opened(mock.id.clone())),
    );
    mouse_area(row)
        .on_enter(Message::Mock(MockMsg::Hovered(Some(mock.id.clone()))))
        .on_exit(Message::Mock(MockMsg::Hovered(None)))
        .into()
}
