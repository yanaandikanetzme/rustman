//! Authorization tab (mockup `.auth-grid`): a 196px side column with the
//! auth type select and help, the selected type's form beside it.

use iced::{
    Alignment, Element, Length,
    widget::{Space, button, column, container, pick_list, row, scrollable, text},
};

use crate::{
    domain::request::AuthType,
    message::{Message, RequestMsg},
    state::tabs::{AuthField, RequestTabState, SuggestField},
    ui::{
        icons::{eye, eye_closed},
        styles,
        theme::{MONO, Palette},
    },
};

/// One entry of the auth type select: stored id plus display name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Choice {
    id: &'static str,
    label: &'static str,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label)
    }
}

const CHOICES: &[Choice] = &[
    Choice {
        id: "none",
        label: "No Auth",
    },
    Choice {
        id: "bearer",
        label: "Bearer Token",
    },
    Choice {
        id: "basic",
        label: "Basic Auth",
    },
    Choice {
        id: "apikey",
        label: "API Key",
    },
    Choice {
        id: "jwt-user",
        label: "JWT Bearer",
    },
    Choice {
        id: "cookie",
        label: "Cookie",
    },
];

fn choice(auth: &AuthType) -> Choice {
    CHOICES
        .iter()
        .copied()
        .find(|c| c.id == auth.as_str())
        .unwrap_or(CHOICES[0])
}

fn help(auth: &AuthType) -> &'static str {
    match auth {
        AuthType::None => "This request does not use any authorization.",
        AuthType::Bearer => {
            "The token is sent in the Authorization header as \"Bearer <token>\". \
             Use a {{variable}} to keep it out of the request."
        }
        AuthType::Basic => {
            "Username and password are base64-encoded into the Authorization header."
        }
        AuthType::ApiKey => "The key is sent as a header or a query parameter, your choice.",
        AuthType::JwtUser => {
            "A short-lived HS256 token is signed on every send and sent as a Bearer token."
        }
        AuthType::Cookie => "The cookie string is sent as-is in the Cookie header.",
    }
}

pub fn view<'a>(
    tab: &'a RequestTabState,
    env: Option<&'a crate::domain::environment::AppEnvironment>,
) -> Element<'a, Message> {
    let current = choice(&tab.auth_type);
    let side = column![
        text("Auth type").size(11).color(Palette::text_subtle()),
        pick_list(CHOICES, Some(current), |c: Choice| {
            Message::Request(RequestMsg::AuthTypeChanged(c.id.to_owned()))
        })
        .text_size(12)
        .padding([8, 10])
        .width(Length::Fill)
        .style(styles::boxed_pick_list),
        text(help(&tab.auth_type))
            .size(11)
            .color(Palette::text_subtle()),
    ]
    .spacing(8)
    .width(196);

    let main: Element<Message> = match &tab.auth_type {
        AuthType::None => container(
            text("No authorization is sent with this request.")
                .size(12)
                .color(Palette::text_muted()),
        )
        .padding([22, 0])
        .into(),
        AuthType::Bearer => form(
            "Bearer Token",
            vec![field_row(
                "Token",
                input(tab, env, AuthField::Bearer, "Token", |s| {
                    Message::Request(RequestMsg::BearerTokenChanged(s))
                }),
            )],
        ),
        AuthType::Basic => {
            let eye_icon = if tab.show_basic_pass {
                eye_closed()
            } else {
                eye()
            };
            form(
                "Basic Auth",
                vec![
                    field_row(
                        "Username",
                        input(tab, env, AuthField::BasicUser, "Username", |s| {
                            Message::Request(RequestMsg::BasicUserChanged(s))
                        }),
                    ),
                    field_row(
                        "Password",
                        row![
                            input(tab, env, AuthField::BasicPass, "Password", |s| {
                                Message::Request(RequestMsg::BasicPassChanged(s))
                            }),
                            button(eye_icon.size(14).color(Palette::text_muted()))
                                .on_press_maybe((!tab.basic_pass.is_empty()).then_some(
                                    Message::Request(RequestMsg::ToggleBasicPassVisibility),
                                ))
                                .style(iced::widget::button::text)
                                .padding(6),
                        ]
                        .spacing(6)
                        .align_y(Alignment::Center)
                        .into(),
                    ),
                ],
            )
        }
        AuthType::ApiKey => form(
            "API Key",
            vec![
                field_row(
                    "Key",
                    input(tab, env, AuthField::ApiKeyName, "Key name", |s| {
                        Message::Request(RequestMsg::ApiKeyNameChanged(s))
                    }),
                ),
                field_row(
                    "Value",
                    input(tab, env, AuthField::ApiKeyValue, "Value", |s| {
                        Message::Request(RequestMsg::ApiKeyValueChanged(s))
                    }),
                ),
                field_row(
                    "Add to",
                    pick_list(
                        &["header", "query"][..],
                        Some(tab.api_key_location.as_str()),
                        |s| Message::Request(RequestMsg::ApiKeyLocationChanged(s.to_owned())),
                    )
                    .text_size(12)
                    .padding([8, 10])
                    .width(160)
                    .style(styles::boxed_pick_list)
                    .into(),
                ),
            ],
        ),
        AuthType::Cookie => form(
            "Cookie",
            vec![field_row(
                "Cookie string",
                input(
                    tab,
                    env,
                    AuthField::Cookie,
                    "name=value; other=value",
                    |s| Message::Request(RequestMsg::CookieStringChanged(s)),
                ),
            )],
        ),
        AuthType::JwtUser => form(
            "JWT Bearer",
            vec![
                field_row(
                    "Subject (sub)",
                    input(tab, env, AuthField::JwtSubject, "Subject", |s| {
                        Message::Request(RequestMsg::JwtSubjectChanged(s))
                    }),
                ),
                field_row(
                    "Secret",
                    input(tab, env, AuthField::JwtSecret, "Secret", |s| {
                        Message::Request(RequestMsg::JwtSecretChanged(s))
                    }),
                ),
                field_row(
                    "Algorithm",
                    pick_list(&["HS256"][..], Some(tab.jwt_algo.as_str()), |s| {
                        Message::Request(RequestMsg::JwtAlgoChanged(s.to_owned()))
                    })
                    .text_size(12)
                    .padding([8, 10])
                    .width(160)
                    .style(styles::boxed_pick_list)
                    .into(),
                ),
                field_row(
                    "",
                    text("Token is generated per request (exp = now + 1h).")
                        .size(11)
                        .color(Palette::text_subtle())
                        .into(),
                ),
            ],
        ),
    };

    scrollable(
        row![side, container(main).width(Length::Fill).max_width(720)]
            .spacing(24)
            .padding([14, 16]),
    )
    .height(Length::Fill)
    .direction(iced::widget::scrollable::Direction::Vertical(
        crate::ui::theme::grabbable_scrollbar(),
    ))
    .style(crate::ui::theme::thin_scrollbar)
    .into()
}

/// `.auth-title` (14px semibold) over the field rows.
fn form<'a>(title: &'static str, rows: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    column![
        text(title)
            .size(14)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..crate::ui::theme::UI_FONT
            })
            .color(Palette::text()),
        Space::new().height(4),
        column(rows).spacing(12),
    ]
    .spacing(12)
    .into()
}

/// `.auth-row`: 150px label column, field beside it.
fn field_row<'a>(label: &'static str, field: Element<'a, Message>) -> Element<'a, Message> {
    row![
        text(label).size(12).color(Palette::text_muted()).width(150),
        container(field).width(Length::Fill),
    ]
    .spacing(16)
    .align_y(Alignment::Center)
    .into()
}

fn input<'a>(
    tab: &'a RequestTabState,
    env: Option<&'a crate::domain::environment::AppEnvironment>,
    field: AuthField,
    placeholder: &'static str,
    on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    let id = SuggestField::Auth(field);
    let value = tab.field_text(0, id).unwrap_or("");
    let secure = tab.field_secret(0, id);
    if !super::cell::is_inline(value) {
        let shown = if secure {
            crate::services::variables::MASK
        } else {
            crate::services::format::head_capped(value, 160)
        };
        return row![
            container(text(format!("{shown} [large value]")).size(12).font(MONO))
                .width(Length::Fill),
            button("Clear")
                .on_press(on_input(String::new()))
                .style(iced::widget::button::text),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into();
    }
    // `{{` lists the variables; a finished one opens its card on hover or
    // click, with a secret's value behind the eye.
    let list = tab.header_suggestions(env);
    let open = tab.header_suggest.filter(|s| s.row == 0 && s.field == id);
    super::headers::suggest_cell(
        super::cell::variable_field(
            tab,
            0,
            id,
            placeholder,
            super::cell::Frame::Boxed,
            env,
            on_input,
        ),
        0,
        id,
        if open.is_some() { &list[..] } else { &[] },
        open.map_or(0, |s| s.selected),
        tab.var_card.as_ref(),
        env,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_auth_type_has_a_choice() {
        for auth in [
            AuthType::None,
            AuthType::Bearer,
            AuthType::Basic,
            AuthType::ApiKey,
            AuthType::JwtUser,
            AuthType::Cookie,
        ] {
            assert_eq!(choice(&auth).id, auth.as_str());
        }
    }
}
