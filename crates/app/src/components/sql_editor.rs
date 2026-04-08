use std::sync::Arc;

use app_core::config::Theme;
use app_core::tab_manager::{TabManager, TabType};
use db::traits::DbBackend;
use db::types::SchemaInfo;
use dioxus::prelude::*;
use dioxus::prelude::Key;
use uuid::Uuid;

use super::results_panel::ResultsPanel;
use super::sql_display::SqlDisplay;
use crate::highlight::highlight_sql;

#[css_module("/assets/styles/sql_editor.css")]
struct Styles;

#[component]
pub fn SqlEditor(
    tab_id: Uuid,
    tab_manager: Signal<TabManager>,
    backend: Signal<Option<Arc<dyn DbBackend>>>,
    schema_info: Signal<Option<SchemaInfo>>,
    theme: Signal<Theme>,
) -> Element {
    let mut tab_manager = tab_manager;
    let mut schema_info = schema_info;

    let (sql_content, result, error, is_loading, is_connected, last_query) = {
        let tm = tab_manager.read();
        let tab = tm.tab_by_id(tab_id);
        let sql = tab
            .and_then(|t| {
                if let TabType::SqlEditor { sql_content } = &t.tab_type {
                    Some(sql_content.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let result = tab.and_then(|t| t.result.clone());
        let error = tab.and_then(|t| t.error.clone());
        let is_loading = tab.map(|t| t.is_loading).unwrap_or(false);
        let last_query = tab
            .and_then(|t| t.result.as_ref().map(|r| r.query.clone()));
        (sql, result, error, is_loading, backend.read().is_some(), last_query)
    };

    let mut execute_query = move || {
        let sql = {
            let tm = tab_manager.read();
            tm.tab_by_id(tab_id).and_then(|t| {
                if let TabType::SqlEditor { sql_content } = &t.tab_type {
                    Some(sql_content.clone())
                } else {
                    None
                }
            })
        };

        let token = {
            let tm = tab_manager.read();
            tm.tab_by_id(tab_id)
                .map(|t| t.cancellation_token.clone())
        };

        if let (Some(sql), Some(token)) = (sql, token) {
            if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
                tab.is_loading = true;
                tab.error = None;
            }

            spawn(async move {
                if let Some(b) = backend.read().as_ref() {
                    let b = b.clone();
                    tokio::select! {
                        result = b.execute_query(&sql) => {
                            if !token.is_cancelled() {
                                let ok = result.is_ok();
                                if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
                                    match result {
                                        Ok(r) => {
                                            tab.result = Some(r);
                                            tab.error = None;
                                        }
                                        Err(e) => {
                                            tab.error = Some(e.to_string());
                                            tab.result = None;
                                        }
                                    }
                                    tab.is_loading = false;
                                }
                                if ok {
                                    if let Ok(info) = b.introspect().await {
                                        schema_info.set(Some(info));
                                    }
                                }
                            }
                        }
                        _ = token.cancelled() => {}
                    }
                } else if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
                    tab.error = Some("Not connected to a database".into());
                    tab.is_loading = false;
                }
            });
        }
    };

    let run_query = move |_| {
        execute_query();
    };

    rsx! {
        div { class: Styles::editor_panel,
            if let Some(query) = &last_query {
                SqlDisplay { sql: query.clone() }
            }
            div { class: Styles::editor_header,
                span { "SQL Editor — Ctrl+Enter to run" }
                button {
                    class: Styles::run_btn,
                    disabled: is_loading || !is_connected,
                    onclick: run_query,
                    if is_loading { "Running..." } else { "Run" }
                }
            }
            {
                let highlighted = if sql_content.is_empty() {
                    String::new()
                } else {
                    highlight_sql(&sql_content, *theme.read())
                };
                // Append a trailing newline so the highlight layer stays in sync
                // when the textarea content ends with a newline.
                let highlighted = format!("{highlighted}\n");
                rsx! {
                    div { class: Styles::editor_container,
                        pre {
                            class: Styles::highlight_layer,
                            dangerous_inner_html: highlighted,
                        }
                        textarea {
                            class: Styles::sql_editor,
                            value: "{sql_content}",
                            placeholder: "Enter SQL query...",
                            spellcheck: false,
                            oninput: move |evt| {
                                if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
                                    if let TabType::SqlEditor { sql_content } = &mut tab.tab_type {
                                        *sql_content = evt.value();
                                    }
                                }
                            },
                            onkeydown: move |evt: KeyboardEvent| {
                                if evt.key() == Key::Enter && (evt.modifiers().ctrl() || evt.modifiers().meta()) {
                                    evt.prevent_default();
                                    execute_query();
                                }
                            },
                            onscroll: move |_| {
                                // Sync scroll from textarea to highlight layer via JS
                                document::eval(r#"
                                    (function() {
                                        var ta = document.querySelector('.sql-editor');
                                        var hl = document.querySelector('.highlight-layer');
                                        if (ta && hl) {
                                            hl.scrollTop = ta.scrollTop;
                                            hl.scrollLeft = ta.scrollLeft;
                                        }
                                    })()
                                "#);
                            },
                        }
                    }
                }
            }
            ResultsPanel { result, error }
        }
    }
}
