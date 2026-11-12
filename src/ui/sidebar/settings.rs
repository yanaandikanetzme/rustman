use iced::{
    Background, Border, Color, Element, Length,
    widget::{Space, button, column, container, row, scrollable, svg, text, text_input},
};

const LOGO_SVG: &[u8] = include_bytes!("../../../public/rustman-logo.svg");

use crate::{
    message::{Message, SettingsMsg},
    ui::theme::{Palette, TEXT_SM, THEMES, ThemeSpec},
    ui::tree_label::tree_label,
};

pub fn view(state: &crate::app::AppState) -> Element<'_, Message> {
    let header = build_header();
    let git_identity = card(build_git_identity(
        &state.git_user_name,
        &state.git_user_email,
    ));
    let appearance = card(build_appearance(state.theme_idx));
    let layout = card(build_layout(state.horizontal_layout, state.sidebar_width));
    let request_defaults = card(build_request_defaults(&state.default_timeout_text));
    let tls = card(build_tls(state.tls_options));
    let global_scripts = card(build_global_scripts(
        state.global_pre_request_editor.text().len(),
        state.global_test_editor.text().len(),
        state.sidebar_width,
    ));
    let shortcuts = card(build_shortcuts(state.sidebar_width));

    let sections = column![
        header,
        git_identity,
        appearance,
        layout,
        request_defaults,
        tls,
        global_scripts,
        shortcuts,
    ];

    scrollable(sections.spacing(6).padding(iced::Padding {
        top: 0.0,
        right: 8.0,
        bottom: 8.0,
        left: 8.0,
    }))
    .height(Length::Fill)
    .style(crate::ui::theme::hidden_scrollbar)
    .into()
}

fn build_git_identity(name: &str, email: &str) -> Element<'static, Message> {
    column![
        section_label("GIT IDENTITY"),
        field_label("Name"),
        text_input("Your Name", name)
            .on_input(|v| Message::Settings(SettingsMsg::GitNameChanged(v)))
            .size(12)
            .padding([6, 10])
            .style(crate::ui::styles::field_input),
        Space::new().height(6),
        field_label("Email"),
        text_input("you@example.com", email)
            .on_input(|v| Message::Settings(SettingsMsg::GitEmailChanged(v)))
            .size(12)
            .padding([6, 10])
            .style(crate::ui::styles::field_input),
        Space::new().height(4),
        text("Used for git commits. Saves to the active repo's config.")
            .size(9)
            .color(Palette::text_subtle()),
    ]
    .spacing(0)
    .into()
}

fn build_request_defaults(timeout_text: &str) -> Element<'static, Message> {
    column![
        section_label("REQUEST DEFAULTS"),
        field_label("Timeout (ms)"),
        text_input("30000", timeout_text)
            .on_input(|v| Message::Settings(SettingsMsg::DefaultTimeoutChanged(v)))
            .size(12)
            .padding([6, 10])
            .width(Length::Fill)
            .style(crate::ui::styles::field_input),
        Space::new().height(4),
        text("Applied to every request. 0 falls back to 30000.")
            .size(9)
            .color(Palette::text_subtle()),
    ]
    .spacing(0)
    .into()
}

/// TLS / connection workarounds for endpoints a default client cannot reach
/// (issue #40: works in Chrome/curl, fails in Rustman with `os error 10054`).
fn build_tls(options: crate::services::http::TlsOptions) -> Element<'static, Message> {
    use crate::message::TlsOption;

    column![
        section_label("TLS & CONNECTION"),
        tls_toggle(
            "Send HTTP/1.1 only",
            "Try this first. Rustman offers HTTP/2 in the handshake; some servers              close the connection instead of declining it (Windows: os error 10054).",
            options.http1_only,
            TlsOption::Http1Only,
        ),
        Space::new().height(6),
        tls_toggle(
            "Ignore certificate errors",
            "Accepts self-signed, internal-CA and mismatched-hostname certificates.              Like curl -k — only use it for endpoints you trust.",
            options.accept_invalid_certs,
            TlsOption::AcceptInvalidCerts,
        ),
        Space::new().height(6),
        tls_toggle(
            "Force TLS 1.2",
            "Pin the handshake to TLS 1.2, for servers that fail on 1.3.",
            options.force_tls12,
            TlsOption::ForceTls12,
        ),
        Space::new().height(6),
        tls_toggle(
            "Force TLS 1.3",
            "Pin the handshake to TLS 1.3. Clears the 1.2 pin.",
            options.force_tls13,
            TlsOption::ForceTls13,
        ),
        Space::new().height(4),
        text("Applies to the next request; existing connections are dropped.")
            .size(9)
            .color(Palette::text_subtle()),
    ]
    .spacing(0)
    .into()
}

fn tls_toggle(
    label: &'static str,
    help: &'static str,
    enabled: bool,
    option: crate::message::TlsOption,
) -> Element<'static, Message> {
    column![
        iced::widget::checkbox(enabled)
            .label(label)
            .on_toggle(move |_| Message::Settings(SettingsMsg::TlsOptionToggled(option)))
            .size(15)
            .text_size(12)
            .style(crate::ui::styles::checkbox),
        text(help).size(9).color(Palette::text_subtle()),
    ]
    .spacing(2)
    .into()
}

fn build_global_scripts(
    pre_request_len: usize,
    test_len: usize,
    content_width: f32,
) -> Element<'static, Message> {
    let status = match (pre_request_len > 0, test_len > 0) {
        (false, false) => "Not set up yet".to_owned(),
        (true, false) => "Pre-request configured".to_owned(),
        (false, true) => "Test script configured".to_owned(),
        (true, true) => "All configured".to_owned(),
    };

    column![
        section_label("GLOBAL SCRIPTS"),
        text("Runs before every request's own pre-request/test script — for setup you'd otherwise copy-paste into each request.")
            .size(9).color(Palette::text_subtle()),
        Space::new().height(8),
        row![
            tree_label(status, 11.0, content_width - 100.0),
            Space::new().width(Length::Fill),
            button(text("Edit").size(11).color(Palette::text()))
                .on_press(Message::Settings(SettingsMsg::OpenGlobalScriptsModal))
                .style(|_t, s| {
                    let hov = matches!(s, iced::widget::button::Status::Hovered);
                    iced::widget::button::Style {
                        background: Some(Background::Color(if hov {
                            Palette::surface_raised()
                        } else {
                            Palette::surface_high()
                        })),
                        border: Border { color: Palette::border(), width: 1.0, radius: 6.0.into() },
                        text_color: Palette::text(),
                        ..Default::default()
                    }
                })
                .padding([5, 12]),
        ]
        .align_y(iced::Alignment::Center),
    ]
    .spacing(0)
    .into()
}

// ── Sections ──────────────────────────────────────────────────────────────────

fn build_header() -> Element<'static, Message> {
    let logo_handle = svg::Handle::from_memory(LOGO_SVG);
    container(
        row![
            svg(logo_handle).width(36).height(36),
            column![
                row![
                    text("Rustman")
                        .size(13)
                        .color(Palette::text())
                        .font(crate::ui::theme::MONO),
                    Space::new().width(5),
                    container(
                        text(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .size(9)
                            .color(Palette::text_muted())
                            .font(crate::ui::theme::MONO)
                    )
                    .style(|_| iced::widget::container::Style {
                        background: Some(Background::Color(Palette::surface_raised())),
                        border: Border {
                            color: Palette::border(),
                            width: 1.0,
                            radius: 4.0.into()
                        },
                        ..Default::default()
                    })
                    .padding([2, 5]),
                ]
                .align_y(iced::Alignment::Center),
                text("Native API Testing Client")
                    .size(9)
                    .color(Palette::text_subtle()),
            ]
            .spacing(2),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    )
    .padding(iced::Padding {
        top: 12.0,
        right: 12.0,
        bottom: 6.0,
        left: 12.0,
    })
    .into()
}

fn build_appearance(theme_idx: usize) -> Element<'static, Message> {
    let mut cards = column![].spacing(4);
    for (i, spec) in THEMES.iter().enumerate() {
        let is_selected = i == theme_idx;
        cards = cards.push(theme_card(spec, is_selected, i));
    }

    column![section_label("APPEARANCE"), field_label("Theme"), cards,]
        .spacing(6)
        .into()
}

fn theme_card(
    spec: &'static ThemeSpec,
    is_selected: bool,
    idx: usize,
) -> Element<'static, Message> {
    let preview = row![
        swatch(spec.background),
        swatch(spec.surface),
        swatch(spec.accent)
    ]
    .spacing(3);

    let check: Element<'static, Message> = if is_selected {
        text("\u{2713}").size(TEXT_SM).color(spec.accent).into()
    } else {
        Space::new().width(0).into()
    };

    button(
        row![
            preview,
            text(spec.name).size(TEXT_SM).color(Palette::text()),
            Space::new().width(Length::Fill),
            check,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    )
    .on_press(Message::Settings(SettingsMsg::ThemeChanged(idx)))
    .style(move |_t, s| {
        let hovered = matches!(s, iced::widget::button::Status::Hovered);
        iced::widget::button::Style {
            background: Some(Background::Color(if is_selected {
                Palette::accent_soft()
            } else if hovered {
                Palette::hover()
            } else {
                Palette::surface_high()
            })),
            border: Border {
                color: if is_selected {
                    Palette::accent()
                } else {
                    Palette::border_subtle()
                },
                width: 1.0,
                radius: 6.0.into(),
            },
            text_color: Palette::text(),
            ..Default::default()
        }
    })
    .padding([6, 10])
    .width(Length::Fill)
    .into()
}

fn swatch(color: Color) -> Element<'static, Message> {
    container(Space::new())
        .width(14)
        .height(14)
        .style(move |_| iced::widget::container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                color: Color { a: 0.3, ..color },
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn build_layout(horizontal: bool, content_width: f32) -> Element<'static, Message> {
    let options = [("Top / Bottom", false), ("Left / Right", true)];

    let mut buttons = Vec::new();
    for (label, is_horizontal) in options {
        let selected = horizontal == is_horizontal;
        buttons.push(
            container(
                button(text(label).size(TEXT_SM).color(if selected {
                    Palette::accent()
                } else {
                    Palette::text_muted()
                }))
                .on_press(Message::Settings(SettingsMsg::LayoutDirectionToggled))
                .style(move |_t, s| {
                    use iced::widget::button::Status;
                    iced::widget::button::Style {
                        background: Some(Background::Color(if selected {
                            Palette::accent_soft()
                        } else if matches!(s, Status::Hovered) {
                            Palette::hover()
                        } else {
                            Palette::surface_high()
                        })),
                        border: Border {
                            color: if selected {
                                Palette::accent()
                            } else {
                                Palette::border_subtle()
                            },
                            width: 1.0,
                            radius: 6.0.into(),
                        },
                        text_color: if selected {
                            Palette::accent()
                        } else {
                            Palette::text_muted()
                        },
                        ..Default::default()
                    }
                })
                .padding([6, 10])
                .width(Length::Fill),
            )
            .width(Length::Fill),
        );
    }
    // Side-by-side needs ~95px per button ("Left / Right" ≈ 72px text +
    // padding); below that the labels wrap inside the buttons and the row
    // grows ragged. Stack vertically instead.
    let mut col = column![section_label("LAYOUT")].spacing(6);
    if content_width >= 230.0 {
        let mut row_el = row![].spacing(6);
        for b in buttons {
            row_el = row_el.push(b);
        }
        col = col.push(row_el);
    } else {
        for b in buttons {
            col = col.push(b);
        }
    }
    col.into()
}

fn build_shortcuts(content_width: f32) -> Element<'static, Message> {
    let cmd = if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl+"
    };
    let alt = if cfg!(target_os = "macos") {
        "⌥"
    } else {
        "Alt+"
    };
    let pairs: Vec<(String, &str)> = vec![
        (format!("{cmd}T"), "New tab"),
        (format!("{cmd}W"), "Close tab"),
        (format!("{cmd}S"), "Save"),
        (format!("{cmd}P"), "Palette"),
        (format!("{cmd}F"), "Find"),
        (format!("{cmd}H"), "Find & Replace"),
        (format!("{cmd}E"), "Export cURL"),
        (format!("{cmd}Enter"), "Send"),
        (format!("{alt}1-9"), "Switch tab"),
        ("Esc".to_owned(), "Close dialog"),
        ("↑/↓".to_owned(), "Navigate"),
    ];

    // Two columns need ~150px per half (key chip + description); below that
    // the halves squeeze and descriptions wrap, stretching rows vertically.
    // Stack single-file instead — always fits, no truncation needed.
    let per_row = if content_width >= 330.0 { 2 } else { 1 };
    // Description budget: cell width minus the key chip (~70px worst case)
    // and spacing. Truncation here is graceful (ellipsis + hover), so the
    // constants only need to be close, not exact.
    let desc_avail = if per_row == 2 {
        (content_width - 36.0) / 2.0 - 75.0
    } else {
        content_width - 107.0
    };

    let mut grid = column![section_label("KEYBOARD SHORTCUTS")].spacing(4);
    for chunk in pairs.chunks(per_row) {
        let mut r = row![].spacing(4);
        for (key, desc) in chunk {
            r = r.push(
                container(
                    row![
                        container(
                            text(key.clone())
                                .size(10)
                                .color(Palette::text())
                                .font(crate::ui::theme::MONO),
                        )
                        .style(|_| iced::widget::container::Style {
                            background: Some(Background::Color(Palette::surface_raised())),
                            border: Border {
                                color: Palette::border(),
                                width: 1.0,
                                radius: 4.0.into()
                            },
                            ..Default::default()
                        })
                        .padding([2, 5]),
                        tree_label((*desc).to_owned(), 10.0, desc_avail),
                    ]
                    .spacing(5)
                    .align_y(iced::Alignment::Center),
                )
                .width(Length::FillPortion(1))
                .padding([1, 0]),
            );
        }
        grid = grid.push(r);
    }
    grid.into()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn card(content: impl Into<Element<'static, Message>>) -> Element<'static, Message> {
    card_ref(content)
}

/// Like `card`, but for content that borrows from `AppState` (e.g. a code
/// editor's own `view()`) instead of owning everything it displays.
fn card_ref<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    let bg = Palette::surface();
    let bd = Palette::border_subtle();
    container(content)
        .style(move |_| iced::widget::container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: bd,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        })
        .padding(12)
        .width(Length::Fill)
        .into()
}

fn section_label(label: &'static str) -> Element<'static, Message> {
    container(
        text(label)
            .size(9)
            .color(Palette::text_subtle())
            .font(crate::ui::theme::MONO),
    )
    .padding(iced::Padding {
        top: 0.0,
        right: 0.0,
        bottom: 2.0,
        left: 0.0,
    })
    .into()
}

fn field_label(label: &'static str) -> Element<'static, Message> {
    text(label).size(10).color(Palette::text_subtle()).into()
}
