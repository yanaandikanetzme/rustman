//! Response cookies, the same framed Name | Value table as Headers. Values
//! are credentials more often than not: each is masked until its eye.
use iced::Element;

use crate::{message::Message, state::tabs::RequestTabState, ui::widgets::kv_table};

use super::headers::{head, line, table};

pub fn view(tab: &RequestTabState) -> Element<'_, Message> {
    let Some(resp) = &tab.response else {
        return kv_table::empty_state("No response yet.");
    };

    let cookies: Vec<(&str, &str)> = resp
        .headers
        .iter()
        .filter(|(k, _)| k.to_ascii_lowercase() == "set-cookie")
        .flat_map(|(_, v)| {
            // The first attribute is "name=value"; extras are ";Path=..." etc.
            let name_val = v.split(';').next().unwrap_or(v.trim());
            let (name, value) = if let Some(eq) = name_val.find('=') {
                (&name_val[..eq], &name_val[eq + 1..])
            } else {
                (name_val, "")
            };
            Some((name.trim(), value.trim()))
        })
        .collect();

    if cookies.is_empty() {
        return kv_table::empty_state("No Set-Cookie headers in this response.");
    }

    let mut rows: Vec<Element<Message>> = vec![head("Name", "Value")];
    for (i, (name, value)) in cookies.into_iter().enumerate() {
        rows.push(line(tab, name, value, true, format!("c:{i}")));
    }
    table(rows)
}
