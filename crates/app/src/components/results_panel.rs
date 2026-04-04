use db::types::{DbValue, QueryResult};
use dioxus::prelude::*;

#[css_module("/assets/styles/results_panel.css")]
struct Styles;

#[component]
pub fn ResultsPanel(
    query_result: Signal<Option<QueryResult>>,
    error_msg: Signal<Option<String>>,
) -> Element {
    rsx! {
        div { class: Styles::results_panel,
            if let Some(err) = error_msg.read().as_ref() {
                div { class: "error", "{err}" }
            }
            if let Some(result) = query_result.read().as_ref() {
                div { class: Styles::result_info,
                    "Query: {result.query} | Rows: {result.rows.len()} | Time: {result.execution_time:?}"
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
