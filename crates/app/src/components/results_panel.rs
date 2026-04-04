use db::types::{DbValue, QueryResult};
use dioxus::prelude::*;

#[component]
pub fn ResultsPanel(
    query_result: Signal<Option<QueryResult>>,
    error_msg: Signal<Option<String>>,
) -> Element {
    rsx! {
        div { class: "results-panel",
            if let Some(err) = error_msg.read().as_ref() {
                div { class: "error", "{err}" }
            }
            if let Some(result) = query_result.read().as_ref() {
                div { class: "result-info",
                    "Query: {result.query} | Rows: {result.rows.len()} | Time: {result.execution_time:?}"
                }
                if !result.columns.is_empty() {
                    table { class: "result-table",
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
                                            class: if *cell == DbValue::Null { "null-value" } else { "" },
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
