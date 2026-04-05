use db::types::{DbValue, QueryResult};
use dioxus::prelude::*;

#[css_module("/assets/styles/results_panel.css")]
struct Styles;

#[component]
pub fn ResultsPanel(
    result: Option<QueryResult>,
    error: Option<String>,
) -> Element {
    rsx! {
        div { class: Styles::results_panel,
            if let Some(err) = &error {
                div { class: "error", "{err}" }
            }
            if let Some(result) = &result {
                div { class: Styles::result_info,
                    "Rows: {result.rows.len()} | Time: {result.execution_time:?}"
                }
                if !result.columns.is_empty() {
                    table { class: Styles::result_table,
                        thead {
                            tr {
                                for col in &result.columns {
                                    th { "{col.name}" }
                                }
                            }
                        }
                        tbody {
                            for row in &result.rows {
                                tr {
                                    for cell in row {
                                        td {
                                            class: if *cell == DbValue::Null { Styles::null_value.to_string() } else { String::new() },
                                            "{cell}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
