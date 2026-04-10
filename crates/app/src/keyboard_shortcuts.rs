use std::sync::Arc;

use app_core::tab_manager::{TabManager, TabType};
use db::traits::DbBackend;
use db::types::SchemaInfo;
use dioxus::prelude::*;

/// Registers global keyboard shortcut handlers for the application.
///
/// Uses a document-level JS event listener (via eval) so shortcuts fire
/// regardless of which element has focus.
pub fn use_keyboard_shortcuts(
    tab_manager: Signal<TabManager>,
    backend: Signal<Option<Arc<dyn DbBackend>>>,
    schema_info: Signal<Option<SchemaInfo>>,
) {
    use_future(move || async move {
        let mut eval = document::eval(
            r#"
            document.addEventListener('keydown', function(e) {
                if (e.key === 'F5') {
                    e.preventDefault();
                    dioxus.send('F5');
                }
            });
            await new Promise(() => {});
            "#,
        );

        loop {
            match eval.recv::<String>().await {
                Ok(key) if key == "F5" => on_f5(tab_manager, backend, schema_info).await,
                _ => break,
            }
        }
    });
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
                        result = b.execute_query(&sql_content) => {
                            if !token.is_cancelled() {
                                let ok = result.is_ok();
                                if let Some(tab) = tab_manager.write().tab_by_id_mut(id) {
                                    match result {
                                        Ok(r) => { tab.result = Some(r); tab.error = None; }
                                        Err(e) => { tab.error = Some(e.to_string()); tab.result = None; }
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
            }
        }
        _ => {}
    }
}
