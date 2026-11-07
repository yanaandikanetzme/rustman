//! Response headers as Postman shows them: a framed Key | Value table with
//! a header row and a hairline between rows; multi-value headers get one
//! row per value.

use iced::{
    Alignment, Background, Element, Length,
    widget::{column, container, row, scrollable, text},
};

use crate::{
    message::{Message, ResponseMsg},
    state::tabs::RequestTabState,
    ui::{
        icons,
        theme::{Palette, UI_FONT},
        widgets::{kv_table, mini},
    },
};

pub fn view(tab: &RequestTabState) -> Element<'_, Message> {
    let Some(resp) = &tab.response else {
        return kv_table::empty_state("No response yet.");
    };

    let mut entries: Vec<(&str, &str)> = resp
        .headers
        .iter()
        .flat_map(|(k, v)| v.split('\n').map(move |one| (k.as_str(), one)))
        .collect();
    entries.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

    if entries.is_empty() {
        return kv_table::empty_state("No response headers.");
    }

    let mut rows: Vec<Element<Message>> = vec![head("Key", "Value")];
    for (i, (key, value)) in entries.into_iter().enumerate() {
        let secret = crate::services::variables::sensitive(key);
        rows.push(line(tab, key, value, secret, format!("h:{i}")));
    }
    table(rows)
}

/// The framed table in the pane's padding, scrolling vertically.
pub(super) fn table(rows: Vec<Element<'_, Message>>) -> Element<'_, Message> {
    scrollable(column![container(kv_table::frame(rows)).padding(
        iced::Padding {
            top: 8.0,
            right: 16.0,
            bottom: 14.0,
            left: 16.0,
        }
    )])
    .width(Length::Fill)
    .height(Length::Fill)
    .direction(iced::widget::scrollable::Direction::Vertical(
        crate::ui::theme::grabbable_scrollbar(),
    ))
    .style(crate::ui::theme::thin_scrollbar)
    .into()
}

pub(super) fn head<'a>(key: &'static str, value: &'static str) -> Element<'a, Message> {
    let title = |s: &'static str| {
        container(
            text(s)
                .size(11)
                .font(iced::Font {
                    weight: iced::font::Weight::Semibold,
                    ..UI_FONT
                })
                .color(Palette::text_subtle()),
        )
        .width(Length::FillPortion(1))
        .padding([8, 12])
    };
    container(row![title(key), title(value)].align_y(Alignment::Center))
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(Palette::background())),
            ..Default::default()
        })
        .into()
}

/// One row. A `secret` value is masked until its eye shows it (`reveal`
/// names the row in the tab's revealed set).
pub(super) fn line<'a>(
    tab: &RequestTabState,
    key: &str,
    value: &str,
    secret: bool,
    reveal: String,
) -> Element<'a, Message> {
    let cell = |s: String, color| {
        container(
            text(s)
                .size(12)
                .color(color)
                .width(Length::Fill)
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
        )
        .width(Length::FillPortion(1))
        .padding([7, 12])
    };
    let shown = secret && tab.revealed_response.contains(&reveal);
    let text_value = if secret && !shown && !value.is_empty() {
        crate::services::variables::MASK.to_owned()
    } else {
        crate::services::format::head_capped(value, 512).to_owned()
    };
    let value_cell: Element<'a, Message> = if secret && !value.is_empty() {
        container(
            row![
                text(text_value)
                    .size(12)
                    .color(Palette::text_muted())
                    .width(Length::Fill)
                    .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
                mini::icon_button(
                    if shown {
                        icons::eye_closed()
                    } else {
                        icons::eye()
                    },
                    if shown { "Hide value" } else { "Show value" },
                    Some(Message::Response(ResponseMsg::RevealToggled(reveal))),
                    shown,
                ),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .width(Length::FillPortion(1))
        .padding(iced::Padding {
            top: 3.0,
            right: 6.0,
            bottom: 3.0,
            left: 12.0,
        })
        .into()
    } else {
        cell(text_value, Palette::text_muted()).into()
    };
    row![cell(key.to_owned(), Palette::text()), value_cell]
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_masked_row_shows_after_its_eye_and_hides_on_a_new_response() {
        let mut tab = RequestTabState::new();
        tab.response = Some(crate::domain::response::HttpResponse {
            status: 200,
            ..Default::default()
        });
        tab.revealed_response.insert("h:0".into());
        assert!(tab.revealed_response.contains("h:0"));
        tab.clear_response();
        assert!(tab.revealed_response.is_empty());
    }
}
