//! Scripts tab, Postman's layout: the two phases on a rail at the left and
//! one editor beside it, instead of two editors stacked.
use iced::{
    Alignment, Background, Border, Element, Length,
    widget::{Space, button, column, container, pin, row, stack, text},
};

use crate::{
    message::{Message, RequestMsg},
    services::import::postman::SCRIPT_MARKER,
    state::tabs::{RequestTabState, ScriptPhase},
    ui::{
        theme::{Palette, UI_FONT_MEDIUM},
        widgets::popup,
    },
};

const RAIL_WIDTH: f32 = 132.0;

pub fn view(tab: &RequestTabState) -> Element<'_, Message> {
    let phase = tab.script_phase;
    let rail = column![
        phase_button(
            "Before request",
            ScriptPhase::Before,
            phase,
            !tab.pre_request_editor.content().trim().is_empty(),
        ),
        phase_button(
            "After response",
            ScriptPhase::After,
            phase,
            !tab.test_editor.content().trim().is_empty(),
        ),
    ]
    .spacing(2)
    .padding([8, 6])
    .width(RAIL_WIDTH);

    let editor = match phase {
        ScriptPhase::Before => &tab.pre_request_editor,
        ScriptPhase::After => &tab.test_editor,
    };
    let view: Element<Message> = match phase {
        ScriptPhase::Before => editor
            .view()
            .map(|m| Message::Request(RequestMsg::PreRequestScriptEdited(m))),
        ScriptPhase::After => editor
            .view()
            .map(|m| Message::Request(RequestMsg::TestScriptEdited(m))),
    };

    // A caret-sized anchor, as in the body editor: arrows and Enter reach the
    // list before the editor while it is open.
    let caret = editor.cursor_screen_position().unwrap_or_default();
    let (list, selected): (&[_], usize) = tab
        .script_completion
        .as_ref()
        .map_or((&[], 0), |c| (&c.items[..], c.selected));
    let completion = popup::popup_suggest(
        Space::new().width(1).height(editor.line_height()).into(),
        super::headers::suggest_list(list, selected, |apply| {
            Message::Request(RequestMsg::ScriptCompletionPicked(apply))
        }),
        Message::Request(RequestMsg::ScriptCompletionClosed),
        !list.is_empty(),
        popup::SuggestNav {
            up: Message::Request(RequestMsg::ScriptCompletionMoved(false)),
            down: Message::Request(RequestMsg::ScriptCompletionMoved(true)),
            enter: Message::Request(RequestMsg::ScriptCompletionAccepted),
        },
    );
    let anchor = pin(completion)
        .x(caret.x)
        .y((caret.y - editor.viewport_scroll()).max(0.0));

    let mut pane = column![].width(Length::Fill).height(Length::Fill);
    if editor.content().starts_with(SCRIPT_MARKER) {
        pane = pane.push(imported_banner());
    }
    pane = pane.push(
        stack![view, anchor]
            .width(Length::Fill)
            .height(Length::Fill),
    );

    row![
        rail,
        iced::widget::rule::vertical(1).style(crate::ui::styles::divider),
        pane,
    ]
    .height(Length::Fill)
    .into()
}

fn phase_button<'a>(
    label: &'a str,
    phase: ScriptPhase,
    current: ScriptPhase,
    has_script: bool,
) -> Element<'a, Message> {
    let active = phase == current;
    let mut line = row![
        text(label).size(12).font(UI_FONT_MEDIUM).color(if active {
            Palette::text()
        } else {
            Palette::text_muted()
        }),
        Space::new().width(Length::Fill),
    ]
    .align_y(Alignment::Center);
    if has_script {
        line = line.push(crate::ui::icons::dot(Palette::SUCCESS));
    }
    button(line)
        .on_press(Message::Request(RequestMsg::ScriptPhaseSelected(phase)))
        .padding([6, 8])
        .width(Length::Fill)
        .style(move |_, status| button::Style {
            background: (active || matches!(status, button::Status::Hovered))
                .then(|| Background::Color(Palette::surface_high())),
            text_color: Palette::text(),
            border: Border {
                radius: 5.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

/// Imported scripts are kept as comments and never run by themselves; running
/// one is the user's click, not the import's.
fn imported_banner<'a>() -> Element<'a, Message> {
    container(
        row![
            text("Imported from Postman, kept as comments and not running")
                .size(12)
                .color(Palette::text_muted()),
            Space::new().width(Length::Fill),
            button(text("Enable script").size(12))
                .on_press(Message::Request(RequestMsg::ScriptImportEnabled))
                .padding([4, 10])
                .style(iced::widget::button::secondary),
        ]
        .align_y(Alignment::Center),
    )
    .padding([6, 10])
    .width(Length::Fill)
    .style(crate::ui::styles::section_header)
    .into()
}
