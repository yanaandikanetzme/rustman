use iced::{
    Alignment, Background, Border, Color, Element, Length,
    widget::{Space, button, column, container, row, scrollable, text},
};

use crate::{
    domain::response::HttpResponse,
    message::{Message, ResponseMsg},
    services::spreadsheet::ParsedSheet,
    state::tabs::{BodyLang, RequestTabState, ResponsePreview},
    ui::{
        icons,
        theme::{MONO, Palette, TEXT_LG, TEXT_SM, TEXT_XS},
        widgets::mini,
    },
};

/// Context-menu key of the body format menu (shared app-wide slot).
pub const FORMAT_MENU: &str = "response:format";

/// Postman's format button: a filled `{ } JSON ⌄` chip whose menu lists
/// each format with its icon and a check on the current one; Raw sits
/// under a divider.
/// `active` is false while Preview is on: the chip then looks unselected, so
/// only one of the two reads as the current view (Postman's behaviour).
/// Like Postman: from Preview the chip first returns to the source; the
/// dropdown opens only once the source is showing.
fn format_chip_press(active: bool) -> Message {
    if active {
        Message::Sidebar(crate::message::SidebarMsg::ContextMenuOpened(
            FORMAT_MENU.to_owned(),
        ))
    } else {
        Message::Response(ResponseMsg::PreviewToggled)
    }
}

fn format_menu(current: BodyLang, open: bool, active: bool) -> Element<'static, Message> {
    let fg = if active {
        Palette::text()
    } else {
        Palette::text_muted()
    };
    use crate::ui::components::molecules::menu::{menu_divider, menu_item};
    let icon = |lang: BodyLang| match lang {
        BodyLang::Json => icons::braces(),
        BodyLang::Xml | BodyLang::Html => icons::code(),
        BodyLang::JavaScript => icons::file_code(),
        BodyLang::Text => icons::letter_text(),
        BodyLang::Raw => icons::file_type(),
    };
    let chip = button(
        container(
            row![
                icon(current).size(12).color(fg),
                text(current.to_string())
                    .size(12)
                    .font(crate::ui::theme::UI_FONT_MEDIUM)
                    .color(fg),
                icons::chevron_down().size(11).color(Palette::text_muted()),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .height(26)
        .center_y(26),
    )
    .on_press(format_chip_press(active))
    .padding([0, 10])
    .style(move |_, status| iced::widget::button::Style {
        background: if open || matches!(status, iced::widget::button::Status::Hovered) {
            Some(Background::Color(Palette::hover()))
        } else if active {
            Some(Background::Color(Palette::surface_high()))
        } else {
            None
        },
        text_color: fg,
        border: Border {
            radius: 5.0.into(),
            ..Default::default()
        },
        ..Default::default()
    });
    let mut items = Vec::new();
    for lang in BodyLang::ALL {
        if lang == BodyLang::Raw {
            items.push(menu_divider());
        }
        let mark: Element<'static, Message> = if lang == current {
            icons::check().size(12).color(Palette::text()).into()
        } else {
            Space::new().width(12).into()
        };
        items.push(
            row![
                container(mark).width(16).padding(iced::padding::left(6)),
                menu_item(
                    Some(icon(lang)),
                    lang.to_string(),
                    Some(Message::Response(ResponseMsg::BodyLangSelected(lang))),
                    false,
                ),
            ]
            .align_y(Alignment::Center)
            .into(),
        );
    }
    // Left-aligned under the button, like Postman's.
    crate::ui::widgets::popup::popup_left(
        chip.into(),
        crate::ui::components::molecules::menu::menu(items, Length::Fixed(168.0)),
        Message::Sidebar(crate::message::SidebarMsg::ContextMenuClosed),
        open,
    )
}

pub fn view(tab: &RequestTabState, spinner_frame: u32, format_open: bool) -> Element<'_, Message> {
    if tab.is_loading {
        return loading_view(spinner_frame);
    }

    let Some(resp) = tab.response.as_ref() else {
        return empty_view();
    };

    if let Some(err) = &resp.error {
        return error_view(err);
    }

    if resp.is_binary {
        // An oversized binary download has no bytes kept in memory at all, so
        // there is nothing to build a preview from. Say that plainly rather
        // than falling through to a preview that can never appear.
        if resp.body_stored && resp.binary_data.is_none() {
            return binary_view_with_note(
                resp,
                "This download is too large to preview in the app. \
                 The response completed successfully — only the in-app preview is skipped.",
            );
        }
        return match &tab.response_preview {
            ResponsePreview::Spreadsheet(Ok(sheet)) => spreadsheet_view(sheet),
            ResponsePreview::Spreadsheet(Err(err)) => binary_view_with_note(resp, err),
            ResponsePreview::None => binary_view(resp),
        };
    }

    // HTML (e.g. a Laravel `dd()` page) defaults to Preview; JSON can switch
    // to Postman's key/value table. The format menu returns to the source.
    let preview_on = tab.body_preview_on();

    let page_count = tab.viewer_page_count();
    let size_note: Element<Message> = match tab.response_truncated_bytes {
        Some(held_len) => {
            let mut note = format!(
                " · page {}/{} · showing {} of {}",
                tab.viewer_page + 1,
                page_count,
                human_bytes(tab.response_editor_bytes()),
                human_bytes(held_len),
            );
            // Bodies past the HTTP inline cap are cut before the viewer ever
            // sees them — say how much never made it, instead of implying the
            // pages cover everything.
            if resp.body_stored {
                let beyond = resp.body_size.saturating_sub(resp.body.len());
                if beyond > 0 {
                    note.push_str(&format!(" (+{} past preview limit)", human_bytes(beyond)));
                }
            }
            text(note).size(TEXT_XS).color(Palette::WARNING).into()
        }
        None => Space::new().into(),
    };

    let page_nav: Element<Message> = if page_count > 1 {
        let page = tab.viewer_page;
        let prev = button(icons::arrow_left().size(11).color(Palette::text_muted()))
            .padding([4, 6])
            .style(iced::widget::button::text);
        let prev = if page > 0 {
            prev.on_press(Message::Response(ResponseMsg::ViewerPagePrev))
        } else {
            prev
        };
        let next = button(icons::arrow_right().size(11).color(Palette::text_muted()))
            .padding([4, 6])
            .style(iced::widget::button::text);
        let next = if page + 1 < page_count {
            next.on_press(Message::Response(ResponseMsg::ViewerPageNext))
        } else {
            next
        };
        row![
            prev,
            text(format!("Page {}/{}", page + 1, page_count))
                .size(TEXT_XS)
                .color(Palette::text_muted()),
            next,
        ]
        .spacing(2)
        .align_y(Alignment::Center)
        .into()
    } else {
        Space::new().into()
    };

    // Postman-style toolbar: body format menu (JSON, XML, HTML, …, Raw) and
    // Preview on the left; wrap, search and copy icons on the right.
    let format_menu = format_menu(tab.body_lang(), format_open, !preview_on);
    let preview_btn: Element<Message> = if tab.preview_available() {
        preview_button(preview_on)
    } else {
        Space::new().width(0).into()
    };
    let wrap_on = tab.response_editor.wrap_enabled();
    let lines_note: Element<Message> = if preview_on {
        Space::new().width(0).into()
    } else {
        text(format!("{} lines", tab.response_viewer_lines))
            .size(10)
            .color(Palette::text_subtle())
            .into()
    };
    let toolbar = container(
        row![
            format_menu,
            preview_btn,
            lines_note,
            size_note,
            Space::new().width(Length::Fill),
            page_nav,
            mini::icon_button(
                icons::wrap_text(),
                if wrap_on {
                    "Unwrap lines"
                } else {
                    "Wrap lines"
                },
                Some(Message::Response(ResponseMsg::ViewerWrapToggled)),
                wrap_on,
            ),
            mini::icon_button(
                icons::search(),
                "Search (Ctrl+F)",
                Some(Message::Response(ResponseMsg::ViewerEdited(
                    iced_code_editor::Message::OpenSearch,
                ))),
                false,
            ),
            mini::icon_button(
                icons::copy(),
                "Copy body",
                Some(Message::Response(ResponseMsg::CopyBody)),
                false,
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding(iced::Padding {
        top: 8.0,
        right: 16.0,
        bottom: 4.0,
        left: 16.0,
    })
    .width(Length::Fill);

    let body: Element<Message> = if preview_on && tab.html_fallback_text.is_some() {
        html_preview_view(&tab.html_views, !resp.body_stored)
    } else if let (true, Some(json)) = (preview_on, tab.parsed_json.as_ref()) {
        json_preview_view(json, &tab.json_open)
    } else if tab.viewer_processing {
        container(text("Parsing…").size(TEXT_SM).color(Palette::text_subtle()))
            .padding([8, 12])
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        tab.response_editor
            .view()
            .map(|m| Message::Response(ResponseMsg::ViewerEdited(m)))
    };

    let body = container(body)
        .padding([0, 2])
        .width(Length::Fill)
        .height(Length::Fill);

    let panel = column![].spacing(0).height(Length::Fill);
    panel.push(toolbar).push(body).into()
}

/// Postman's `▷ Preview` toggle: icon + label, filled while active.
fn preview_button(on: bool) -> Element<'static, Message> {
    let fg = if on {
        Palette::text()
    } else {
        Palette::text_muted()
    };
    button(
        container(
            row![
                icons::play().size(12).color(fg),
                text("Preview")
                    .size(12)
                    .font(crate::ui::theme::UI_FONT_MEDIUM)
                    .color(fg),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .height(26)
        .center_y(26),
    )
    .on_press(Message::Response(ResponseMsg::PreviewToggled))
    .padding([0, 10])
    .style(move |_, status| iced::widget::button::Style {
        background: (on || matches!(status, iced::widget::button::Status::Hovered))
            .then(|| Background::Color(Palette::surface_high())),
        text_color: fg,
        border: Border {
            radius: 5.0.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

/// VarDumper's dark theme (Laravel `dd()`), so dumps read as in a browser.
const DUMP_BG: Color = Color::from_rgb8(0x18, 0x17, 0x1B);

fn dump_color(kind: crate::services::html_preview::DumpKind) -> Color {
    use crate::services::html_preview::DumpKind as K;
    match kind {
        K::Default | K::Ellipsis => Color::from_rgb8(0xFF, 0x84, 0x00),
        K::Num | K::Const | K::Note | K::Index => Color::from_rgb8(0x12, 0x99, 0xDA),
        K::Str | K::Key => Color::from_rgb8(0x56, 0xDB, 0x3A),
        K::Ref => Color::from_rgb8(0xA0, 0xA0, 0xA0),
        K::Member => Color::WHITE,
        K::Meta => Color::from_rgb8(0xB7, 0x29, 0xD9),
    }
}

/// Colour runs of a dump block, compared by identity: they are rebuilt only
/// with a new response, so a pointer check is enough for the highlighter.
#[derive(Clone)]
struct DumpRuns(std::sync::Arc<Vec<crate::services::html_preview::LineRuns>>);

impl PartialEq for DumpRuns {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Feeds the precomputed dump colours to the preview's read-only editor.
struct DumpHighlighter {
    runs: DumpRuns,
    line: usize,
}

impl iced::advanced::text::Highlighter for DumpHighlighter {
    type Settings = DumpRuns;
    type Highlight = crate::services::html_preview::DumpKind;
    type Iterator<'a> =
        std::vec::IntoIter<(std::ops::Range<usize>, crate::services::html_preview::DumpKind)>;

    fn new(settings: &Self::Settings) -> Self {
        Self {
            runs: settings.clone(),
            line: 0,
        }
    }

    fn update(&mut self, new_settings: &Self::Settings) {
        self.runs = new_settings.clone();
        self.line = 0;
    }

    fn change_line(&mut self, line: usize) {
        self.line = self.line.min(line);
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        let runs: crate::services::html_preview::LineRuns = self
            .runs
            .0
            .get(self.line)
            .map(|runs| {
                runs.iter()
                    .filter(|(r, _)| r.end <= line.len())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        self.line += 1;
        runs.into_iter()
    }

    fn current_line(&self) -> usize {
        self.line
    }
}

fn dump_format(
    kind: &crate::services::html_preview::DumpKind,
    _theme: &iced::Theme,
) -> iced::advanced::text::highlighter::Format<iced::Font> {
    use crate::services::html_preview::DumpKind;
    iced::advanced::text::highlighter::Format {
        color: Some(dump_color(*kind)),
        font: matches!(kind, DumpKind::Str | DumpKind::Const).then_some(iced::Font {
            weight: iced::font::Weight::Bold,
            ..MONO
        }),
    }
}

/// HTML Preview: VarDumper blocks keep their colours and layout, other
/// content shows as readable text. Each block is a read-only editor, so its
/// text can be selected and copied (Ctrl+C). Blocks are parsed once per
/// response. The real page stays one Open in browser / Save away (full
/// body only).
fn html_preview_view(
    views: &[crate::state::tabs::HtmlBlockView],
    full_body: bool,
) -> Element<'_, Message> {
    use iced::widget::text_editor;

    let (open_action, save_action): (Element<Message>, Element<Message>) = if full_body {
        (open_in_browser_button(), save_text_button())
    } else {
        (Space::new().width(0).into(), Space::new().width(0).into())
    };

    let mut content = column![].spacing(8).padding(iced::padding::right(10));
    for (i, view) in views.iter().enumerate() {
        let dump = view.dump;
        let editor = text_editor(&view.content)
            .on_action(move |a| Message::Response(ResponseMsg::HtmlPreviewAction(i, a)))
            .size(if dump { 12.0 } else { TEXT_SM as f32 })
            .font(if dump { MONO } else { crate::ui::theme::UI_FONT })
            .line_height(if dump {
                iced::widget::text::LineHeight::Absolute(iced::Pixels(18.0))
            } else {
                iced::widget::text::LineHeight::default()
            })
            .padding(if dump {
                iced::Padding::from([6, 10])
            } else {
                iced::Padding::ZERO
            })
            .style(move |_, _| text_editor::Style {
                background: Background::Color(if dump {
                    DUMP_BG
                } else {
                    Color::TRANSPARENT
                }),
                border: Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                placeholder: Palette::text_subtle(),
                value: if dump {
                    dump_color(crate::services::html_preview::DumpKind::Default)
                } else {
                    Palette::text()
                },
                selection: Color {
                    a: 0.35,
                    ..Palette::accent()
                },
            })
            .highlight_with::<DumpHighlighter>(DumpRuns(view.runs.clone()), dump_format);
        content = content.push(crate::ui::widgets::wheel_passthrough::wheel_passthrough(editor));
    }

    container(
        column![
            row![
                text(
                    "Rendered preview. Open in browser for the full page with scripts and styles."
                )
                .size(TEXT_XS)
                .color(Palette::text_subtle()),
                Space::new().width(Length::Fill),
                open_action,
                save_action,
            ]
            .align_y(Alignment::Center),
            scrollable(content)
                .height(Length::Fill)
                .style(crate::ui::theme::thin_scrollbar),
        ]
        .spacing(6)
        .height(Length::Fill),
    )
    .padding([8, 14])
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Rows the JSON preview renders before asking for the source view. Each
/// row is a few widgets; thousands stay smooth, a 50k-item array would not.
const JSON_ROW_BUDGET: usize = 2_000;

/// Postman's JSON Preview: a key/value table; objects and arrays are rows
/// with a `{n}` / `[n]` badge that expand into a nested table.
fn json_preview_view<'a>(
    value: &'a serde_json::Value,
    open: &'a std::collections::HashSet<String>,
) -> Element<'a, Message> {
    let mut budget = JSON_ROW_BUDGET;
    let mut col = column![json_table(value, "", open, &mut budget)].spacing(8);
    if budget == 0 {
        col = col.push(
            text(format!(
                "Preview shows the first {JSON_ROW_BUDGET} rows. Switch to JSON for the full body."
            ))
            .size(TEXT_XS)
            .color(Palette::WARNING),
        );
    }
    scrollable(container(col).padding(iced::Padding {
        top: 4.0,
        right: 16.0,
        bottom: 12.0,
        left: 14.0,
    }))
    .height(Length::Fill)
    .width(Length::Fill)
    .style(crate::ui::theme::thin_scrollbar)
    .into()
}

/// Path segment of a child row: JSON-pointer escaped so keys holding `/`
/// cannot collide with nested paths.
pub(crate) fn json_child_path(parent: &str, key: &str) -> String {
    format!("{parent}/{}", key.replace('~', "~0").replace('/', "~1"))
}

/// Inline text of a scalar; long strings are cut (the source view has them).
fn json_scalar(v: &serde_json::Value) -> String {
    const MAX: usize = 2_000;
    let s = match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "null".to_owned(),
        other => other.to_string(),
    };
    if s.len() > MAX {
        let mut cut = crate::services::format::head_capped(&s, MAX).to_owned();
        cut.push('…');
        cut
    } else {
        s
    }
}

fn json_badge(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::Object(m) if m.is_empty() => Some("{}".into()),
        serde_json::Value::Object(m) => Some(format!("{{{}}}", m.len())),
        serde_json::Value::Array(a) if a.is_empty() => Some("[]".into()),
        serde_json::Value::Array(a) => Some(format!("[{}]", a.len())),
        _ => None,
    }
}

fn badge_chip(label: String) -> Element<'static, Message> {
    container(text(label).size(10).font(MONO).color(Palette::text_muted()))
        .padding([1, 5])
        .style(|_| container::Style {
            background: Some(Background::Color(Palette::surface_high())),
            border: Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

fn json_table<'a>(
    value: &'a serde_json::Value,
    path: &str,
    open: &'a std::collections::HashSet<String>,
    budget: &mut usize,
) -> Element<'a, Message> {
    let entries: Vec<(String, &serde_json::Value)> = match value {
        serde_json::Value::Object(m) => m.iter().map(|(k, v)| (k.clone(), v)).collect(),
        serde_json::Value::Array(a) => a
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v))
            .collect(),
        _ => Vec::new(),
    };
    // One key column width per table, like Postman: sized to the longest
    // scalar key so values line up.
    let longest = entries
        .iter()
        .filter(|(_, v)| !is_expandable(v))
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(4);
    let key_w = (longest as f32 * 7.4 + 20.0).clamp(44.0, 240.0);
    let key_font = iced::Font {
        weight: iced::font::Weight::Semibold,
        ..crate::ui::theme::UI_FONT
    };

    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    for (key, v) in entries {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        if is_expandable(v) {
            let child = json_child_path(path, &key);
            let expanded = open.contains(&child);
            let mut head = row![
                (if expanded {
                    icons::chevron_down()
                } else {
                    icons::chevron_right()
                })
                .size(12)
                .color(Palette::text_muted()),
                text(key).size(12).font(key_font).color(Palette::text()),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            if let Some(b) = json_badge(v) {
                head = head.push(badge_chip(b));
            }
            let head = button(container(head).padding([6, 8]).width(Length::Fill))
                .on_press(Message::Response(ResponseMsg::JsonNodeToggled(
                    child.clone(),
                )))
                .padding(0)
                .width(Length::Fill)
                .style(|_, status| iced::widget::button::Style {
                    background: matches!(status, iced::widget::button::Status::Hovered)
                        .then(|| Background::Color(Palette::hover())),
                    text_color: Palette::text(),
                    ..Default::default()
                });
            if expanded {
                rows.push(
                    column![
                        head,
                        container(json_table(v, &child, open, budget)).padding(iced::Padding {
                            top: 0.0,
                            right: 8.0,
                            bottom: 8.0,
                            left: 28.0,
                        }),
                    ]
                    .into(),
                );
            } else {
                rows.push(head.into());
            }
        } else {
            let value_cell: Element<'a, Message> = match json_badge(v) {
                Some(b) => badge_chip(b),
                None => text(json_scalar(v))
                    .size(12)
                    .color(Palette::text_muted())
                    .into(),
            };
            rows.push(
                row![
                    container(text(key).size(12).font(key_font).color(Palette::text()))
                        .width(key_w)
                        .padding([6, 8]),
                    container(value_cell).padding([6, 8]).width(Length::Fill),
                ]
                .align_y(Alignment::Center)
                .into(),
            );
        }
    }
    crate::ui::widgets::kv_table::frame(rows)
}

/// Non-empty objects and arrays open into a nested table.
fn is_expandable(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Object(m) => !m.is_empty(),
        serde_json::Value::Array(a) => !a.is_empty(),
        _ => false,
    }
}

/// Compact "Save" action matching the toolbar's Copy button. Writes the
/// response body — HTML source, JSON, text — to a file chosen by the user.
fn save_text_button() -> Element<'static, Message> {
    text_action_button("Save", Message::Response(ResponseMsg::SaveBody))
}

/// Renders the HTML body in the system browser via a temp file, so a page
/// gets a real engine and real CSS/scripts without one being linked into the
/// app.
fn open_in_browser_button() -> Element<'static, Message> {
    text_action_button(
        "Open in browser",
        Message::Response(ResponseMsg::OpenBodyInBrowser),
    )
}

/// Compact text action matching the toolbar's Copy button style.
fn text_action_button(label: &'static str, msg: Message) -> Element<'static, Message> {
    button(text(label).size(TEXT_SM).color(Palette::text_muted()))
        .on_press(msg)
        .style(|_t, status| {
            let hovered = matches!(status, iced::widget::button::Status::Hovered);
            iced::widget::button::Style {
                background: if hovered {
                    Some(Background::Color(Palette::hover()))
                } else {
                    None
                },
                text_color: Palette::text_muted(),
                border: Border {
                    radius: 6.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .padding([4, 8])
        .into()
}

/// Formats a byte count as a short human-readable size.
fn human_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// A keyboard-key chip: bordered, mono font, slight bottom "keycap" shadow.
fn kbd_chip(label: &str) -> Element<'static, Message> {
    container(
        text(label.to_owned())
            .size(TEXT_XS)
            .color(Palette::text_muted())
            .font(MONO),
    )
    .padding([3, 8])
    .style(|_| container::Style {
        background: Some(Background::Color(Palette::surface_high())),
        border: Border {
            color: Palette::border(),
            width: 1.0,
            radius: 5.0.into(),
        },
        shadow: iced::Shadow {
            color: Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.4,
            },
            offset: iced::Vector::new(0.0, 1.5),
            blur_radius: 0.0,
        },
        ..Default::default()
    })
    .into()
}

fn empty_view() -> Element<'static, Message> {
    let modifier = if cfg!(target_os = "macos") {
        "Cmd"
    } else {
        "Ctrl"
    };
    let content = column![
        text("Send a request")
            .size(TEXT_LG)
            .color(Palette::text())
            .font(crate::ui::theme::UI_FONT_MEDIUM),
        Space::new().height(8),
        row![
            text("Press").size(TEXT_SM).color(Palette::text_subtle()),
            kbd_chip(modifier),
            kbd_chip("Enter"),
            text("or click Send to run the request")
                .size(TEXT_SM)
                .color(Palette::text_subtle()),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    ]
    .align_x(Alignment::Center);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

fn error_view(err: &str) -> Element<'static, Message> {
    let content = column![
        text("Request failed")
            .size(TEXT_LG)
            .color(Palette::ERROR)
            .font(crate::ui::theme::UI_FONT_MEDIUM),
        Space::new().height(4),
        text(err.to_owned())
            .size(TEXT_SM)
            .color(Palette::text_muted()),
    ]
    .max_width(480)
    .align_x(Alignment::Center);

    container(content)
        .padding([24, 24])
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

fn binary_view(resp: &HttpResponse) -> Element<'static, Message> {
    let is_pdf = resp.headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("content-type") && value.to_ascii_lowercase().contains("pdf")
    });
    binary_view_with_note(
        resp,
        if is_pdf {
            "PDFs aren't rendered in the app — save the response and open it in your PDF viewer."
        } else {
            "This response isn't valid text, so it can't be shown in the editor. Save it to open it with the right app."
        },
    )
}

fn binary_view_with_note(resp: &HttpResponse, note: &str) -> Element<'static, Message> {
    let content_type = resp
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.clone())
        .unwrap_or_else(|| "unknown type".to_owned());

    let size_str = if resp.body_size < 1024 {
        format!("{} B", resp.body_size)
    } else if resp.body_size < 1024 * 1024 {
        format!("{:.1} KB", resp.body_size as f64 / 1024.0)
    } else {
        format!("{:.1} MB", resp.body_size as f64 / (1024.0 * 1024.0))
    };

    let mut content = column![
        text("Binary response")
            .size(TEXT_LG)
            .color(Palette::text())
            .font(crate::ui::theme::UI_FONT_MEDIUM),
        Space::new().height(4),
        text(format!("{content_type} · {size_str}"))
            .size(TEXT_SM)
            .color(Palette::text_muted()),
        Space::new().height(2),
        text(note.to_owned())
            .size(TEXT_SM)
            .color(Palette::text_subtle()),
    ]
    .align_x(Alignment::Center);

    // Binary bodies (PDFs included) have no inline renderer: offer to save
    // the bytes so they can be opened externally. Only offered while the
    // full body is still in memory; oversized downloads are dropped and the
    // note above says so.
    if resp.binary_data.is_some() {
        content = content.push(Space::new().height(14)).push(
            button(text("Save response…").size(TEXT_SM).color(Palette::text()))
                .on_press(Message::Response(ResponseMsg::SaveBody))
                .style(|_t, status| {
                    let hovered = matches!(status, iced::widget::button::Status::Hovered);
                    iced::widget::button::Style {
                        background: Some(Background::Color(if hovered {
                            Palette::hover()
                        } else {
                            Palette::surface_high()
                        })),
                        text_color: Palette::text(),
                        border: Border {
                            color: Palette::border(),
                            width: 1.0,
                            radius: 6.0.into(),
                        },
                        ..Default::default()
                    }
                })
                .padding([6, 14]),
        );
    }

    container(content)
        .padding([24, 24])
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

/// A simple scrollable grid preview of a parsed spreadsheet's first sheet.
fn spreadsheet_view(sheet: &ParsedSheet) -> Element<'static, Message> {
    let mut rows_col = column![].spacing(0);
    for (row_idx, cells) in sheet.rows.iter().enumerate() {
        let is_header = row_idx == 0;
        let mut row_widget = row![].spacing(0);
        for cell in cells {
            row_widget = row_widget.push(
                container(text(cell.clone()).size(TEXT_SM).color(if is_header {
                    Palette::text()
                } else {
                    Palette::text_muted()
                }))
                .padding([6, 10])
                .width(Length::Fixed(140.0))
                .style(move |_| container::Style {
                    background: if is_header {
                        Some(Background::Color(Palette::surface_high()))
                    } else {
                        None
                    },
                    border: Border {
                        color: Palette::border(),
                        width: 1.0,
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                }),
            );
        }
        rows_col = rows_col.push(row_widget);
    }

    let toolbar = container(
        text(format!(
            "Sheet: {} · {} row(s)",
            sheet.sheet_name,
            sheet.rows.len()
        ))
        .size(TEXT_XS)
        .color(Palette::text_subtle()),
    )
    .padding([6, 12]);

    column![
        toolbar,
        scrollable(rows_col)
            .direction(scrollable::Direction::Both {
                vertical: scrollable::Scrollbar::default(),
                horizontal: scrollable::Scrollbar::default(),
            })
            .width(Length::Fill)
            .height(Length::Fill),
    ]
    .height(Length::Fill)
    .into()
}

fn loading_view<'a>(spinner_frame: u32) -> Element<'a, Message> {
    let art = spinner_frame_art(spinner_frame);

    container(
        column![
            text(art)
                .size(11)
                .font(MONO)
                .line_height(iced::widget::text::LineHeight::Relative(1.0))
                .color(Palette::accent()),
            Space::new().height(14),
            text("sending request…")
                .size(13)
                .color(Palette::text_muted()),
        ]
        .spacing(0)
        .align_x(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

/// Number of pre-rendered spinner frames that are cycled while a request runs.
///
/// `donut` is a little software raytracer: its `theta`/`phi` loops run ~52,000
/// iterations with a `sin_cos` each. It used to be recomputed from scratch on the
/// UI thread for every frame at 30fps, competing for CPU with the very request it
/// was reporting on. The animation depends only on the frame counter, so the
/// frames are rendered once and reused.
const SPINNER_FRAMES: usize = 60;

/// Returns the pre-rendered art for `spinner_frame`, building the set on first
/// use.
fn spinner_frame_art(spinner_frame: u32) -> &'static str {
    use std::sync::OnceLock;
    static FRAMES: OnceLock<Vec<String>> = OnceLock::new();

    let frames = FRAMES.get_or_init(|| {
        (0..SPINNER_FRAMES)
            .map(|i| {
                let f = i as f32;
                donut(f * 0.04, f * 0.023)
            })
            .collect()
    });

    // `SPINNER_FRAMES` is non-zero, so the modulo is always in range.
    frames[(spinner_frame as usize) % SPINNER_FRAMES].as_str()
}

fn donut(a: f32, b: f32) -> String {
    const W: usize = 72;
    const H: usize = 22;
    const K2: f32 = 5.0;
    const R1: f32 = 1.0;
    const R2: f32 = 2.0;
    let kx = W as f32 * 0.27;
    let ky = kx * 0.47;
    let chars = b".,-~:;=!*#$@";

    let mut out = vec![b' '; W * H];
    let mut zbuf = vec![0.0f32; W * H];
    let (sa, ca) = a.sin_cos();
    let (sb, cb) = b.sin_cos();

    let mut theta = 0.0f32;
    while theta < std::f32::consts::TAU {
        let (st, ct) = theta.sin_cos();
        let mut phi = 0.0f32;
        while phi < std::f32::consts::TAU {
            let (sp, cp) = phi.sin_cos();
            let cx = R2 + R1 * ct;
            let cy = R1 * st;
            let x = cx * (cb * cp + sa * sb * sp) - cy * ca * sb;
            let y = cx * (sb * cp - sa * cb * sp) + cy * ca * cb;
            let z = K2 + ca * cx * sp + cy * sa;
            let ooz = 1.0 / z;
            let xp = (W as f32 / 2.0 + kx * ooz * x) as isize;
            let yp = (H as f32 / 2.0 - ky * ooz * y) as isize;
            let lum = cp * ct * sb - ca * ct * sp - sa * st + cb * (ca * st - ct * sa * sp);
            if xp >= 0 && xp < W as isize && yp >= 0 && yp < H as isize {
                let o = xp as usize + yp as usize * W;
                if ooz > zbuf[o] {
                    zbuf[o] = ooz;
                    let li = (lum * 8.0) as isize;
                    out[o] = chars[li.clamp(0, chars.len() as isize - 1) as usize];
                }
            }
            phi += 0.015;
        }
        theta += 0.05;
    }

    let mut s = String::with_capacity(W * H + H);
    for row in 0..H {
        s.push_str(std::str::from_utf8(&out[row * W..row * W + W]).unwrap());
        if row + 1 < H {
            s.push('\n');
        }
    }
    s
}

#[cfg(test)]
mod preview_tests {
    use super::*;

    #[test]
    fn json_paths_cannot_collide() {
        let a = json_child_path(&json_child_path("", "a"), "b");
        let ab = json_child_path("", "a/b");
        assert_ne!(a, ab);
        assert_eq!(ab, "/a~1b");
        assert_eq!(json_child_path("", "~1"), "/~01");
    }

    #[test]
    fn json_badges_and_scalars_match_postman() {
        assert_eq!(json_badge(&serde_json::json!({})).as_deref(), Some("{}"));
        assert_eq!(
            json_badge(&serde_json::json!({"a": 1, "b": 2})).as_deref(),
            Some("{2}")
        );
        assert_eq!(json_badge(&serde_json::json!([1])).as_deref(), Some("[1]"));
        assert_eq!(json_scalar(&serde_json::json!("POST")), "POST");
        assert_eq!(json_scalar(&serde_json::Value::Null), "null");
        assert!(json_scalar(&serde_json::json!("x".repeat(5_000))).ends_with('…'));
    }
}

#[cfg(test)]
mod format_chip_tests {
    use super::*;

    #[test]
    fn chip_leaves_preview_first_and_opens_the_menu_after() {
        assert!(matches!(
            format_chip_press(false),
            Message::Response(ResponseMsg::PreviewToggled)
        ));
        assert!(matches!(
            format_chip_press(true),
            Message::Sidebar(crate::message::SidebarMsg::ContextMenuOpened(_))
        ));
    }
}
