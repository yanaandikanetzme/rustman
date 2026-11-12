//! DataTable molecule: a compact read-only table with a caption header row.
//! Cells are any element, so a column can hold buttons.

use iced::{
    Alignment, Background, Border, Element, Length,
    widget::{Column, Row, column, container, rule},
};

use crate::ui::{components::atoms::text, theme::Palette};

/// Builder:
/// `DataTable::new([("Field", Length::Fixed(130.0)), ("main", Length::FillPortion(1))])
///     .row(vec![a, b])`.
/// Each row must have one cell per column.
pub struct DataTable<'a, M> {
    headers: Vec<&'static str>,
    widths: Vec<Length>,
    rows: Vec<Element<'a, M>>,
    style: Style,
}

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Compact,
    Ruled,
    Spacious,
}

impl<'a, M: 'a> DataTable<'a, M> {
    pub fn new(columns: impl IntoIterator<Item = (&'static str, Length)>) -> Self {
        Self::build(columns, Style::Compact)
    }

    /// Dense editor table with straight edges and hairline cell dividers.
    pub fn ruled(columns: impl IntoIterator<Item = (&'static str, Length)>) -> Self {
        Self::build(columns, Style::Ruled)
    }

    /// Page table: roomier rows split by hairlines, padded like a card.
    pub fn spacious(columns: impl IntoIterator<Item = (&'static str, Length)>) -> Self {
        Self::build(columns, Style::Spacious)
    }

    fn build(columns: impl IntoIterator<Item = (&'static str, Length)>, style: Style) -> Self {
        let (headers, widths): (Vec<_>, Vec<_>) = columns.into_iter().unzip();
        Self {
            headers,
            widths,
            rows: Vec::new(),
            style,
        }
    }

    pub fn row(mut self, cells: Vec<Element<'a, M>>) -> Self {
        debug_assert_eq!(cells.len(), self.widths.len(), "one cell per column");
        self.rows.push(line(cells, &self.widths, self.style, false));
        self
    }

    pub fn row_with_hover(mut self, cells: Vec<Element<'a, M>>, enter: M, exit: M) -> Self
    where
        M: Clone,
    {
        debug_assert_eq!(cells.len(), self.widths.len(), "one cell per column");
        self.rows.push(
            iced::widget::mouse_area(line(cells, &self.widths, self.style, false))
                .on_enter(enter)
                .on_exit(exit)
                .into(),
        );
        self
    }
}

fn line<'a, M: 'a>(
    cells: Vec<Element<'a, M>>,
    widths: &[Length],
    style: Style,
    header: bool,
) -> Element<'a, M> {
    let ruled = style == Style::Ruled;
    let mut r: Row<'a, M> = Row::new()
        .spacing(match style {
            Style::Compact => 10,
            Style::Ruled => 0,
            Style::Spacious => 12,
        })
        .align_y(Alignment::Center);
    if ruled {
        r = r.height(28);
    }
    for (i, (cell, w)) in cells.into_iter().zip(widths).enumerate() {
        if ruled && i > 0 {
            r = r.push(rule::vertical(1).style(crate::ui::styles::divider));
        }
        r = r.push(
            container(cell)
                .padding(if ruled { [2, 6] } else { [0, 0] })
                .width(*w),
        );
    }
    match style {
        Style::Ruled => column![r, rule::horizontal(1).style(crate::ui::styles::divider)]
            .spacing(0)
            .into(),
        Style::Compact => container(r).padding([5, 10]).width(Length::Fill).into(),
        Style::Spacious => container(r)
            .padding(if header { [8, 14] } else { [10, 14] })
            .width(Length::Fill)
            .into(),
    }
}

impl<'a, M: 'a> From<DataTable<'a, M>> for Element<'a, M> {
    fn from(t: DataTable<'a, M>) -> Self {
        let style = t.style;
        let header = line(
            t.headers.into_iter().map(|h| text::caption(h)).collect(),
            &t.widths,
            style,
            true,
        );
        let mut rows: Column<'a, M> = column![header].spacing(0);
        for row in t.rows {
            if style == Style::Spacious {
                rows = rows.push(rule::horizontal(1).style(crate::ui::styles::divider));
            }
            rows = rows.push(row);
        }
        let ruled = style == Style::Ruled;
        container(rows)
            .width(Length::Fill)
            .height(Length::Shrink)
            .style(move |_| container::Style {
                background: (!ruled).then(|| Background::Color(Palette::surface())),
                border: Border {
                    color: Palette::border_subtle(),
                    width: if ruled { 0.0 } else { 1.0 },
                    radius: if ruled { 0.0.into() } else { 6.0.into() },
                },
                ..Default::default()
            })
            .into()
    }
}
