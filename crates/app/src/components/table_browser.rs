use std::sync::Arc;
use std::time::{Duration, Instant};

use app_core::sql_update::{build_updates_for_tab, format_where_match, quote_ident, quote_qualified};
use app_core::tab_manager::{PAGE_SIZE, TabManager, TabType};
use db::traits::DbBackend;
use db::types::{DbValue, ObjectType, SchemaInfo};
use dioxus::prelude::*;
use uuid::Uuid;

use super::results_panel::{CellEdit, FkJump, ResultsPanel};
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
    let mut show_no_pk_confirm: Signal<bool> = use_signal(|| false);

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

    // Lazy-fetch FK metadata for tables (skip views) on first render.
    use_effect(move || {
        let needs_fetch = {
            let tm = tab_manager.read();
            tm.tab_by_id(tab_id)
                .and_then(|t| {
                    if let TabType::TableBrowser {
                        object_name,
                        object_type,
                        foreign_keys,
                        ..
                    } = &t.tab_type
                    {
                        if *object_type == ObjectType::Table && foreign_keys.is_none() {
                            Some(object_name.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
        };

        if let Some(object_name) = needs_fetch {
            spawn(async move {
                let b = { backend.read().as_ref().cloned() };
                let Some(b) = b else { return };
                let (schema, table) = split_qualified_name(&object_name);
                if let Ok(fks) = b.get_foreign_keys(schema.as_deref(), &table).await {
                    tab_manager.write().set_table_browser_foreign_keys(tab_id, fks);
                }
            });
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
        foreign_keys,
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
        let foreign_keys = tab
            .and_then(|t| {
                if let TabType::TableBrowser { foreign_keys, .. } = &t.tab_type {
                    foreign_keys.clone()
                } else {
                    None
                }
            })
            .unwrap_or_default();
        (
            sql,
            where_clause,
            result,
            error,
            is_loading,
            total_count,
            ordering,
            view_source_target,
            foreign_keys,
        )
    };

    let (edited_cells, object_name_for_save) = {
        let tm = tab_manager.read();
        let tab = tm.tab_by_id(tab_id);
        tab.and_then(|t| {
            if let TabType::TableBrowser {
                edited_cells,
                object_name,
                ..
            } = &t.tab_type
            {
                Some((edited_cells.clone(), object_name.clone()))
            } else {
                None
            }
        })
        .unwrap_or_default()
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

    let run_transaction = move || {
        spawn(async move {
            let snapshot = {
                let tm = tab_manager.read();
                tm.tab_by_id(tab_id).and_then(|t| {
                    if let TabType::TableBrowser {
                        object_name,
                        edited_cells,
                        primary_keys,
                        ..
                    } = &t.tab_type
                    {
                        let result = t.result.as_ref()?;
                        let (schema, table) = split_qualified_name(object_name);
                        let qualified = quote_qualified(schema.as_deref(), &table);
                        let pks = primary_keys.as_deref().unwrap_or(&[]).to_vec();
                        Some((
                            qualified,
                            result.columns.clone(),
                            result.rows.clone(),
                            edited_cells.clone(),
                            pks,
                        ))
                    } else {
                        None
                    }
                })
            };

            let Some((qualified_table, columns, rows, edited_cells, pks)) = snapshot else {
                return;
            };

            let b = { backend.read().as_ref().cloned() };
            let Some(b) = b else {
                if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
                    tab.error = Some("Not connected to a database".into());
                }
                return;
            };

            let bk = b.backend_kind();

            match build_updates_for_tab(&qualified_table, &columns, &rows, &edited_cells, &pks, bk)
            {
                Ok(stmts) => match b.execute_transaction(&stmts).await {
                    Ok(()) => {
                        tab_manager.write().clear_edited_cells(tab_id);
                        tab_manager.write().reset_for_refresh(tab_id);
                    }
                    Err(e) => {
                        if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
                            tab.error = Some(e.to_string());
                        }
                    }
                },
                Err(e) => {
                    if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
                        tab.error = Some(e);
                    }
                }
            }
        });
    };

    let save_edits = move |_| {
        spawn(async move {
            let cached_pks = {
                let tm = tab_manager.read();
                tm.tab_by_id(tab_id).and_then(|t| {
                    if let TabType::TableBrowser { primary_keys, .. } = &t.tab_type {
                        primary_keys.clone()
                    } else {
                        None
                    }
                })
            };

            let pks = if let Some(pks) = cached_pks {
                pks
            } else {
                let object_name = {
                    let tm = tab_manager.read();
                    tm.tab_by_id(tab_id).and_then(|t| {
                        if let TabType::TableBrowser { object_name, .. } = &t.tab_type {
                            Some(object_name.clone())
                        } else {
                            None
                        }
                    })
                };
                let Some(object_name) = object_name else { return; };

                let b = { backend.read().as_ref().cloned() };
                let Some(b) = b else { return; };

                let (schema, table) = split_qualified_name(&object_name);
                match b.get_primary_keys(schema.as_deref(), &table).await {
                    Ok(pks) => {
                        tab_manager.write().set_table_browser_primary_keys(tab_id, pks.clone());
                        pks
                    }
                    Err(e) => {
                        if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
                            tab.error = Some(e.to_string());
                        }
                        return;
                    }
                }
            };

            if pks.is_empty() {
                show_no_pk_confirm.set(true);
            } else {
                run_transaction();
            }
        });
    };

    let confirm_save = move |_| {
        show_no_pk_confirm.set(false);
        run_transaction();
    };

    let on_fk_jump = move |jump: FkJump| {
        // Don't navigate on NULL — the target row would be ambiguous and the
        // generated WHERE would just hide all rows anyway.
        if matches!(jump.value, DbValue::Null) {
            return;
        }
        let bk = match backend.read().as_ref() {
            Some(b) => b.backend_kind(),
            None => return,
        };
        let predicate = format!(
            "{} {}",
            quote_ident(&jump.fk.to_column),
            format_where_match(&jump.value, bk),
        );
        let new_id = tab_manager
            .write()
            .open_table_browser(jump.fk.to_table.clone(), jump.fk.to_schema.clone());
        tab_manager
            .write()
            .set_table_browser_where(new_id, predicate);
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
                    id: "where-input",
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
                editable: true,
                edited_cells,
                foreign_keys,
                on_fk_jump,
                on_cell_edit: move |edit: CellEdit| {
                    tab_manager.write().set_edited_cell(
                        tab_id,
                        edit.row,
                        &edit.column,
                        edit.new_value,
                    );
                },
                on_save: save_edits,
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
            if *show_no_pk_confirm.read() {
                div {
                    class: Styles::confirm_overlay,
                    onclick: move |_| show_no_pk_confirm.set(false),
                    div {
                        class: Styles::confirm_dialog,
                        onclick: move |evt| evt.stop_propagation(),
                        p { class: Styles::confirm_message,
                            strong { "{object_name_for_save}" }
                            " has no primary key. The save will use "
                            strong { "all original column values" }
                            " in each WHERE clause to identify rows. This may match more than one row. Continue?"
                        }
                        div { class: Styles::confirm_buttons,
                            button {
                                class: Styles::cancel_btn,
                                onclick: move |_| show_no_pk_confirm.set(false),
                                "Cancel"
                            }
                            button {
                                class: Styles::confirm_btn,
                                onclick: confirm_save,
                                "Continue and save"
                            }
                        }
                    }
                }
            }
        }
    }
}


