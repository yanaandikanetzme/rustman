//! The Console panel: every request as an expandable row (network facts,
//! headers and bodies of both sides) and what scripts wrote, in order.
use crate::{
    app::AppState,
    message::{LayoutMsg, Message},
    state::console::{ConsoleEntry, ConsoleState, NetworkEntry},
    ui::{
        components::atoms::Button,
        icons,
        theme::{MONO, Palette, UI_FONT_MEDIUM},
    },
};
use iced::{
    Alignment, Element, Length,
    widget::{Space, button, column, container, row, scrollable, text},
};

const VALUE_BLUE: iced::Color = iced::Color::from_rgb(0.49, 0.70, 0.98);

pub(super) fn view(state: &AppState) -> Element<'_, Message> {
    let console = &state.console;
    let mut rows = column![].spacing(2).padding(iced::padding::right(12));
    for entry in &console.entries {
        rows = rows.push(match entry {
            ConsoleEntry::Log { text: line, error } => text(line.clone())
                .size(12)
                .font(MONO)
                .color(if *error {
                    Palette::ERROR
                } else {
                    Palette::text()
                })
                .wrapping(text::Wrapping::Glyph)
                .into(),
            ConsoleEntry::Network(entry) => network_row(entry, console),
        });
    }
    let body: Element<Message> = if console.entries.is_empty() {
        text("Nothing yet. Requests and console.log output appear here.")
            .size(12)
            .color(Palette::text_muted())
            .into()
    } else {
        scrollable(rows)
            .anchor_bottom()
            .height(Length::Fill)
            .style(crate::ui::theme::thin_scrollbar)
            .into()
    };
    container(
        column![
            row![
                text("Console")
                    .size(12)
                    .font(UI_FONT_MEDIUM)
                    .color(Palette::text()),
                Space::new().width(Length::Fill),
                Button::new("Clear")
                    .ghost()
                    .on_press(Message::Layout(LayoutMsg::ConsoleCleared)),
                Button::new("Copy")
                    .ghost()
                    .on_press(Message::Layout(LayoutMsg::ConsoleCopied)),
                Button::new("Close")
                    .ghost()
                    .on_press(Message::Layout(LayoutMsg::ConsoleToggled)),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
            body,
        ]
        .spacing(6),
    )
    .padding([6, 12])
    .height(280)
    .width(Length::Fill)
    .style(crate::ui::styles::content_surface)
    .into()
}

fn toggle_row<'a>(key: String, open: bool, content: Element<'a, Message>) -> Element<'a, Message> {
    button(
        row![
            if open {
                icons::chevron_down()
            } else {
                icons::chevron_right()
            }
            .size(11)
            .color(Palette::text_muted()),
            content,
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .on_press(Message::Layout(LayoutMsg::ConsoleRowToggled(key)))
    .padding([2, 4])
    .width(Length::Fill)
    .style(|_, status| button::Style {
        background: matches!(status, button::Status::Hovered)
            .then(|| iced::Background::Color(Palette::hover())),
        text_color: Palette::text(),
        ..Default::default()
    })
    .into()
}

fn status_color(entry: &NetworkEntry) -> iced::Color {
    match entry.status {
        _ if entry.error.is_some() => Palette::ERROR,
        200..=299 => Palette::SUCCESS,
        300..=399 => Palette::WARNING,
        _ => Palette::ERROR,
    }
}

fn network_row<'a>(entry: &'a NetworkEntry, console: &'a ConsoleState) -> Element<'a, Message> {
    let id = entry.id;
    let open = console.expanded.contains(&id.to_string());
    let status = if let Some(error) = &entry.error {
        crate::services::format::head_capped(error, 80).to_owned()
    } else {
        format!("{} {}", entry.status, entry.status_text)
    };
    let summary = row![
        text(entry.request.method.as_str().to_owned())
            .size(12)
            .font(MONO)
            .color(super::style::method_color(&entry.request.method)),
        text(crate::services::format::head_capped(&entry.request.url, 300).to_owned())
            .size(12)
            .font(MONO)
            .color(Palette::text())
            .width(Length::Fill)
            .wrapping(text::Wrapping::None),
        text(status).size(12).font(MONO).color(status_color(entry)),
        text(format!("{} ms", entry.duration_ms))
            .size(12)
            .font(MONO)
            .color(Palette::text_muted()),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let mut out = column![toggle_row(id.to_string(), open, summary.into())].spacing(1);
    if open {
        let n = &entry.network;
        let mut facts = Vec::new();
        if !n.http_version.is_empty() {
            facts.push(("HTTP Version".to_owned(), n.http_version.clone()));
        }
        if let Some(address) = &n.remote_addr {
            facts.push(("Remote Address".to_owned(), address.clone()));
        }
        facts.push(("Response Size".to_owned(), format!("{} B", entry.size)));
        let sections = column![
            pairs_section(id, "Network", &facts, console),
            pairs_section(id, "Request Headers", &entry.request.headers, console),
            body_section(id, "Request Body", &entry.request.body, console),
            pairs_section(id, "Response Headers", &entry.response_headers, console),
            body_section(id, "Response Body", &entry.response_body, console),
        ]
        .spacing(1)
        .padding(iced::padding::left(16));
        out = out.push(sections);
    }
    out.into()
}

fn section_header<'a>(id: u64, title: &'a str, open: bool) -> Element<'a, Message> {
    toggle_row(
        section_key(id, title),
        open,
        text(title).size(12).color(Palette::text_muted()).into(),
    )
}

fn section_key(id: u64, title: &str) -> String {
    format!("{id}:{title}")
}

fn pairs_section<'a>(
    id: u64,
    title: &'a str,
    pairs: &[(String, String)],
    console: &ConsoleState,
) -> Element<'a, Message> {
    let open = console.expanded.contains(&section_key(id, title));
    let mut out = column![section_header(id, title, open)].spacing(1);
    if open {
        let mut lines = column![].spacing(2).padding(iced::padding::left(20));
        for (key, value) in pairs {
            lines = lines.push(
                row![
                    text(format!("{key}:"))
                        .size(12)
                        .font(MONO)
                        .color(Palette::text()),
                    text(format!("\"{value}\""))
                        .size(12)
                        .font(MONO)
                        .color(VALUE_BLUE)
                        .width(Length::Fill)
                        .wrapping(text::Wrapping::Glyph),
                ]
                .spacing(6),
            );
        }
        out = out.push(lines);
    }
    out.into()
}

fn body_section<'a>(
    id: u64,
    title: &'a str,
    body: &str,
    console: &ConsoleState,
) -> Element<'a, Message> {
    let open = console.expanded.contains(&section_key(id, title));
    let mut out = column![section_header(id, title, open)].spacing(1);
    if open {
        let shown = if body.is_empty() {
            text("(empty)").size(12).color(Palette::text_muted())
        } else {
            text(body.to_owned())
                .size(12)
                .font(MONO)
                .color(Palette::text())
                .wrapping(text::Wrapping::Glyph)
        };
        out = out.push(container(shown).padding(iced::padding::left(20)));
    }
    out.into()
}
