//! Headers tab (mockup `.hdr-toolbar` + `.kv`): label, auto-generated
//! headers toggle, Bulk Edit, Presets and a ⋯ menu above the table.

use iced::{
    Alignment, Background, Border, Element, Length,
    widget::{Space, button, checkbox, column, container, row, scrollable, text},
};

use crate::{
    domain::{
        environment::AppEnvironment,
        request::{AuthType, BodyType},
    },
    message::{Message, RequestMsg},
    services::header_suggest::{Source, Suggestion},
    state::tabs::{RequestTabState, SuggestField},
    ui::{
        components::molecules::menu_item,
        icons, styles,
        theme::{MONO, Palette, UI_FONT_MEDIUM},
        widgets::{kv_table, mini},
    },
};

pub const PRESETS_MENU: &str = "request:headers:presets";
pub const MORE_MENU: &str = "request:headers:more";

/// Presets menu: label and the Content-Type it sets ("" removes it).
const PRESETS: &[(&str, &str)] = &[
    ("None", ""),
    ("JSON", "application/json"),
    ("XML", "application/xml"),
    ("Text", "text/plain"),
    ("Form URL Encoded", "application/x-www-form-urlencoded"),
];

/// Shown instead of values that only exist once the request is built.
const CALCULATED: &str = "<calculated when request is sent>";

pub fn view<'a>(
    tab: &'a RequestTabState,
    env: Option<&'a AppEnvironment>,
    show_auto: bool,
    open_menu: Option<&str>,
) -> Element<'a, Message> {
    let auto = auto_headers(tab);
    let toolbar = toolbar(tab, auto.len(), show_auto, open_menu);
    let body: Element<'a, Message> = match &tab.headers_bulk {
        Some(content) => kv_table::bulk_panel(
            content,
            |a| Message::Request(RequestMsg::HeadersBulkEdited(a)),
            Message::Request(RequestMsg::HeadersBulkToggle),
        ),
        None => scrollable(column![header_table(tab, env, show_auto.then_some(auto))])
            .height(Length::Fill)
            .direction(iced::widget::scrollable::Direction::Vertical(
                crate::ui::theme::grabbable_scrollbar(),
            ))
            .style(crate::ui::theme::thin_scrollbar)
            .into(),
    };
    column![toolbar, body]
        .padding([14, 16])
        .height(Length::Fill)
        .into()
}

fn toolbar<'a>(
    tab: &'a RequestTabState,
    hidden: usize,
    show_auto: bool,
    open_menu: Option<&str>,
) -> Element<'a, Message> {
    let bulk = tab.headers_bulk.is_some();
    let presets = PRESETS
        .iter()
        .map(|(label, value)| {
            menu_item(
                None,
                *label,
                Some(Message::Request(RequestMsg::HeaderPresetApplied(
                    (*value).to_owned(),
                ))),
                false,
            )
        })
        .collect();
    let more_open = open_menu == Some(MORE_MENU);
    let more = mini::anchored_menu(
        mini::icon_button(
            icons::more(),
            "More",
            Some(Message::Sidebar(
                crate::message::SidebarMsg::ContextMenuOpened(MORE_MENU.to_owned()),
            )),
            more_open,
        ),
        more_open,
        vec![
            menu_item(
                Some(if show_auto {
                    icons::eye_closed()
                } else {
                    icons::eye()
                }),
                if show_auto {
                    "Hide auto-generated headers"
                } else {
                    "Show auto-generated headers"
                },
                Some(Message::Request(RequestMsg::AutoHeadersToggled)),
                false,
            ),
            menu_item(
                Some(icons::eraser()),
                "Remove all headers",
                (!tab.headers.is_empty() && !bulk)
                    .then_some(Message::Request(RequestMsg::HeadersCleared)),
                true,
            ),
        ],
        220.0,
    );
    container(
        row![
            text("Headers").size(12).color(Palette::text_muted()),
            hidden_toggle(hidden, show_auto),
            Space::new().width(Length::Fill),
            mini::text_button(
                None,
                if bulk {
                    "Key-Value Edit"
                } else {
                    "Bulk Edit (unmasked)"
                },
                Some(Message::Request(RequestMsg::HeadersBulkToggle)),
            ),
            mini::dropdown(
                "Presets",
                PRESETS_MENU,
                open_menu == Some(PRESETS_MENU),
                26.0,
                presets,
                200.0,
            ),
            more,
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding(iced::Padding {
        bottom: 10.0,
        ..Default::default()
    })
    .width(Length::Fill)
    .into()
}

/// `.hidden-toggle`: "N hidden" pill; accent while the auto-generated
/// headers are listed.
fn hidden_toggle<'a>(hidden: usize, on: bool) -> Element<'a, Message> {
    let label = if on {
        "Hide auto-generated headers".to_owned()
    } else {
        format!("{hidden} hidden")
    };
    let fg = if on {
        Palette::accent()
    } else {
        Palette::text_muted()
    };
    button(
        container(
            row![
                (if on {
                    icons::eye_closed()
                } else {
                    icons::eye()
                })
                .size(12)
                .color(fg),
                text(label).size(11).color(fg),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        )
        .height(24)
        .center_y(24),
    )
    .on_press(Message::Request(RequestMsg::AutoHeadersToggled))
    .padding([0, 9])
    .style(move |_, status| {
        let hot = matches!(status, button::Status::Hovered);
        button::Style {
            background: if on {
                Some(Background::Color(Palette::accent_soft()))
            } else if hot {
                Some(Background::Color(Palette::surface_high()))
            } else {
                None
            },
            text_color: fg,
            border: Border {
                color: if on {
                    Palette::accent()
                } else {
                    Palette::border()
                },
                width: 1.0,
                radius: 24.0.into(),
            },
            ..Default::default()
        }
    })
    .into()
}

/// A header the client adds on send (Postman's "hidden" headers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoHeader {
    pub name: String,
    pub value: String,
    /// An enabled table row sets the same header, so this value is not
    /// sent: Postman lists it struck through.
    pub overridden: bool,
}

/// Headers the client adds on send. Secrets never show: auth values are a
/// pointer to the Authorization tab.
pub fn auto_headers(tab: &RequestTabState) -> Vec<AutoHeader> {
    let user_sets = |name: &str| {
        tab.headers
            .iter()
            .any(|h| h.enabled && h.key.trim().eq_ignore_ascii_case(name))
    };
    let mut out: Vec<AutoHeader> = Vec::new();
    let mut add = |name: &str, value: &str| {
        out.push(AutoHeader {
            name: name.to_owned(),
            value: value.to_owned(),
            overridden: user_sets(name),
        });
    };
    let from_auth = "<from Authorization tab>";
    match tab.auth_type {
        AuthType::Bearer if !tab.bearer_token.is_empty() => add("Authorization", from_auth),
        AuthType::Basic if !tab.basic_user.is_empty() => add("Authorization", from_auth),
        AuthType::JwtUser if !tab.jwt_secret.is_empty() => add("Authorization", from_auth),
        AuthType::Cookie if !tab.cookie_string.is_empty() => add("Cookie", from_auth),
        AuthType::ApiKey
            if !tab.api_key_name.is_empty()
                && tab.api_key_location == crate::domain::request::ApiKeyLocation::Header =>
        {
            add(&tab.api_key_name, from_auth)
        }
        _ => {}
    }
    match tab.body_type {
        BodyType::Json | BodyType::GraphQL => add("Content-Type", "application/json"),
        BodyType::FormData => add("Content-Type", CALCULATED),
        BodyType::UrlEncoded => add("Content-Type", "application/x-www-form-urlencoded"),
        _ => {}
    }
    if tab.body_type != BodyType::None {
        add("Content-Length", CALCULATED);
    }
    add("Host", CALCULATED);
    add("User-Agent", concat!("rustman/", env!("CARGO_PKG_VERSION")));
    add("Accept", "*/*");
    out
}

/// Why an auto-generated header is there, for its ⓘ hint.
fn auto_hint(name: &str) -> &'static str {
    match name.to_ascii_lowercase().as_str() {
        "content-type" => "Set from the Body tab. Add your own Content-Type row to change it.",
        "content-length" => "Calculated from the body when the request is sent.",
        "host" => "Taken from the request URL when the request is sent.",
        "user-agent" => "Identifies Rustman to the server. Add a User-Agent row to change it.",
        "accept" => "Accepts any response type. Add an Accept row to change it.",
        _ => "Added from the Authorization tab.",
    }
}

fn header_table<'a>(
    tab: &'a RequestTabState,
    env: Option<&'a AppEnvironment>,
    auto: Option<Vec<AutoHeader>>,
) -> Element<'a, Message> {
    use iced::widget::{rich_text, span, tooltip};
    let mut rows: Vec<Element<Message>> = vec![kv_table::head3(Space::new().into())];

    for h in auto.unwrap_or_default() {
        let hint = auto_hint(&h.name);
        let info = tooltip(
            icons::info().size(12).color(Palette::text_subtle()),
            container(text(hint).size(11).color(Palette::text()))
                .padding([6, 8])
                .max_width(260)
                .style(|_| container::Style {
                    background: Some(Background::Color(Palette::surface_high())),
                    border: Border {
                        color: Palette::border(),
                        width: 1.0,
                        radius: 5.0.into(),
                    },
                    ..Default::default()
                }),
            tooltip::Position::Top,
        );
        let key = container(
            row![
                text(h.name)
                    .size(12)
                    .font(MONO)
                    .color(Palette::text_muted()),
                Space::new().width(Length::Fill),
                info,
            ]
            .align_y(Alignment::Center),
        )
        .padding([6, 7]);
        let value = container(
            rich_text::<(), Message, iced::Theme, iced::Renderer>([span(h.value)
                .color(Palette::text_subtle())
                .strikethrough(h.overridden)])
            .size(12)
            .font(MONO),
        )
        .padding([6, 7]);
        rows.push(kv_table::kv_row3(
            icons::check().size(12).color(Palette::text_subtle()).into(),
            key.into(),
            value.into(),
            Space::new().into(),
            Space::new().into(),
        ));
    }

    let open = tab.header_suggest;
    let list = tab.header_suggestions(env);
    for (i, item) in tab.headers.iter().enumerate() {
        let cell_open =
            |field| open.is_some_and(|s| s.row == i && s.field == field) && !list.is_empty();
        let key_cell =
            super::cell::text_cell("Key", &item.key, 12.0, [5, 7], Length::Fill, move |s| {
                Message::Request(RequestMsg::HeaderKeyChanged(i, s))
            });
        let value_cell = value_cell(
            i,
            &item.value,
            crate::services::variables::sensitive(&item.key),
        );
        let key_open = cell_open(SuggestField::Key);
        let value_open = cell_open(SuggestField::Value);
        let selected = open.map_or(0, |s| s.selected);
        let key_cell = suggest_cell(
            key_cell,
            i,
            SuggestField::Key,
            key_open.then_some(&list[..]).unwrap_or(&[]),
            selected,
            tab.var_card.as_ref(),
            env,
        );
        let value_cell = suggest_cell(
            value_cell,
            i,
            SuggestField::Value,
            value_open.then_some(&list[..]).unwrap_or(&[]),
            selected,
            tab.var_card.as_ref(),
            env,
        );
        let description = super::cell::text_cell(
            "Description",
            &item.description,
            12.0,
            [5, 7],
            Length::Fill,
            move |s| Message::Request(RequestMsg::HeaderDescriptionChanged(i, s)),
        );

        rows.push(kv_table::kv_row3(
            checkbox(item.enabled)
                .on_toggle(move |_| Message::Request(RequestMsg::HeaderToggled(i)))
                .size(13)
                .spacing(0)
                .style(styles::checkbox)
                .into(),
            key_cell,
            value_cell,
            description,
            kv_table::remove_button(Message::Request(RequestMsg::HeaderRemoved(i))),
        ));
    }

    rows.push(kv_table::add_row3(Message::Request(
        RequestMsg::HeaderAdded,
    )));
    kv_table::frame(rows)
}

/// A Key/Value cell with its autocomplete list. Always wrapped (open or
/// not) so the input keeps focus while the list comes and goes.
pub(crate) fn suggest_cell<'a>(
    cell: Element<'a, Message>,
    row: usize,
    field: SuggestField,
    list: &[Suggestion],
    selected: usize,
    card: Option<&'a crate::state::tabs::VarCard>,
    env: Option<&'a AppEnvironment>,
) -> Element<'a, Message> {
    use crate::ui::widgets::{click_probe::click_probe, popup};
    let probe = click_probe(
        cell,
        Message::Request(RequestMsg::HeaderSuggestOpened(row, field)),
    );
    let tracked = iced::widget::mouse_area(probe)
        .on_enter(Message::Request(RequestMsg::HeaderVarHovered(
            row, field, true,
        )))
        .on_exit(Message::Request(RequestMsg::HeaderVarHovered(
            row, field, false,
        )));
    // Both popups are always there (empty when closed) so the input's widget
    // tree, and its focus, stays the same while typing `{{`.
    let here = card.filter(|c| c.anchor == crate::state::tabs::VarCardAnchor::Header(row, field));
    let with_card = popup::popup_card(
        tracked.into(),
        here.map_or_else(
            || Space::new().width(0).height(0).into(),
            |c| super::var_card::view(c, env),
        ),
        Message::Request(RequestMsg::VarCardClosed),
        here.is_some(),
    );
    popup::popup_suggest(
        with_card,
        suggest_list(list, selected, move |apply| {
            Message::Request(RequestMsg::HeaderSuggestionPicked(row, field, apply))
        }),
        Message::Request(RequestMsg::HeaderSuggestClosed),
        !list.is_empty(),
        popup::SuggestNav {
            up: Message::Request(RequestMsg::HeaderSuggestMoved(false)),
            down: Message::Request(RequestMsg::HeaderSuggestMoved(true)),
            enter: Message::Request(RequestMsg::HeaderSuggestAccepted),
        },
    )
}

/// `{{name}}` tokens in a cell, in order, without repeats.
pub(crate) fn variable_names(text: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else { break };
        let name = after[..end].trim();
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
        }
        rest = &after[end + 2..];
    }
    out
}

/// Badge in front of a variable suggestion, like Postman's `E` / `G` chip.
pub(crate) fn source_badge(source: Source) -> Element<'static, Message> {
    let (label, color) = match source {
        Source::Plain => return Space::new().width(0).into(),
        Source::Environment => ("E", Palette::SUCCESS),
        Source::Dynamic => ("$", Palette::accent()),
    };
    container(text(label).size(10).font(UI_FONT_MEDIUM).color(color))
        .width(16)
        .height(16)
        .center_x(16)
        .center_y(16)
        .style(move |_| container::Style {
            background: Some(Background::Color(iced::Color { a: 0.16, ..color })),
            border: Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

/// Postman's autocomplete card: a scrolling list under the cell, the
/// highlighted row filled; variables carry a badge and their description.
pub(crate) fn suggest_list<'a>(
    list: &[Suggestion],
    selected: usize,
    on_pick: impl Fn(String) -> Message,
) -> Element<'a, Message> {
    use iced::widget::tooltip;
    let selected = selected.min(list.len().saturating_sub(1));
    let mut items = column![].spacing(1);
    for (i, s) in list.iter().enumerate() {
        let hot = i == selected;
        let label = row![
            source_badge(s.source),
            text(s.label.clone())
                .size(13)
                .font(crate::ui::theme::UI_FONT)
                .color(Palette::text()),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let item = button(container(label).padding([6, 10]).width(Length::Fill))
            .on_press(on_pick(s.apply.clone()))
            .padding(0)
            .width(Length::Fill)
            .style(move |_, status| button::Style {
                background: (hot || matches!(status, button::Status::Hovered))
                    .then(|| Background::Color(Palette::hover())),
                text_color: Palette::text(),
                border: Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            });
        let item: Element<'a, Message> = if s.detail.is_empty() {
            item.into()
        } else {
            tooltip(
                item,
                container(text(s.detail).size(12).color(Palette::text()))
                    .padding([6, 10])
                    .style(|_| container::Style {
                        background: Some(Background::Color(Palette::surface_high())),
                        border: Border {
                            color: Palette::border(),
                            width: 1.0,
                            radius: 5.0.into(),
                        },
                        ..Default::default()
                    }),
                tooltip::Position::Right,
            )
            .gap(6)
            .into()
        };
        items = items.push(item);
    }
    container(
        scrollable(items)
            .height(Length::Shrink)
            .direction(iced::widget::scrollable::Direction::Vertical(
                crate::ui::theme::grabbable_scrollbar(),
            ))
            .style(crate::ui::theme::thin_scrollbar),
    )
    .padding(4)
    .width(Length::Fixed(240.0))
    .max_height(260)
    .style(|_| container::Style {
        background: Some(Background::Color(Palette::surface())),
        border: Border {
            color: Palette::border(),
            width: 1.0,
            radius: 6.0.into(),
        },
        shadow: iced::Shadow {
            color: iced::Color {
                a: 0.35,
                ..iced::Color::BLACK
            },
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..Default::default()
    })
    .into()
}

fn value_cell(i: usize, value: &str, sensitive: bool) -> Element<'_, Message> {
    if sensitive {
        return super::cell::secret_cell(value, move |s| {
            Message::Request(RequestMsg::HeaderValueChanged(i, s))
        });
    }
    super::cell::text_cell("Value", value, 12.0, [5, 7], Length::Fill, move |s| {
        Message::Request(RequestMsg::HeaderValueChanged(i, s))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_rows_strike_the_auto_header_they_replace() {
        let mut tab = RequestTabState::new();
        tab.body_type = BodyType::Json;
        let content_type = |t: &RequestTabState| {
            auto_headers(t)
                .into_iter()
                .find(|h| h.name == "Content-Type")
                .expect("JSON body adds Content-Type")
        };
        assert!(!content_type(&tab).overridden);

        let mut ct = crate::domain::request::KeyValue::new_empty();
        ct.key = "content-type".into();
        ct.value = "text/plain".into();
        tab.headers.push(ct);
        assert!(content_type(&tab).overridden);
        // A disabled row sends nothing, so the auto value still applies.
        tab.headers[0].enabled = false;
        assert!(!content_type(&tab).overridden);
    }

    #[test]
    fn auth_values_never_show() {
        let mut tab = RequestTabState::new();
        tab.auth_type = AuthType::Bearer;
        tab.bearer_token = "s3cret".into();
        let auth = auto_headers(&tab)
            .into_iter()
            .find(|h| h.name == "Authorization")
            .expect("bearer adds Authorization");
        assert!(!auth.value.contains("s3cret"));
    }
}

#[cfg(test)]
mod variable_tip_tests {
    use super::variable_names;

    #[test]
    fn names_are_unique_trimmed_and_ignore_unclosed_tokens() {
        assert_eq!(
            variable_names("Bearer {{ token }}-{{dev}}{{token}}{{open"),
            vec!["token", "dev"]
        );
        assert!(variable_names("no vars").is_empty());
        assert!(variable_names("{{}}").is_empty());
    }
}
