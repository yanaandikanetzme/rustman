//! The card a `{{variable}}` opens in the Body editor and the Headers table:
//! its value in a selectable field, masked behind an eye for secrets, with
//! Copy and where it comes from.
use iced::{
    Alignment, Background, Border, Element, Length,
    widget::{column, container, mouse_area, row, text, text_input},
};

use crate::{
    domain::environment::AppEnvironment,
    message::{Message, RequestMsg},
    services::{
        format::head_capped,
        header_suggest::{DYNAMIC_VARIABLES, Source},
        variables,
    },
    state::tabs::VarCard,
    ui::{
        icons,
        theme::{MONO, Palette},
        widgets::{click_probe::click_probe, mini},
    },
};

/// Longest value put in the field; Copy still takes the whole value.
const VALUE_MAX_BYTES: usize = 4096;
/// A cell can hold many variables; the card stays a card.
const MAX_NAMES: usize = 4;

fn msg(m: RequestMsg) -> Message {
    Message::Request(m)
}

fn frame() -> Border {
    Border {
        color: Palette::border(),
        width: 1.0,
        radius: 6.0.into(),
    }
}

fn boxed<'a>(shown: String, color: iced::Color) -> Element<'a, Message> {
    container(text(shown).size(13).font(MONO).color(color))
        .padding([7, 10])
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(Palette::background())),
            border: frame(),
            ..Default::default()
        })
        .into()
}

fn source_row<'a>(source: Source, label: String) -> Element<'a, Message> {
    row![
        super::headers::source_badge(source),
        text(label).size(12).color(Palette::text_muted()),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn variable<'a>(
    name: &'a str,
    card: &'a VarCard,
    env: Option<&'a AppEnvironment>,
) -> Element<'a, Message> {
    let mut block = column![
        text(head_capped(name, 128).to_owned())
            .size(12)
            .font(MONO)
            .color(Palette::text_muted()),
    ]
    .spacing(8);
    let value = env.and_then(|e| e.variables.get(name));
    let dynamic = DYNAMIC_VARIABLES.iter().find(|(n, _)| *n == name);
    match (value, dynamic) {
        (Some(value), _) => {
            let secret = card.mask_all || variables::sensitive(name);
            let field: Element<'a, Message> = if secret && !card.revealed {
                boxed(variables::MASK.to_owned(), Palette::text_muted())
            } else {
                // A live input whose edits go nowhere: only an input offers
                // mouse selection and Ctrl+C.
                text_input("(empty)", head_capped(value, VALUE_MAX_BYTES))
                    .on_input(|_| msg(RequestMsg::VarCardPinned))
                    .size(13)
                    .font(MONO)
                    .padding([7, 10])
                    .width(Length::Fill)
                    .style(|_, _| text_input::Style {
                        background: Background::Color(Palette::background()),
                        border: frame(),
                        icon: Palette::text_muted(),
                        placeholder: Palette::text_subtle(),
                        value: Palette::text(),
                        selection: iced::Color {
                            a: 0.35,
                            ..Palette::accent()
                        },
                    })
                    .into()
            };
            let mut line = row![field].spacing(4).align_y(Alignment::Center);
            if secret {
                line = line.push(mini::icon_button(
                    if card.revealed {
                        icons::eye_closed()
                    } else {
                        icons::eye()
                    },
                    if card.revealed {
                        "Hide value"
                    } else {
                        "Show value"
                    },
                    Some(msg(RequestMsg::VarCardRevealToggled)),
                    card.revealed,
                ));
            }
            line = line.push(mini::icon_button(
                if card.copied.as_deref() == Some(name) {
                    icons::check()
                } else {
                    icons::copy()
                },
                "Copy value",
                Some(msg(RequestMsg::VarCardCopied(name.to_owned()))),
                false,
            ));
            block = block.push(line);
            if value.len() > VALUE_MAX_BYTES && (!secret || card.revealed) {
                block = block.push(
                    text("Showing the first 4 KB · Copy takes the full value")
                        .size(11)
                        .color(Palette::text_muted()),
                );
            }
            block = block.push(source_row(
                Source::Environment,
                env.map_or(String::new(), |e| e.name.clone()),
            ));
        }
        (None, Some((_, description))) => {
            block = block.push(boxed((*description).to_owned(), Palette::text_muted()));
            block = block.push(source_row(
                Source::Dynamic,
                "Dynamic variable · new value on every send".to_owned(),
            ));
        }
        (None, None) => {
            block = block.push(boxed(
                if env.is_some() {
                    "Unresolved variable".to_owned()
                } else {
                    "Unresolved variable · no active environment".to_owned()
                },
                Palette::ERROR,
            ));
        }
    }
    block.into()
}

pub(crate) fn view<'a>(card: &'a VarCard, env: Option<&'a AppEnvironment>) -> Element<'a, Message> {
    let mut content = column![].spacing(12);
    for name in card.names.iter().take(MAX_NAMES) {
        content = content.push(variable(name, card, env));
    }
    let body = container(content)
        .padding(10)
        .width(340)
        .style(|_| container::Style {
            background: Some(Background::Color(Palette::surface_high())),
            border: Border {
                color: Palette::border(),
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        });
    click_probe(
        mouse_area(body)
            .on_enter(msg(RequestMsg::VarCardHover(true)))
            .on_exit(msg(RequestMsg::VarCardHover(false))),
        msg(RequestMsg::VarCardPinned),
    )
    .into()
}
