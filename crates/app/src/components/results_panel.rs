use app_core::tab_manager::{ColumnOrderInfo, SortDirection};
use db::types::{DbValue, QueryResult};
use dioxus::prelude::*;

#[css_module("/assets/styles/results_panel.css")]
struct Styles;

fn normalize_column_key(name: &str) -> String {
    let trimmed = name.trim();
    let unquoted = if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    unquoted.to_ascii_lowercase()
}

#[component]
pub fn ResultsPanel(
    result: Option<QueryResult>,
    error: Option<String>,
    #[props(default)] total_count: Option<u64>,
    #[props(default)] on_refresh: Option<EventHandler>,
    #[props(default)] on_sort_column: Option<EventHandler<String>>,
    #[props(default)] ordering: Vec<ColumnOrderInfo>,
    children: Element,
) -> Element {
    rsx! {
        div { class: Styles::results_panel,
            if let Some(err) = &error {
                div { class: "error", "{err}" }
            }
            if let Some(result) = &result {
                div { class: Styles::result_info,
                    if let Some(total) = total_count {
                        "Rows: {result.rows.len()} | Total: {total} | Time: {result.execution_time:?}"
                    } else {
                        "Rows: {result.rows.len()} | Time: {result.execution_time:?}"
                    }
                    if let Some(handler) = on_refresh {
                        button {
                            class: Styles::refresh_btn,
                            onclick: move |_| handler.call(()),
                            "↻ Refresh"
                        }
                    }
                }
            }
            if let Some(result) = &result {
                if !result.columns.is_empty() {
                    table { class: Styles::result_table,
                        thead {
                            tr {
                                for col in &result.columns {
                                    {
                                        let column_name = col.name.clone();
                                        let sort_handler = on_sort_column.clone();
                                        let column_key = normalize_column_key(&col.name);
                                        let sort_info = ordering.iter().find(|o| o.column_key == column_key);
                                        let arrow = sort_info.map(|info| {
                                            if info.direction == SortDirection::Asc { "▲" } else { "▼" }
                                        });
                                        let show_precedence = ordering.len() > 1;
                                        rsx! {
                                            th {
                                                class: if sort_handler.is_some() { Styles::sortable_header.to_string() } else { String::new() },
                                                onclick: move |_| {
                                                    if let Some(handler) = sort_handler.as_ref() {
                                                        handler.call(column_name.clone());
                                                    }
                                                },
                                                span { "{col.name}" }
                                                if let Some(a) = arrow {
                                                    span { class: Styles::sort_indicator,
                                                        " {a}"
                                                        if show_precedence {
                                                            if let Some(info) = sort_info {
                                                                "{info.precedence}"
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
            {children}
        }
    }
}
