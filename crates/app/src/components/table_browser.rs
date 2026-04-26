use std::sync::Arc;
use std::time::{Duration, Instant};

use app_core::tab_manager::{PAGE_SIZE, TabManager, TabType};
use db::traits::DbBackend;
use db::types::{DbValue, ObjectType, SchemaInfo};
use dioxus::prelude::*;
use uuid::Uuid;

use super::results_panel::ResultsPanel;
use super::sql_display::SqlDisplay;
use crate::operation_feedback::{
    OP_TIMEOUT, SLOW_WARNING_MS, remaining_timeout, slow_warning_message, timeout_error_message,
};
use crate::utils::split_qualified_name;

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
    let mut loading_started_at: Signal<Option<Instant>> = use_signal(|| None);
    let mut loading_elapsed_ms: Signal<u128> = use_signal(|| 0);

    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_millis(150)).await;
            let is_loading = {
                let tm = tab_manager.read();
                tm.tab_by_id(tab_id).map(|t| t.is_loading).unwrap_or(false)
            };
            if is_loading {
                let started = loading_started_at.read().unwrap_or_else(Instant::now);
                if loading_started_at.read().is_none() {
                    loading_started_at.set(Some(started));
                }
                loading_elapsed_ms.set(started.elapsed().as_millis());
            } else {
                loading_started_at.set(None);
                loading_elapsed_ms.set(0);
            }
        }
    });

    // Auto-execute query on mount if no result yet
    use_effect(move || {
        let has_result = {
            let tm = tab_manager.read();
            tm.tab_by_id(tab_id)
                .map(|t| {
                    t.result.is_some()
                        || t.is_loading
                        || t.total_count.is_some()
                        || t.error.is_some()
                })
                .unwrap_or(true)
        };

        if !has_result {
            let (sql, count_sql) = {
                let tm = tab_manager.read();
                tm.tab_by_id(tab_id)
                    .and_then(|t| {
                        if let TabType::TableBrowser {
                            generated_sql,
                            count_sql,
                            ..
                        } = &t.tab_type
                        {
                            Some((generated_sql.clone(), count_sql.clone()))
                        } else {
                            None
                        }
                    })
                    .unzip()
            };

            let token = {
                let tm = tab_manager.read();
                tm.tab_by_id(tab_id).map(|t| t.cancellation_token.clone())
            };

            if let (Some(sql), Some(token)) = (sql, token) {
                if let Some(tm) = tab_manager.write().tab_by_id_mut(tab_id) {
                    tm.is_loading = true;
                }

                spawn(async move {
                    let b = { backend.read().as_ref().cloned() };
                    if let Some(b) = b {
                        let started_at = Instant::now();

                        // Step 1: count query — bail early on error, set total_count if > 0
                        if let Some(csql) = count_sql {
                            let Some(remaining) = remaining_timeout(started_at) else {
                                if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
                                    tab.error = Some(timeout_error_message("Request"));
                                    tab.is_loading = false;
                                }
                                return;
                            };
                            let count_result = tokio::select! {
                                r = tokio::time::timeout(remaining, b.execute_query(&csql)) => r,
                                _ = token.cancelled() => { return; }
                            };
                            match count_result {
                                Ok(Err(e)) => {
                                    if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
                                        tab.error = Some(e.to_string());
                                        tab.is_loading = false;
                                    }
                                    return;
                                }
                                Ok(Ok(r)) => {
                                    let n = r
                                        .rows
                                        .first()
                                        .and_then(|row| row.first())
                                        .and_then(|v| {
                                            if let DbValue::Int(n) = v {
                                                Some(*n as u64)
                                            } else {
                                                None
                                            }
                                        })
                                        .unwrap_or(0);
                                    if n > 0 {
                                        if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id)
                                        {
                                            tab.total_count = Some(n);
                                        }
                                    }
                                }
                                Err(_) => {
                                    if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
                                        tab.error = Some(timeout_error_message("Request"));
                                        tab.is_loading = false;
                                    }
                                    return;
                                }
                            }
                        }

                        // Step 2: main data query
                        let Some(remaining) = remaining_timeout(started_at) else {
                            if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
                                tab.error = Some(timeout_error_message("Request"));
                                tab.is_loading = false;
                            }
                            return;
                        };
                        tokio::select! {
                            result = tokio::time::timeout(remaining, b.execute_query(&sql)) => {
                                if !token.is_cancelled() {
                                    let ok = matches!(result, Ok(Ok(_)));
                                    if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
                                        match result {
                                            Ok(Ok(r)) => {
                                                tab.result = Some(r);
                                                tab.error = None;
                                            }
                                            Ok(Err(e)) => {
                                                tab.error = Some(e.to_string());
                                                tab.result = None;
                                            }
                                            Err(_) => {
                                                tab.error = Some(timeout_error_message("Request"));
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
        }
    });

    let (
        generated_sql,
        where_clause,
        result,
        error,
        is_loading,
        total_count,
        ordering,
        view_source_target,
    ) = {
        let tm = tab_manager.read();
        let tab = tm.tab_by_id(tab_id);
        let (sql, where_clause) = tab
            .and_then(|t| {
                if let TabType::TableBrowser {
                    generated_sql,
                    where_clause,
                    ..
                } = &t.tab_type
                {
                    Some((generated_sql.clone(), where_clause.clone()))
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let result = tab.and_then(|t| t.result.clone());
        let error = tab.and_then(|t| t.error.clone());
        let is_loading = tab.map(|t| t.is_loading).unwrap_or(false);
        let total_count = tab.and_then(|t| t.total_count);
        let ordering = tm.tab_column_ordering(tab_id);
        let view_source_target = tab.and_then(|t| {
            if let TabType::TableBrowser {
                object_name,
                object_type,
                ..
            } = &t.tab_type
            {
                if object_type == &ObjectType::View {
                    let (schema, name) = split_qualified_name(object_name);
                    Some((name, schema))
                } else {
                    None
                }
            } else {
                None
            }
        });
        (
            sql,
            where_clause,
            result,
            error,
            is_loading,
            total_count,
            ordering,
            view_source_target,
        )
    };

    let mut where_input = use_signal(|| where_clause.clone());

    let row_count = result.as_ref().map(|r| r.rows.len()).unwrap_or(0);
    let has_more = row_count > 0 && row_count % PAGE_SIZE == 0;

    let refresh = move |_| {
        tab_manager.write().reset_for_refresh(tab_id);
    };

    let sort_by_column = move |column_name: String| {
        let updated = tab_manager
            .write()
            .cycle_order_by_column(tab_id, &column_name)
            .is_some();
        if updated {
            tab_manager.write().reset_for_refresh(tab_id);
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
            tm.tab_by_id(tab_id).map(|t| t.cancellation_token.clone())
        };

        if let (Some(sql), Some(token)) = (sql, token) {
            if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
                tab.is_loading = true;
            }

            spawn(async move {
                let b = { backend.read().as_ref().cloned() };
                if let Some(b) = b {
                    tokio::select! {
                        result = tokio::time::timeout(OP_TIMEOUT, b.execute_query(&sql)) => {
                            if !token.is_cancelled() {
                                if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
                                    match result {
                                        Ok(Ok(new_result)) => {
                                            if let Some(existing) = tab.result.as_mut() {
                                                existing.rows.extend(new_result.rows);
                                                existing.execution_time += new_result.execution_time;
                                            }
                                            tab.error = None;
                                        }
                                        Ok(Err(e)) => {
                                            tab.error = Some(e.to_string());
                                        }
                                        Err(_) => {
                                            tab.error = Some(timeout_error_message("Query"));
                                        }
                                    }
                                    tab.is_loading = false;
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

    let elapsed_ms = *loading_elapsed_ms.read();
    let elapsed_secs = elapsed_ms as f64 / 1000.0;
    let taking_longer = is_loading && elapsed_ms >= SLOW_WARNING_MS;

    rsx! {
        div { class: Styles::table_browser,
            if let Some((view_name, schema)) = view_source_target {
                SqlDisplay {
                    sql: generated_sql,
                    action_label: "View Source".to_string(),
                    on_action: move |_| {
                        tab_manager.write().open_view_source(view_name.clone(), schema.clone());
                    }
                }
            } else {
                SqlDisplay { sql: generated_sql }
            }
            div { class: Styles::where_bar,
                span { class: Styles::where_label, "WHERE" }
                input {
                    class: Styles::where_input,
                    r#type: "text",
                    placeholder: "Type a filter and press Enter (e.g. id > 5)",
                    value: "{where_input.read()}",
                    oninput: move |evt| where_input.set(evt.value()),
                    onkeydown: move |evt: KeyboardEvent| {
                        if evt.key() == Key::Enter {
                            evt.prevent_default();
                            let value = where_input.read().clone();
                            let applied = tab_manager.write().set_table_browser_where(tab_id, value);
                            if applied {
                                let normalized = {
                                    let tm = tab_manager.read();
                                    tm.tab_by_id(tab_id).and_then(|t| {
                                        if let TabType::TableBrowser { where_clause, .. } = &t.tab_type {
                                            Some(where_clause.clone())
                                        } else {
                                            None
                                        }
                                    })
                                };
                                if let Some(normalized) = normalized {
                                    where_input.set(normalized);
                                }
                            }
                        }
                    },
                }
            }
            if is_loading {
                div { class: Styles::loading, "Loading object... {elapsed_secs:.1}s" }
            }
            if taking_longer {
                div { class: Styles::slow_warning,
                    "{slow_warning_message()}"
                }
            }
            ResultsPanel {
                result,
                error,
                total_count,
                on_refresh: refresh,
                on_sort_column: sort_by_column,
                ordering,
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


