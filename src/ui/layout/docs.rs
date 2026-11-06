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
    let top = container(
        row![
            text(docs.name.as_str())
                .size(13)
                .font(semibold())
                .color(Palette::text()),
            Space::new().width(Length::Fill),
            text("Language").size(12).color(Palette::text_muted()),
            mini::anchored_menu(
                lang_button.into(),
                open_menu == Some(LANG_MENU),
                lang_items,
                200.0
            ),
            mini::icon_button(
                icons::close(),
                "Close documentation",
                Some(docs_msg(SidebarMsg::DocsClosed)),
                false,
            ),
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
