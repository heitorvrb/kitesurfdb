use std::sync::Arc;

use app_core::tab_manager::{TabManager, TabType};
use crate::operation_feedback::{OP_TIMEOUT, timeout_error_message};
use crate::utils::split_qualified_name;
use db::traits::DbBackend;
use db::types::{ObjectType, SchemaInfo};
use dioxus::prelude::*;

/// Registers global keyboard shortcut handlers for the application.
///
/// Uses a document-level JS event listener (via eval) so shortcuts fire
/// regardless of which element has focus.
pub fn use_keyboard_shortcuts(
    tab_manager: Signal<TabManager>,
    backend: Signal<Option<Arc<dyn DbBackend>>>,
    schema_info: Signal<Option<SchemaInfo>>,
    show_search_modal: Signal<bool>,
) {
    use_future(move || async move {
        let mut eval = document::eval(
            r#"
            document.addEventListener('keydown', function(e) {
                if (e.key === 'F5') {
                    e.preventDefault();
                    dioxus.send('F5');
                    return;
                }

                if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'w') {
                    e.preventDefault();
                    dioxus.send('CLOSE_TAB');
                    return;
                }

                if ((e.ctrlKey || e.metaKey) && e.key === 'T') {
                    e.preventDefault();
                    const el = document.getElementById('where-input');
                    if (el) { el.focus(); el.select(); }
                    return;
                }

                if ((e.ctrlKey || e.metaKey) && e.key === 't') {
                    e.preventDefault();
                    dioxus.send('NEW_EDITOR_TAB');
                    return;
                }

                if ((e.ctrlKey || e.metaKey) && (e.code === 'Tab' || e.key === 'Tab' || e.key === 'ISO_Left_Tab')) {
                    e.preventDefault();
                    dioxus.send((e.shiftKey || e.key === 'ISO_Left_Tab') ? 'PREV_TAB' : 'NEXT_TAB');
                    return;
                }

                if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'f') {
                    e.preventDefault();
                    dioxus.send('OPEN_SEARCH');
                }

                if (e.key === 'F12') {
                    e.preventDefault();
                    dioxus.send('VIEW_SOURCE');
                }
            });
            await new Promise(() => {});
            "#,
        );

        loop {
            match eval.recv::<String>().await {
                Ok(key) if key == "F5" => on_f5(tab_manager, backend, schema_info).await,
                Ok(key) if key == "CLOSE_TAB" => on_close_tab(tab_manager),
                Ok(key) if key == "NEW_EDITOR_TAB" => on_new_editor_tab(tab_manager, backend),
                Ok(key) if key == "NEXT_TAB" => on_next_tab(tab_manager),
                Ok(key) if key == "PREV_TAB" => on_prev_tab(tab_manager),
                Ok(key) if key == "OPEN_SEARCH" => on_open_search(show_search_modal, backend),
                Ok(key) if key == "VIEW_SOURCE" => on_view_source(tab_manager),
                _ => break,
            }
        }
    });
}

fn on_close_tab(mut tab_manager: Signal<TabManager>) {
    let active_tab_id = {
        let tm = tab_manager.read();
        tm.active_tab_id()
    };

    if let Some(id) = active_tab_id {
        tab_manager.write().close_tab(id);
    }
}

fn on_new_editor_tab(
    mut tab_manager: Signal<TabManager>,
    backend: Signal<Option<Arc<dyn DbBackend>>>,
) {
    if backend.read().is_some() {
        tab_manager.write().open_sql_editor();
    }
}

fn on_next_tab(mut tab_manager: Signal<TabManager>) {
    tab_manager.write().activate_next_tab();
}

fn on_prev_tab(mut tab_manager: Signal<TabManager>) {
    tab_manager.write().activate_previous_tab();
}

fn on_open_search(
    mut show_search_modal: Signal<bool>,
    backend: Signal<Option<Arc<dyn DbBackend>>>,
) {
    if backend.read().is_some() {
        show_search_modal.set(true);
    }
}

fn on_view_source(mut tab_manager: Signal<TabManager>) {
    let active = {
        let tm = tab_manager.read();
        tm.active_tab().map(|t| (t.id, t.tab_type.clone()))
    };

    if let Some((_id, TabType::TableBrowser { object_name, object_type, .. })) = active {
        if object_type == ObjectType::View {
            let (schema, name) = split_qualified_name(&object_name);
            tab_manager.write().open_view_source(name, schema);
        }
    }
}

async fn on_f5(
    mut tab_manager: Signal<TabManager>,
    backend: Signal<Option<Arc<dyn DbBackend>>>,
    mut schema_info: Signal<Option<SchemaInfo>>,
) {
    let active = {
        let tm = tab_manager.read();
        tm.active_tab().map(|t| (t.id, t.tab_type.clone()))
    };

    match active {
        Some((id, TabType::TableBrowser { .. })) => {
            tab_manager.write().reset_for_refresh(id);
        }
        Some((id, TabType::SqlEditor { sql_content })) => {
            let token = {
                let tm = tab_manager.read();
                tm.tab_by_id(id).map(|t| t.cancellation_token.clone())
            };
            if let Some(token) = token {
                if let Some(tab) = tab_manager.write().tab_by_id_mut(id) {
                    tab.is_loading = true;
                    tab.error = None;
                }
                let b = backend.read().as_ref().map(|b| b.clone());
                if let Some(b) = b {
                    tokio::select! {
                        result = tokio::time::timeout(OP_TIMEOUT, b.execute_query(&sql_content)) => {
                            if !token.is_cancelled() {
                                let ok = matches!(result, Ok(Ok(_)));
                                if let Some(tab) = tab_manager.write().tab_by_id_mut(id) {
                                    match result {
                                        Ok(Ok(r)) => { tab.result = Some(r); tab.error = None; }
                                        Ok(Err(e)) => { tab.error = Some(e.to_string()); tab.result = None; }
                                        Err(_) => {
                                            tab.error = Some(timeout_error_message("Query"));
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
                } else if let Some(tab) = tab_manager.write().tab_by_id_mut(id) {
                    tab.error = Some("Not connected to a database".into());
                    tab.is_loading = false;
                }
            }
        }
        _ => {}
    }
}
