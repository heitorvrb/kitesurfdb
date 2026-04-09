use std::sync::Arc;

use app_core::tab_manager::{TabManager, TabType, PAGE_SIZE};
use db::traits::DbBackend;
use db::types::{DbValue, QueryResult, SchemaInfo};
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
                .map(|t| t.result.is_some() || t.is_loading || t.total_count.is_some())
                .unwrap_or(true)
        };

        if !has_result {
            let (sql, count_sql) = {
                let tm = tab_manager.read();
                tm.tab_by_id(tab_id)
                    .and_then(|t| {
                        if let TabType::TableBrowser { generated_sql, count_sql, .. } = &t.tab_type {
                            Some((generated_sql.clone(), count_sql.clone()))
                        } else {
                            None
                        }
                    })
                    .unzip()
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

                        // Step 1: count query — bail early if 0 or failed
                        if let Some(csql) = count_sql {
                            let count_result = tokio::select! {
                                r = b.execute_query(&csql) => r,
                                _ = token.cancelled() => { return; }
                            };
                            match count_result {
                                Err(e) => {
                                    if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
                                        tab.error = Some(e.to_string());
                                        tab.is_loading = false;
                                    }
                                    return;
                                }
                                Ok(r) => {
                                    let n = r.rows.first()
                                        .and_then(|row| row.first())
                                        .and_then(|v| if let DbValue::Int(n) = v { Some(*n as u64) } else { None })
                                        .unwrap_or(0);
                                    if n == 0 {
                                        if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
                                            tab.result = Some(QueryResult {
                                                columns: vec![],
                                                rows: vec![],
                                                rows_affected: 0,
                                                execution_time: r.execution_time,
                                                query: r.query,
                                            });
                                            tab.is_loading = false;
                                        }
                                        return;
                                    }
                                    if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
                                        tab.total_count = Some(n);
                                    }
                                }
                            }
                        }

                        // Step 2: main data query
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

    let (generated_sql, result, error, is_loading, total_count) = {
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
        let total_count = tab.and_then(|t| t.total_count);
        (sql, result, error, is_loading, total_count)
    };

    let row_count = result.as_ref().map(|r| r.rows.len()).unwrap_or(0);
    let has_more = row_count > 0 && row_count % PAGE_SIZE == 0;

    let refresh = move |_| {
        if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
            tab.result = None;
            tab.total_count = None;
            tab.error = None;
            tab.is_loading = false;
        }
    };

    let load_more = move |_| {
        let offset = {
            let tm = tab_manager.read();
            tm.tab_by_id(tab_id)
                .and_then(|t| t.result.as_ref().map(|r| r.rows.len()))
                .unwrap_or(0)
        };

        let sql = {
            let tm = tab_manager.read();
            tm.tab_by_id(tab_id).and_then(|t| {
                if let TabType::TableBrowser { generated_sql, .. } = &t.tab_type {
                    Some(format!("{generated_sql} OFFSET {offset}"))
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
            }

            spawn(async move {
                if let Some(b) = backend.read().as_ref() {
                    let b = b.clone();
                    tokio::select! {
                        result = b.execute_query(&sql) => {
                            if !token.is_cancelled() {
                                if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
                                    match result {
                                        Ok(new_result) => {
                                            if let Some(existing) = tab.result.as_mut() {
                                                existing.rows.extend(new_result.rows);
                                                existing.execution_time += new_result.execution_time;
                                            }
                                            tab.error = None;
                                        }
                                        Err(e) => {
                                            tab.error = Some(e.to_string());
                                        }
                                    }
                                    tab.is_loading = false;
                                }
                            }
                        }
                        _ = token.cancelled() => {}
                    }
                }
            });
        }
    };

    rsx! {
        div { class: Styles::table_browser,
            SqlDisplay { sql: generated_sql }
            if is_loading {
                div { class: Styles::loading, "Loading..." }
            }
            ResultsPanel { result, error, total_count, on_refresh: refresh,
                if has_more && !is_loading {
                    div { class: Styles::load_more_bar,
                        button {
                            class: Styles::load_more_btn,
                            onclick: load_more,
                            "Load more rows"
                        }
                    }
                }
            }
        }
    }
}
