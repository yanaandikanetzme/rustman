//! The collection documentation page: every request with its URL, auth,
//! body and examples as code in the chosen language (Postman's).
use iced::{
    Alignment, Background, Border, Element, Length,
    widget::{Space, button, column, container, row, rule, scrollable, text},
};

use crate::{
    app::AppState,
    message::{Message, SidebarMsg},
    services::snippet::Lang,
    state::docs::{COLLAPSED_LINES, CollectionDocs, DocRequest},
    ui::{
        components::molecules::menu::menu_item,
        icons,
        theme::{MONO, Palette, UI_FONT_MEDIUM},
        widgets::mini,
    },
};

const LANG_MENU: &str = "docs-lang";

fn semibold() -> iced::Font {
    iced::Font {
        weight: iced::font::Weight::Semibold,
        ..crate::ui::theme::UI_FONT
    }
}

fn docs_msg(msg: SidebarMsg) -> Message {
    Message::Sidebar(msg)
}

pub(super) fn view<'a>(state: &'a AppState, docs: &'a CollectionDocs) -> Element<'a, Message> {
    let open_menu = state.sidebar.context_menu.as_deref();
    let lang_items = Lang::ALL
        .into_iter()
        .map(|lang| {
            menu_item(
                (lang == docs.lang).then(icons::check),
                lang.label(),
                Some(docs_msg(SidebarMsg::DocsLangSet(lang))),
                false,
            )
        })
        .collect();
    let lang_button = button(
        row![
            text(docs.lang.label())
                .size(12)
                .font(UI_FONT_MEDIUM)
                .color(Palette::text()),
            icons::chevron_down().size(11).color(Palette::text_muted()),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .on_press(docs_msg(SidebarMsg::ContextMenuOpened(
        LANG_MENU.to_owned(),
    )))
    .padding([3, 8])
    .style(iced::widget::button::text);
    let close = mini::icon_button(
        icons::close(),
        "Close",
        Some(docs_msg(SidebarMsg::DocsClosed)),
        false,
    );
    if !docs.full {
        return overview(docs, close);
    }
    let back = button(
        row![
            icons::arrow_left().size(12).color(Palette::text_muted()),
            text(docs.name.as_str())
                .size(13)
                .font(semibold())
                .color(Palette::text()),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .on_press(docs_msg(SidebarMsg::DocsFullToggled(false)))
    .padding([3, 6])
    .style(iced::widget::button::text);
    let top = container(
        row![
            tooltip(back, "Back to the overview"),
            Space::new().width(Length::Fill),
            text("Language").size(12).color(Palette::text_muted()),
            mini::anchored_menu(
                lang_button.into(),
                open_menu == Some(LANG_MENU),
                lang_items,
                200.0
            ),
            close,
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([8, 16]);

    let mut page = column![
        text(docs.name.as_str())
            .size(24)
            .font(semibold())
            .color(Palette::text()),
        text(match docs.request_count {
            1 => "1 request".to_owned(),
            n => format!("{n} requests"),
        })
        .size(12)
        .color(Palette::text_muted()),
        rule::horizontal(1).style(crate::ui::styles::divider),
    ]
    .spacing(10);
    if docs.request_count == 0 {
        page = page.push(
            text("This collection has no requests yet.")
                .size(12)
                .color(Palette::text_muted()),
        );
    }
    for section in &docs.sections {
        if let Some(title) = &section.title {
            page = page.push(Space::new().height(14)).push(
                text(title.to_uppercase())
                    .size(17)
                    .font(semibold())
                    .color(Palette::text()),
            );
        }
        for req in &section.requests {
            page = page
                .push(Space::new().height(18))
                .push(request_block(state, docs, req));
        }
    }
    let body = scrollable(
        container(container(page.padding([24, 24])).max_width(780))
            .center_x(Length::Fill)
            .padding(iced::padding::bottom(40)),
    )
    .height(Length::Fill)
    .style(crate::ui::theme::thin_scrollbar);
    column![
        top,
        rule::horizontal(1).style(crate::ui::styles::divider),
        body
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn tooltip<'a>(content: impl Into<Element<'a, Message>>, tip: &'a str) -> Element<'a, Message> {
    iced::widget::tooltip(
        content,
        container(text(tip).size(12).color(Palette::text()))
            .padding([5, 8])
            .style(|_| container::Style {
                background: Some(Background::Color(Palette::surface_high())),
                border: Border {
                    color: Palette::border(),
                    width: 1.0,
                    radius: 5.0.into(),
                },
                ..Default::default()
            }),
        iced::widget::tooltip::Position::Bottom,
    )
    .into()
}

/// When a collection was created, from either unit storage has used.
fn created_label(created_at: i64) -> Option<String> {
    if created_at <= 0 {
        return None;
    }
    // Older imports stored seconds, everything else milliseconds.
    let millis = if created_at < 100_000_000_000 {
        created_at * 1000
    } else {
        created_at
    };
    let at = chrono::DateTime::from_timestamp_millis(millis)?.with_timezone(&chrono::Local);
    Some(at.format("%-I:%M %p, %B %d, %Y").to_string())
}

/// Postman's collection Overview: the name, what it holds, when it was made,
/// and the way into the complete documentation.
fn overview<'a>(docs: &'a CollectionDocs, close: Element<'a, Message>) -> Element<'a, Message> {
    let overview_tab = container(
        text("Overview")
            .size(12)
            .font(UI_FONT_MEDIUM)
            .color(Palette::text()),
    )
    .padding([4, 8])
    .style(|_| container::Style {
        background: Some(Background::Color(Palette::surface_high())),
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    });
    let top = container(
        row![overview_tab, Space::new().width(Length::Fill), close]
            .spacing(8)
            .align_y(Alignment::Center),
    )
    .padding([8, 16]);
    let plural = |n: usize, one: &str, many: &str| {
        if n == 1 {
            format!("1 {one}")
        } else {
            format!("{n} {many}")
        }
    };
    let mut facts = row![
        icons::send().size(12).color(Palette::text_muted()),
        text(plural(docs.request_count, "request", "requests"))
            .size(12)
            .color(Palette::text_muted()),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    if docs.folder_count > 0 {
        facts = facts.push(Space::new().width(10)).push(
            row![
                icons::folder().size(12).color(Palette::text_muted()),
                text(plural(docs.folder_count, "folder", "folders"))
                    .size(12)
                    .color(Palette::text_muted()),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    }
    let mut page = column![
        text(docs.name.as_str())
            .size(24)
            .font(semibold())
            .color(Palette::text()),
        facts,
    ]
    .spacing(12);
    if let Some(created) = created_label(docs.created_at) {
        page = page.push(
            row![
                icons::timer().size(12).color(Palette::text_muted()),
                text(created).size(12).color(Palette::text_muted()),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    }
    page = page
        .push(rule::horizontal(1).style(crate::ui::styles::divider))
        .push(Space::new().height(8))
        .push(
            button(
                text("View complete documentation")
                    .size(13)
                    .color(Palette::accent()),
            )
            .on_press(docs_msg(SidebarMsg::DocsFullToggled(true)))
            .padding(0)
            .style(iced::widget::button::text),
        );
    let body = scrollable(
        container(container(page.padding([28, 24])).max_width(700)).center_x(Length::Fill),
    )
    .height(Length::Fill)
    .style(crate::ui::theme::thin_scrollbar);
    column![
        top,
        rule::horizontal(1).style(crate::ui::styles::divider),
        body
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn heading<'a>(title: &'a str, note: Option<String>) -> Element<'a, Message> {
    let mut line = row![text(title).size(15).font(semibold()).color(Palette::text())]
        .spacing(8)
        .align_y(Alignment::End);
    if let Some(note) = note {
        line = line.push(text(note).size(11).color(Palette::text_muted()));
    }
    line.into()
}

fn request_block<'a>(
    state: &'a AppState,
    docs: &'a CollectionDocs,
    req: &'a DocRequest,
) -> Element<'a, Message> {
    let r = &req.request;
    let open = button(
        row![
            text("Open request").size(12).color(Palette::accent()),
            icons::arrow_right().size(12).color(Palette::accent()),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
    )
    .on_press(docs_msg(SidebarMsg::RequestOpened(r.clone())))
    .padding([2, 4])
    .style(iced::widget::button::text);
    let mut out = column![
        row![
            text(r.method.as_str())
                .size(16)
                .font(semibold())
                .color(super::style::method_color(&r.method)),
            text(r.name.as_str())
                .size(16)
                .font(semibold())
                .color(Palette::text())
                .width(Length::Fill),
            open,
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        container(
            text(if r.url.is_empty() {
                "(no URL)"
            } else {
                r.url.as_str()
            })
            .size(12)
            .font(MONO)
            .color(Palette::text())
            .wrapping(text::Wrapping::Glyph)
        )
        .padding([8, 10])
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(Palette::surface_high())),
            border: Border {
                radius: 5.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }),
    ]
    .spacing(12);

    if let Some((kind, rows)) = &req.auth {
        let mut table = column![].spacing(8);
        for (key, value) in rows {
            table = table.push(
                row![
                    text(key.as_str())
                        .size(12)
                        .font(UI_FONT_MEDIUM)
                        .color(Palette::text())
                        .width(180),
                    text(value.as_str())
                        .size(12)
                        .font(MONO)
                        .color(Palette::text_muted())
                        .width(Length::Fill)
                        .wrapping(text::Wrapping::Glyph),
                ]
                .spacing(12),
            );
        }
        out = out.push(
            column![
                heading("Authorization", Some((*kind).to_owned())),
                rule::horizontal(1).style(crate::ui::styles::divider),
                table,
            ]
            .spacing(8),
        );
    }

    if let Some((label, body)) = &req.body {
        let syntax = if label.contains("json") || label == "GraphQL" {
            "json"
        } else {
            "text"
        };
        out = out.push(
            column![
                heading("Body", Some(label.clone())),
                code_block(docs, format!("{}:body", r.id), syntax, body),
            ]
            .spacing(8),
        );
    }

    let lang = docs.lang.label();
    if req.examples.is_empty() {
        out = out.push(
            column![
                heading("Example", None),
                sub("Request"),
                code_block(docs, format!("{}:snippet", r.id), lang, &req.snippet),
            ]
            .spacing(8),
        );
        return out.into();
    }

    let chosen = docs
        .chosen
        .get(&r.id)
        .copied()
        .unwrap_or(0)
        .min(req.examples.len() - 1);
    let example = &req.examples[chosen];
    let menu_key = format!("docs-example:{}", r.id);
    let picker: Element<Message> = if req.examples.len() > 1 {
        let items = req
            .examples
            .iter()
            .enumerate()
            .map(|(i, e)| {
                menu_item(
                    (i == chosen).then(icons::check),
                    e.name.clone(),
                    Some(docs_msg(SidebarMsg::DocsExampleChosen(r.id.clone(), i))),
                    false,
                )
            })
            .collect();
        mini::anchored_menu(
            button(
                row![
                    text(example.name.as_str())
                        .size(12)
                        .color(Palette::text_muted()),
                    icons::chevron_down().size(11).color(Palette::text_muted()),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .on_press(docs_msg(SidebarMsg::ContextMenuOpened(menu_key.clone())))
            .padding([3, 8])
            .style(iced::widget::button::text)
            .into(),
            state.sidebar.context_menu.as_deref() == Some(menu_key.as_str()),
            items,
            240.0,
        )
    } else {
        text(example.name.as_str())
            .size(12)
            .color(Palette::text_muted())
            .into()
    };

    let headers_key = format!("{}:headers:{chosen}", r.id);
    let showing_headers = docs.expanded.contains(&headers_key);
    let tab = |label: String, active: bool| {
        button(text(label).size(12).color(if active {
            Palette::text()
        } else {
            Palette::text_muted()
        }))
        .on_press_maybe((!active).then(|| docs_msg(SidebarMsg::DocsToggled(headers_key.clone()))))
        .padding([3, 8])
        .style(move |_, _| button::Style {
            background: active.then(|| Background::Color(Palette::surface_high())),
            border: Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
    };
    let status_color = match example.status {
        200..=299 => Palette::SUCCESS,
        300..=399 => Palette::WARNING,
        _ => Palette::ERROR,
    };
    let response_tabs = row![
        tab("Body".to_owned(), !showing_headers),
        tab(
            format!("Headers ({})", example.headers.len()),
            showing_headers
        ),
        Space::new().width(Length::Fill),
        container(
            text(format!("{} {}", example.status, example.status_text))
                .size(11)
                .font(MONO)
                .color(status_color)
        )
        .padding([2, 6])
        .style(move |_| container::Style {
            border: Border {
                color: status_color,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        }),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    let response: Element<Message> = if showing_headers {
        let headers: String = example
            .headers
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join("\n");
        code_block(
            docs,
            format!("{}:resp-headers:{chosen}", r.id),
            "headers",
            &headers,
        )
    } else {
        code_block(
            docs,
            format!("{}:resp-body:{chosen}", r.id),
            "json",
            &example.body,
        )
    };
    out = out.push(
        column![
            row![
                heading("Example", None),
                Space::new().width(Length::Fill),
                picker
            ]
            .align_y(Alignment::Center),
            rule::horizontal(1).style(crate::ui::styles::divider),
            sub("Request"),
            code_block(
                docs,
                format!("{}:example:{chosen}", r.id),
                lang,
                &example.snippet
            ),
            sub("Response"),
            response_tabs,
            response,
        ]
        .spacing(8),
    );
    out.into()
}

fn sub(label: &str) -> Element<'_, Message> {
    text(label)
        .size(12)
        .font(UI_FONT_MEDIUM)
        .color(Palette::text_muted())
        .into()
}

/// A read-only code card: language chip and Copy at the top, the first
/// lines until "View more".
fn code_block<'a>(
    docs: &CollectionDocs,
    key: String,
    label: &'a str,
    content: &str,
) -> Element<'a, Message> {
    let expanded = docs.expanded.contains(&key);
    let total = content.lines().count();
    let long = total > COLLAPSED_LINES;
    let shown: String = if long && !expanded {
        content
            .lines()
            .take(COLLAPSED_LINES)
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        content.to_owned()
    };
    let mut card = column![
        row![
            container(text(label).size(11).color(Palette::text_muted()))
                .padding([2, 6])
                .style(|_| container::Style {
                    background: Some(Background::Color(Palette::surface_high())),
                    border: Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            Space::new().width(Length::Fill),
            mini::icon_button(
                icons::copy(),
                "Copy",
                Some(docs_msg(SidebarMsg::DocsCopied(content.to_owned()))),
                false,
            ),
        ]
        .align_y(Alignment::Center),
        text(if content.is_empty() {
            "(empty)".to_owned()
        } else {
            shown
        })
        .size(12)
        .font(MONO)
        .color(Palette::text())
        .wrapping(text::Wrapping::Glyph),
    ]
    .spacing(8);
    if long {
        card = card.push(
            container(
                button(
                    text(if expanded { "View less" } else { "View more" })
                        .size(12)
                        .color(Palette::text()),
                )
                .on_press(docs_msg(SidebarMsg::DocsToggled(key)))
                .padding([3, 10])
                .style(|_, _| button::Style {
                    background: Some(Background::Color(Palette::surface_high())),
                    border: Border {
                        radius: 10.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            )
            .center_x(Length::Fill),
        );
    }
    container(card)
        .padding(10)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(Palette::background())),
            border: Border {
                color: Palette::border_subtle(),
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::created_label;

    #[test]
    fn created_dates_read_the_same_from_seconds_and_milliseconds() {
        assert_eq!(created_label(0), None);
        let seconds = created_label(1_762_400_000).unwrap();
        assert_eq!(created_label(1_762_400_000_000), Some(seconds.clone()));
        assert!(seconds.contains("2025"), "{seconds}");
    }
}
