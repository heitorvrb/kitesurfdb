use std::sync::Arc;

use db::sqlite::SqliteBackend;
use db::types::QueryResult;
use dioxus::prelude::*;

use crate::components::{ConnectionBar, ResultsPanel, SqlEditor};

const STYLE: &str = include_str!("../../assets/styles/main.css");

#[component]
pub fn App() -> Element {
    let backend: Signal<Option<Arc<SqliteBackend>>> = use_signal(|| None);
    let is_connected = use_signal(|| false);
    let query_result: Signal<Option<QueryResult>> = use_signal(|| None);
    let error_msg: Signal<Option<String>> = use_signal(|| None);

    rsx! {
        style { {STYLE} }
        div { class: "app",
            ConnectionBar {
                backend,
                is_connected,
                query_result,
                error_msg,
            }
            main { class: "workspace",
                SqlEditor {
                    backend,
                    query_result,
                    error_msg,
                    is_connected,
                }
                ResultsPanel {
                    query_result,
                    error_msg,
                }
            }
        }
    }
}
