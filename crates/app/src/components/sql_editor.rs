use std::sync::Arc;

use db::sqlite::SqliteBackend;
use db::traits::DbBackend;
use db::types::QueryResult;
use dioxus::prelude::*;

#[css_module("/assets/styles/sql_editor.css")]
struct Styles;

#[component]
pub fn SqlEditor(
    backend: Signal<Option<Arc<SqliteBackend>>>,
    query_result: Signal<Option<QueryResult>>,
    error_msg: Signal<Option<String>>,
    is_connected: Signal<bool>,
) -> Element {
    let mut sql_input = use_signal(|| String::from("SELECT 1 AS result;"));
    let mut is_running = use_signal(|| false);
    let mut query_result = query_result;
    let mut error_msg = error_msg;

    let run_query = move |_| {
        let sql = sql_input.read().clone();
        let backend = backend;
        spawn(async move {
            is_running.set(true);
            error_msg.set(None);

            if let Some(b) = backend.read().as_ref() {
                match b.execute_query(&sql).await {
                    Ok(result) => {
                        query_result.set(Some(result));
                    }
                    Err(e) => {
                        error_msg.set(Some(e.to_string()));
                        query_result.set(None);
                    }
                }
            } else {
                error_msg.set(Some("Not connected to a database".into()));
            }
            is_running.set(false);
        });
    };

    rsx! {
        div { class: Styles::editor_panel,
            div { class: Styles::editor_header,
                span { "SQL Editor" }
                button {
                    class: Styles::run_btn,
                    disabled: *is_running.read() || !*is_connected.read(),
                    onclick: run_query,
                    if *is_running.read() { "Running..." } else { "Run" }
                }
            }
            textarea {
                class: Styles::sql_editor,
                value: "{sql_input}",
                placeholder: "Enter SQL query...",
                oninput: move |evt| sql_input.set(evt.value()),
            }
        }
    }
}
