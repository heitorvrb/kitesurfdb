use std::sync::Arc;

use app_core::tab_manager::{TabManager, TabType};
use db::traits::DbBackend;
use db::types::SchemaInfo;
use dioxus::prelude::*;
use uuid::Uuid;

use super::results_panel::ResultsPanel;
use super::sql_display::SqlDisplay;

#[css_module("/assets/styles/table_browser.css")]
struct Styles;

#[component]
pub fn TableBrowser(
    tab_id: Uuid,
    tab_manager: Signal<TabManager>,
    backend: Signal<Option<Arc<dyn DbBackend>>>,
    schema_info: Signal<Option<SchemaInfo>>,
) -> Element {
    let mut tab_manager = tab_manager;
    let mut schema_info = schema_info;

    // Auto-execute query on mount if no result yet
    use_effect(move || {
        let has_result = {
            let tm = tab_manager.read();
            tm.tab_by_id(tab_id)
                .map(|t| t.result.is_some() || t.is_loading)
                .unwrap_or(true)
        };

        if !has_result {
            let sql = {
                let tm = tab_manager.read();
                tm.tab_by_id(tab_id).and_then(|t| {
                    if let TabType::TableBrowser { generated_sql, .. } = &t.tab_type {
                        Some(generated_sql.clone())
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
                if let Some(tm) = tab_manager.write().tab_by_id_mut(tab_id) {
                    tm.is_loading = true;
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
                    }
                });
            }
        }
    });

    let (generated_sql, result, error, is_loading) = {
        let tm = tab_manager.read();
        let tab = tm.tab_by_id(tab_id);
        let sql = tab
            .and_then(|t| {
                if let TabType::TableBrowser { generated_sql, .. } = &t.tab_type {
                    Some(generated_sql.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let result = tab.and_then(|t| t.result.clone());
        let error = tab.and_then(|t| t.error.clone());
        let is_loading = tab.map(|t| t.is_loading).unwrap_or(false);
        (sql, result, error, is_loading)
    };

    rsx! {
        div { class: Styles::table_browser,
            SqlDisplay { sql: generated_sql }
            if is_loading {
                div { class: Styles::loading, "Loading..." }
            }
            ResultsPanel { result, error }
        }
    }
}
