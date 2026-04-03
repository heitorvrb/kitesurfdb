use std::sync::Arc;

use db::sqlite::SqliteBackend;
use db::traits::DbBackend;
use db::types::{ConnectionConfig, DbValue, QueryResult};
use dioxus::prelude::*;

const STYLE: &str = include_str!("../../assets/styles/main.css");

#[component]
pub fn App() -> Element {
    let mut db_path = use_signal(|| String::from(":memory:"));
    let backend: Signal<Option<Arc<SqliteBackend>>> = use_signal(|| None);
    let mut is_connected = use_signal(|| false);
    let mut connection_error: Signal<Option<String>> = use_signal(|| None);

    let mut sql_input = use_signal(|| String::from("SELECT 1 AS result;"));
    let mut query_result: Signal<Option<QueryResult>> = use_signal(|| None);
    let mut error_msg: Signal<Option<String>> = use_signal(|| None);
    let mut is_running = use_signal(|| false);

    let connect = move |_| {
        let path = db_path.read().clone();
        let mut backend = backend.clone();
        spawn(async move {
            connection_error.set(None);
            let config = ConnectionConfig::new_sqlite("session", &path);
            match SqliteBackend::connect(&config).await {
                Ok(b) => {
                    backend.set(Some(Arc::new(b)));
                    is_connected.set(true);
                    query_result.set(None);
                    error_msg.set(None);
                }
                Err(e) => {
                    connection_error.set(Some(e.to_string()));
                    is_connected.set(false);
                }
            }
        });
    };

    let disconnect = move |_| {
        let mut backend = backend.clone();
        spawn(async move {
            if let Some(b) = backend.read().as_ref() {
                let _ = b.disconnect().await;
            }
            backend.set(None);
            is_connected.set(false);
            query_result.set(None);
            error_msg.set(None);
        });
    };

    let run_query = move |_| {
        let sql = sql_input.read().clone();
        let backend = backend.clone();
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
        style { {STYLE} }
        div { class: "app",
            header { class: "toolbar",
                h1 { "DB Client" }
                div { class: "connection-bar",
                    input {
                        class: "db-path-input",
                        value: "{db_path}",
                        placeholder: "SQLite path (e.g. :memory: or /path/to/db.sqlite)",
                        disabled: *is_connected.read(),
                        oninput: move |evt| db_path.set(evt.value()),
                    }
                    if *is_connected.read() {
                        button {
                            class: "disconnect-btn",
                            onclick: disconnect,
                            "Disconnect"
                        }
                    } else {
                        button {
                            class: "connect-btn",
                            onclick: connect,
                            "Connect"
                        }
                    }
                    if *is_connected.read() {
                        span { class: "status connected", "Connected" }
                    } else {
                        span { class: "status disconnected", "Disconnected" }
                    }
                }
            }
            if let Some(err) = connection_error.read().as_ref() {
                div { class: "error", "{err}" }
            }
            main { class: "workspace",
                div { class: "editor-panel",
                    div { class: "editor-header",
                        span { "SQL Editor" }
                        button {
                            class: "run-btn",
                            disabled: *is_running.read() || !*is_connected.read(),
                            onclick: run_query,
                            if *is_running.read() { "Running..." } else { "Run" }
                        }
                    }
                    textarea {
                        class: "sql-editor",
                        value: "{sql_input}",
                        placeholder: "Enter SQL query...",
                        oninput: move |evt| sql_input.set(evt.value()),
                    }
                }
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
    }
}
