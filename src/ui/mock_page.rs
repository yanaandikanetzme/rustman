//! The mock page in the main area: a mock's detail (URL, facts, call log,
//! routes) or the create/edit form. Composition only; looks come from the
//! shared atoms and theme.
use iced::{
    Alignment, Background, Border, Color, Element, Length, Padding,
    widget::{
        Space, button, checkbox, column, container, pick_list, row, scrollable, text, text_input,
    },
};

use crate::{
    app::AppState,
    domain::mock::{MOCK_URL_VAR, MockServer},
    message::{Message, mock::MockMsg},
    services::mock::{CallLog, Route, build_routes},
    state::mocks::{DetailTab, LogFilter, MockForm, MockPage, PortNote, RunStatus},
    ui::{
        components::{
            atoms::{Button, chip, text as t},
            molecules::DataTable,
            tone::Tone,
        },
        icons,
        theme::{MONO, Palette, TEXT_MD, TEXT_SM, TEXT_XS, UI_FONT_MEDIUM},
    },
};

pub fn view<'a>(state: &'a AppState, page: &'a MockPage) -> Element<'a, Message> {
    let body = match page {
        MockPage::Detail(id) => match state.mocks.get(id) {
            Some(mock) => detail(state, mock),
            None => t::muted("This mock server no longer exists."),
        },
        MockPage::Form(form) => form_view(state, form),
    };
    container(scrollable(container(body).padding([18, 22])).style(crate::ui::theme::thin_scrollbar))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(crate::ui::styles::content_surface)
        .into()
}

fn mock(msg: MockMsg) -> Message {
    Message::Mock(msg)
}

/// The server tile, the `MOCK SERVERS` crumb and the page title.
fn title<'a>(name: &str, badge: Option<Element<'a, Message>>) -> Element<'a, Message> {
    let tile = container(icons::server().size(18).color(Palette::accent()))
        .center_x(36)
        .center_y(36)
        .style(|_| container::Style {
            background: Some(Background::Color(Palette::accent_soft())),
            border: Border {
                radius: 8.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });
    let mut name_row = row![
        text(name.to_owned())
            .size(18)
            .font(UI_FONT_MEDIUM)
            .color(Palette::text())
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    if let Some(badge) = badge {
        name_row = name_row.push(badge);
    }
    row![
        tile,
        column![
            text("MOCK SERVERS").size(TEXT_XS).color(Palette::text_muted()),
            name_row
        ]
        .spacing(2)
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .into()
}

fn pill<'a>(label: &str, tone: Tone) -> Element<'a, Message> {
    container(
        row![
            icons::dot(tone.color()),
            text(label.to_owned())
                .size(TEXT_SM)
                .font(UI_FONT_MEDIUM)
                .line_height(1.0)
                .color(tone.color())
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    // A fixed height with the text box as tall as its glyphs centres the
    // label; the default line height left more room under it than above.
    .height(22)
    .center_y(22)
    .padding([0, 9])
    .style(move |_| container::Style {
        background: Some(Background::Color(tone.tint(0.16))),
        border: Border {
            radius: 999.0.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

fn framed<'a>(content: impl Into<Element<'a, Message>>) -> container::Container<'a, Message> {
    container(content).style(|_| container::Style {
        border: Border {
            color: Palette::border_subtle(),
            width: 1.0,
            radius: 7.0.into(),
        },
        ..Default::default()
    })
}

fn icon_label<'a>(icon: text::Text<'static>, label: &str, msg: Message) -> Element<'a, Message> {
    button(
        container(
            row![
                icon.size(13).color(Palette::text_muted()),
                text(label.to_owned()).size(TEXT_SM).color(Palette::text())
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .center_y(Length::Fill),
    )
    .on_press(msg)
    .padding([0, 14])
    .height(Length::Fill)
    .style(|_, s| button::Style {
        background: matches!(s, button::Status::Hovered)
            .then(|| Background::Color(Palette::hover())),
        ..Default::default()
    })
    .into()
}

fn method_color(method: &str) -> Color {
    match method {
        "GET" => Palette::GET,
        "POST" => Palette::POST,
        "PUT" => Palette::PUT,
        "PATCH" => Palette::PATCH,
        "DELETE" => Palette::DELETE,
        _ => Palette::HEAD,
    }
}

fn method<'a>(m: &str) -> Element<'a, Message> {
    text(m.to_owned())
        .size(TEXT_XS)
        .font(MONO)
        .color(method_color(m))
        .into()
}

fn status_code<'a>(status: u16) -> Element<'a, Message> {
    let tone = match status {
        200..=399 => Tone::Success,
        400..=499 => Tone::Warning,
        _ => Tone::Danger,
    };
    container(
        text(status.to_string())
            .size(TEXT_XS)
            .font(MONO)
            .line_height(1.0)
            .color(tone.color()),
    )
    .height(18)
    .center_y(18)
    .padding([0, 7])
        .style(move |_| container::Style {
            background: Some(Background::Color(tone.tint(0.16))),
            border: Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

fn mono<'a>(s: String, color: Color) -> Element<'a, Message> {
    text(s)
        .size(TEXT_SM)
        .font(MONO)
        .color(color)
        .wrapping(text::Wrapping::None)
        .into()
}

// ── Detail ───────────────────────────────────────────────────────────────────

fn detail<'a>(state: &'a AppState, m: &'a MockServer) -> Element<'a, Message> {
    let status = state.mocks.status(&m.id);
    let live = status.is_some_and(RunStatus::is_live);
    let badge = match status {
        Some(RunStatus::Running) => pill("Running", Tone::Success),
        Some(RunStatus::Starting) => pill("Starting", Tone::Warning),
        Some(RunStatus::Failed(_)) => pill("Failed", Tone::Danger),
        None => pill("Stopped", Tone::Subtle),
    };
    let run_button = if live {
        Button::new("Stop").on_press(mock(MockMsg::Stop(m.id.clone())))
    } else {
        Button::new("Start").primary().on_press(mock(MockMsg::Start(m.id.clone())))
    };
    let header = row![
        title(&m.name, Some(badge)),
        Space::new().width(Length::Fill),
        Button::new("Edit")
            .ghost()
            .on_press(mock(MockMsg::EditOpened(m.id.clone()))),
        Button::new("Delete")
            .ghost()
            .on_press(mock(MockMsg::DeleteAsked(m.id.clone()))),
        run_button,
        Button::new("Close").ghost().on_press(mock(MockMsg::PageClosed)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let mut page = column![header].spacing(16);

    if state.mocks.delete_asked.as_deref() == Some(m.id.as_str()) {
        page = page.push(
            crate::ui::components::molecules::Banner::new(
                Tone::Danger,
                crate::ui::components::atoms::Mark::Icon(icons::alert()),
                format!("Delete \"{}\"?", m.name),
            )
            .detail("The mock stops and its call log is removed. The collection is not touched.")
            .action(Button::new("Cancel").ghost().on_press(mock(MockMsg::DeleteCancelled)))
            .action(Button::new("Delete").primary().on_press(mock(MockMsg::DeleteConfirmed))),
        );
    }
    if let Some(RunStatus::Failed(e)) = status {
        page = page.push(
            crate::ui::components::molecules::Banner::new(
                Tone::Danger,
                crate::ui::components::atoms::Mark::Icon(icons::alert()),
                "The mock server could not start",
            )
            .detail(format!("{e}. Pick another port with Edit, or stop the program using it.")),
        );
    }

    // URL bar: what a client calls.
    let env = m
        .environment_id
        .as_ref()
        .and_then(|id| state.environments.iter().find(|e| &e.id == id));
    let url = m.url();
    let saved_as = env
        .filter(|e| e.variables.get(MOCK_URL_VAR) == Some(&url))
        .map(|e| format!("Saved as {{{{{MOCK_URL_VAR}}}}} in {}", e.name));
    let url_bar = framed(
        row![
            container(
                row![
                    icons::dot(crate::ui::sidebar::mock_status_color(status)),
                    text("MOCK URL")
                        .size(TEXT_XS)
                        .font(MONO)
                        .color(Palette::text_muted())
                ]
                .spacing(7)
                .align_y(Alignment::Center)
            )
            .width(115)
            .height(Length::Fill)
            .padding([0, 14])
            .center_y(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Palette::surface_high())),
                ..Default::default()
            }),
            container(mono(url, Palette::text()))
                .padding([0, 14])
                .center_y(Length::Fill),
            Space::new().width(Length::Fill),
            container(text(saved_as.unwrap_or_default()).size(TEXT_SM).color(Palette::text_subtle()))
                .padding([0, 14])
                .center_y(Length::Fill),
            icon_label(icons::copy(), "Copy", mock(MockMsg::CopyUrl(m.id.clone()))),
            icon_label(icons::plus(), "New request", mock(MockMsg::NewRequest(m.id.clone()))),
        ]
        .height(38)
        .align_y(Alignment::Center),
    );
    page = page.push(url_bar);

    // Facts.
    let collection = state.collections.iter().find(|c| c.id == m.collection_id);
    let requests = state
        .requests
        .get(&m.collection_id)
        .map_or(0, Vec::len);
    let examples: usize = state
        .requests
        .get(&m.collection_id)
        .map(|rs| {
            rs.iter()
                .map(|r| state.examples.items.get(&r.id).map_or(0, Vec::len))
                .sum()
        })
        .unwrap_or(0);
    let (calls, unmatched) = state.mocks.stats.get(&m.id).copied().unwrap_or((0, 0));
    let fact = |k: &str, v: String, s: String| -> Element<'a, Message> {
        container(
            column![
                text(k.to_owned()).size(TEXT_XS).color(Palette::text_muted()),
                text(v).size(TEXT_MD).font(UI_FONT_MEDIUM).color(Palette::text()),
                text(s).size(TEXT_XS).color(Palette::text_subtle()),
            ]
            .spacing(3),
        )
        .padding([10, 14])
        .width(Length::FillPortion(1))
        .into()
    };
    let facts = framed(
        row![
            fact(
                "Collection",
                collection.map_or("Deleted".into(), |c| c.name.clone()),
                format!("{requests} requests · {examples} examples"),
            ),
            fact(
                "Environment",
                env.map_or("None".into(), |e| e.name.clone()),
                if env.is_some() {
                    "Resolves {{vars}} in examples".into()
                } else {
                    "Examples are served as saved".into()
                },
            ),
            fact(
                "Response delay",
                if m.delay_ms > 0 {
                    format!("{} ms", m.delay_ms)
                } else {
                    "None".into()
                },
                if m.delay_ms > 0 {
                    "Waits before each response".into()
                } else {
                    "Answers immediately".into()
                },
            ),
            fact(
                "Calls",
                calls.to_string(),
                if unmatched > 0 {
                    format!("{unmatched} unmatched since start")
                } else {
                    "Since start".into()
                },
            ),
        ]
        .width(Length::Fill),
    );
    page = page.push(facts);

    // Sub-tabs.
    let log = state.mocks.logs.get(&m.id);
    let routes = crate::app::mocks::routes_for(state, &m.id);
    let tab = state.mocks.detail_tab;
    let tabs = row![
        sub_tab("Call log", log.map_or(0, |l| l.len()), tab == DetailTab::Calls, DetailTab::Calls),
        sub_tab("Routes", routes.len(), tab == DetailTab::Routes, DetailTab::Routes),
    ]
    .spacing(18);
    page = page.push(column![tabs, iced::widget::rule::horizontal(1).style(crate::ui::styles::divider)].spacing(0));

    page = match tab {
        DetailTab::Calls => page.push(call_log(state, m, log.map(|l| l.iter()))),
        DetailTab::Routes => page.push(routes_table(routes)),
    };
    page.into()
}

fn sub_tab<'a>(label: &str, count: usize, on: bool, tab: DetailTab) -> Element<'a, Message> {
    let fg = if on { Palette::text() } else { Palette::text_muted() };
    let body = column![
        row![
            text(label.to_owned())
                .size(TEXT_SM)
                .font(if on { UI_FONT_MEDIUM } else { crate::ui::theme::UI_FONT })
                .color(fg),
            text(count.to_string()).size(TEXT_XS).font(MONO).color(Palette::text_subtle()),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
        container(Space::new().height(2))
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: on.then(|| Background::Color(Palette::accent())),
                ..Default::default()
            }),
    ]
    .spacing(8)
    .width(Length::Shrink);
    button(body)
        .padding(0)
        .on_press(mock(MockMsg::DetailTabSelected(tab)))
        .style(|_, _| button::Style::default())
        .into()
}

fn unmatched(call: &CallLog) -> bool {
    call.example.is_none() && call.status >= 400
}

fn call_log<'a>(
    state: &'a AppState,
    m: &'a MockServer,
    log: Option<impl Iterator<Item = &'a CallLog>>,
) -> Element<'a, Message> {
    let filter = state.mocks.log_filter;
    let toolbar = row![
        chip("All", filter == LogFilter::All, mock(MockMsg::LogFilterSelected(LogFilter::All))),
        chip(
            "Matched",
            filter == LogFilter::Matched,
            mock(MockMsg::LogFilterSelected(LogFilter::Matched))
        ),
        chip(
            "Unmatched",
            filter == LogFilter::Unmatched,
            mock(MockMsg::LogFilterSelected(LogFilter::Unmatched))
        ),
        text_input("Filter path", &state.mocks.path_filter)
            .on_input(|v| mock(MockMsg::PathFilterChanged(v)))
            .size(TEXT_SM)
            .padding([4, 8])
            .width(240)
            .style(crate::ui::styles::boxed_input),
        Space::new().width(Length::Fill),
        Button::new("Clear log")
            .ghost()
            .on_press(mock(MockMsg::ClearLog(m.id.clone()))),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let needle = state.mocks.path_filter.trim().to_lowercase();
    let mut table = DataTable::spacious([
        ("Time", Length::Fixed(84.0)),
        ("Method", Length::Fixed(64.0)),
        ("Path", Length::FillPortion(3)),
        ("Answered with", Length::FillPortion(3)),
        ("Status", Length::Fixed(60.0)),
        ("Duration", Length::Fixed(64.0)),
    ]);
    let mut shown = 0;
    for call in log.into_iter().flatten() {
        let keep = match filter {
            LogFilter::All => true,
            LogFilter::Matched => !unmatched(call),
            LogFilter::Unmatched => unmatched(call),
        } && (needle.is_empty() || call.path.to_lowercase().contains(&needle));
        if !keep {
            continue;
        }
        shown += 1;
        let time = chrono::DateTime::from_timestamp_millis(call.at_ms)
            .map(|d| d.with_timezone(&chrono::Local).format("%H:%M:%S").to_string())
            .unwrap_or_default();
        let answered: Element<Message> = match (&call.example, &call.note) {
            (Some(name), _) => row![
                icons::example().size(12).color(Palette::text_subtle()),
                text(name.clone()).size(TEXT_SM).color(Palette::text())
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .into(),
            (None, Some(note)) => row![
                icons::alert().size(12).color(Tone::Warning.color()),
                text(note.clone()).size(TEXT_SM).color(if call.status < 400 {
                    Palette::text_muted()
                } else {
                    Tone::Warning.color()
                })
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .into(),
            (None, None) => t::meta("—"),
        };
        table = table.row(vec![
            mono(time, Palette::text_muted()),
            method(&call.method),
            mono(call.path.clone(), Palette::text()),
            answered,
            status_code(call.status),
            mono(format!("{} ms", call.duration_ms), Palette::text_muted()),
        ]);
        if shown >= 300 {
            break;
        }
    }
    let body: Element<Message> = if shown == 0 {
        container(t::muted(if state.mocks.is_live(&m.id) {
            format!("No calls yet. Send a request to {} to see it here.", m.url())
        } else {
            "No calls. Start the mock server, then send requests to its URL.".into()
        }))
        .padding([16, 14])
        .into()
    } else {
        table.into()
    };
    column![toolbar, body].spacing(12).into()
}

fn routes_table<'a>(routes: Vec<Route>) -> Element<'a, Message> {
    if routes.is_empty() {
        return container(t::muted("The collection has no requests to mock."))
            .padding([16, 14])
            .into();
    }
    let mut table = DataTable::spacious([
        ("Method", Length::Fixed(64.0)),
        ("Path", Length::FillPortion(3)),
        ("Request", Length::FillPortion(2)),
        ("Answers with", Length::FillPortion(3)),
    ]);
    for r in routes {
        let answers: Element<Message> = if r.examples.is_empty() {
            row![
                icons::alert().size(12).color(Tone::Warning.color()),
                text("No example · 404").size(TEXT_SM).color(Tone::Warning.color())
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .into()
        } else {
            let codes: Vec<String> = r.examples.iter().map(|e| e.status.to_string()).collect();
            let label = if r.examples.len() == 1 {
                format!("{} · {}", r.examples[0].name, codes[0])
            } else {
                format!("{} examples · {}", r.examples.len(), codes.join(", "))
            };
            text(label).size(TEXT_SM).color(Palette::text_muted()).into()
        };
        table = table.row(vec![
            method(&r.method),
            mono(r.path.clone(), Palette::text()),
            text(r.request_name.clone()).size(TEXT_SM).color(Palette::text()).into(),
            answers,
        ]);
    }
    column![
        table,
        t::meta(
            "Calls match on method and path. With several examples the first success answers; \
             send x-mock-response-name or x-mock-response-code to pick another."
        )
    ]
    .spacing(10)
    .into()
}

// ── Form ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
struct Choice {
    id: Option<String>,
    label: String,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

fn field<'a>(label: &str, input: impl Into<Element<'a, Message>>, help: Option<Element<'a, Message>>) -> Element<'a, Message> {
    let mut col = column![
        text(label.to_owned())
            .size(TEXT_SM)
            .font(UI_FONT_MEDIUM)
            .color(Palette::text_muted()),
        input.into()
    ]
    .spacing(6);
    if let Some(help) = help {
        col = col.push(help);
    }
    col.into()
}

fn check<'a>(
    on: bool,
    label: &str,
    help: &str,
    msg: Option<fn(bool) -> MockMsg>,
) -> Element<'a, Message> {
    let mut boxed = checkbox(on).size(14).style(crate::ui::styles::checkbox);
    if let Some(msg) = msg {
        boxed = boxed.on_toggle(move |v| mock(msg(v)));
    }
    row![
        boxed,
        column![
            text(label.to_owned()).size(TEXT_SM).font(UI_FONT_MEDIUM).color(if msg.is_some() {
                Palette::text()
            } else {
                Palette::text_subtle()
            }),
            text(help.to_owned()).size(TEXT_XS).color(Palette::text_subtle()),
        ]
        .spacing(2)
    ]
    .spacing(10)
    .into()
}

fn card<'a>(title: &str, sub: String, body: Element<'a, Message>) -> Element<'a, Message> {
    framed(
        column![
            container(
                row![
                    text(title.to_owned()).size(TEXT_MD).font(UI_FONT_MEDIUM).color(Palette::text()),
                    text(sub).size(TEXT_SM).color(Palette::text_muted()),
                ]
                .spacing(10)
                .align_y(Alignment::Center)
            )
            .padding([12, 16]),
            iced::widget::rule::horizontal(1).style(crate::ui::styles::divider),
            body,
        ]
        .spacing(0),
    )
    .width(Length::FillPortion(1))
    .into()
}

fn form_view<'a>(state: &'a AppState, f: &'a MockForm) -> Element<'a, Message> {
    let editing = f.editing.is_some();
    let header = row![
        title(if editing { "Edit mock server" } else { "New mock server" }, None),
        Space::new().width(Length::Fill),
        Button::new("Close").ghost().on_press(mock(MockMsg::PageClosed)),
    ]
    .align_y(Alignment::Center);

    let collections: Vec<Choice> = state
        .collections
        .iter()
        .map(|c| Choice {
            id: Some(c.id.clone()),
            label: c.name.clone(),
        })
        .collect();
    let picked_collection = collections
        .iter()
        .find(|c| c.id == f.collection_id)
        .cloned();
    let mut environments = vec![Choice {
        id: None,
        label: "No environment".into(),
    }];
    environments.extend(state.environments.iter().map(|e| Choice {
        id: Some(e.id.clone()),
        label: e.name.clone(),
    }));
    let picked_env = environments
        .iter()
        .find(|c| c.id == f.environment_id)
        .cloned();

    let port_note: Element<Message> = match &f.port_note {
        Some(PortNote::Free) => text("Port is free").size(TEXT_SM).color(Tone::Success.color()).into(),
        Some(PortNote::Taken(Some(name))) => text(format!("Used by \"{name}\""))
            .size(TEXT_SM)
            .color(Tone::Danger.color())
            .into(),
        Some(PortNote::Taken(None)) => text("In use by another program")
            .size(TEXT_SM)
            .color(Tone::Danger.color())
            .into(),
        Some(PortNote::Invalid) => text("Use 1024–65535")
            .size(TEXT_SM)
            .color(Tone::Warning.color())
            .into(),
        None => Space::new().into(),
    };
    let port_input = row![
        text("127.0.0.1:").size(TEXT_SM).font(MONO).color(Palette::text_subtle()),
        text_input("4010", &f.port)
            .on_input(|v| mock(MockMsg::FormPort(v)))
            .size(TEXT_SM)
            .font(MONO)
            .padding([6, 8])
            .width(Length::Fill)
            .style(crate::ui::styles::boxed_input),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let mut delay_row = column![check(
        f.delay_on,
        "Simulate a fixed delay",
        "Wait before each response.",
        Some(MockMsg::FormDelayToggled)
    )]
    .spacing(8);
    if f.delay_on {
        delay_row = delay_row.push(
            row![
                Space::new().width(24),
                text_input("300", &f.delay)
                    .on_input(|v| mock(MockMsg::FormDelay(v)))
                    .size(TEXT_SM)
                    .font(MONO)
                    .padding([6, 8])
                    .width(90)
                    .style(crate::ui::styles::boxed_input),
                text("ms").size(TEXT_SM).color(Palette::text_muted()),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    }

    let env_name = picked_env
        .as_ref()
        .filter(|c| c.id.is_some())
        .map(|c| c.label.clone());
    let save_var_help = match &env_name {
        Some(name) => format!("Adds the variable to {name}, so requests can switch to the mock."),
        None => "Pick an environment to store the URL in.".into(),
    };
    let configure = column![
        field(
            "Name",
            text_input("Merchant API mock", &f.name)
                .on_input(|v| mock(MockMsg::FormName(v)))
                .on_submit(mock(MockMsg::FormSubmitted))
                .size(TEXT_MD)
                .padding([7, 10])
                .style(crate::ui::styles::boxed_input),
            None,
        ),
        field(
            "Collection to mock",
            pick_list(collections, picked_collection, |c: Choice| {
                mock(MockMsg::FormCollection(c.id.unwrap_or_default()))
            })
            .placeholder("Pick a collection")
            .text_size(TEXT_MD)
            .padding([7, 10])
            .width(Length::Fill)
            .style(crate::ui::styles::boxed_pick_list),
            Some(t::meta("Every saved example becomes a response.")),
        ),
        row![
            container(field("Port", port_input, Some(port_note))).width(Length::FillPortion(1)),
            container(field(
                "Environment",
                pick_list(environments, picked_env, |c: Choice| {
                    mock(MockMsg::FormEnvironment(c.id))
                })
                .text_size(TEXT_MD)
                .padding([7, 10])
                .width(Length::Fill)
                .style(crate::ui::styles::boxed_pick_list),
                None,
            ))
            .width(Length::FillPortion(1)),
        ]
        .spacing(12),
        check(
            f.save_var && env_name.is_some(),
            "Save the mock URL as {{mock_url}}",
            &save_var_help,
            env_name.is_some().then_some(MockMsg::FormSaveVarToggled as fn(bool) -> MockMsg),
        ),
        delay_row,
        check(
            f.auto_start,
            "Start when Rustman opens",
            "Otherwise start it from the sidebar.",
            Some(MockMsg::FormAutoStartToggled)
        ),
    ]
    .spacing(14)
    .padding([14, 16]);

    // What the mock will answer, from the picked collection.
    let env = f
        .environment_id
        .as_ref()
        .and_then(|id| state.environments.iter().find(|e| &e.id == id));
    let routes = f
        .collection_id
        .as_ref()
        .and_then(|id| state.requests.get(id))
        .map(|rs| build_routes(rs, &state.examples.items, env))
        .unwrap_or_default();
    let collection_name = picked_collection_name(state, f);
    let mut preview = column![].spacing(0);
    for r in routes.iter().take(40) {
        let answers = if r.examples.is_empty() {
            text("No example · 404").size(TEXT_SM).color(Tone::Warning.color())
        } else if r.examples.len() == 1 {
            text(format!("{} · {}", r.examples[0].name, r.examples[0].status))
                .size(TEXT_SM)
                .color(Palette::text_muted())
        } else {
            let codes: Vec<String> = r.examples.iter().map(|e| e.status.to_string()).collect();
            text(format!("{} examples · {}", r.examples.len(), codes.join(", ")))
                .size(TEXT_SM)
                .color(Palette::text_muted())
        };
        preview = preview.push(
            column![
                container(
                    row![
                        container(method(&r.method)).width(56),
                        container(mono(r.path.clone(), Palette::text())).width(Length::Fill),
                        answers,
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center)
                )
                .padding([7, 16]),
                iced::widget::rule::horizontal(1).style(crate::ui::styles::divider),
            ]
            .spacing(0),
        );
    }
    if routes.len() > 40 {
        preview = preview.push(container(t::meta(format!("and {} more", routes.len() - 40))).padding([8, 16]));
    }
    if routes.is_empty() {
        preview = preview.push(
            container(t::muted(if f.collection_id.is_some() {
                "This collection has no requests yet."
            } else {
                "Pick a collection to see what the mock answers."
            }))
            .padding([12, 16]),
        );
    }
    preview = preview.push(
        container(t::meta(
            "Calls match on method and path. With several examples the first success \
             answers; send x-mock-response-name or x-mock-response-code to pick another.",
        ))
        .padding([10, 16]),
    );

    let cards = row![
        card("Configure", "Pick what to serve and where".into(), configure.into()),
        card(
            "What it will answer",
            match collection_name {
                Some(name) => format!("{} routes from {name}", routes.len()),
                None => String::new(),
            },
            preview.into()
        ),
    ]
    .spacing(22);

    let mut actions = row![
        Button::new(if editing { "Save" } else { "Create and start" })
            .primary()
            .on_press(mock(MockMsg::FormSubmitted)),
        Button::new("Cancel").ghost().on_press(mock(MockMsg::PageClosed)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    actions = match &f.error {
        Some(e) => actions.push(text(e.clone()).size(TEXT_SM).color(Tone::Danger.color())),
        None => actions.push(t::meta(
            "You can stop, edit or delete it from the Mock Servers sidebar.",
        )),
    };

    column![
        header,
        cards,
        container(actions).padding(Padding {
            top: 4.0,
            ..Padding::ZERO
        })
    ]
    .spacing(18)
    .into()
}

fn picked_collection_name(state: &AppState, f: &MockForm) -> Option<String> {
    let id = f.collection_id.as_ref()?;
    state
        .collections
        .iter()
        .find(|c| &c.id == id)
        .map(|c| c.name.clone())
}
