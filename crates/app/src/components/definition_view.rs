use std::sync::Arc;
use std::time::{Duration, Instant};

use app_core::tab_manager::{TabManager, TabType};
use db::traits::DbBackend;
use db::types::ObjectType;
use dioxus::prelude::*;
use uuid::Uuid;
use crate::operation_feedback::{OP_TIMEOUT, SLOW_WARNING_MS, slow_warning_message, timeout_error_message};

#[css_module("/assets/styles/definition_view.css")]
struct Styles;

#[component]
pub fn DefinitionView(
    tab_id: Uuid,
    tab_manager: Signal<TabManager>,
    backend: Signal<Option<Arc<dyn DbBackend>>>,
) -> Element {
    let mut tab_manager = tab_manager;
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

    // Auto-fetch definition on mount if not yet loaded
    use_effect(move || {
        let info = {
            let tm = tab_manager.read();
            tm.tab_by_id(tab_id).and_then(|t| match &t.tab_type {
                TabType::TriggerView {
                    object_name,
                    definition,
                } => Some((
                    object_name.clone(),
                    definition.clone(),
                    ObjectType::Trigger,
                    t.error.clone(),
                )),
                TabType::FunctionView {
                    object_name,
                    definition,
                } => Some((
                    object_name.clone(),
                    definition.clone(),
                    ObjectType::Function,
                    t.error.clone(),
                )),
                _ => None,
            })
        };

        if let Some((object_name, definition, object_type, error)) = info {
            if definition.is_some() || error.is_some() {
                return;
            }

            // Parse name and schema from qualified name
            let (schema, name) = if let Some((s, n)) = object_name.split_once('.') {
                (Some(s.to_string()), n.to_string())
            } else {
                (None, object_name.clone())
            };

            let token = {
                let tm = tab_manager.read();
                tm.tab_by_id(tab_id)
                    .map(|t| t.cancellation_token.clone())
            };

            if let Some(token) = token {
                if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
                    tab.is_loading = true;
                }

                spawn(async move {
                    if let Some(b) = backend.read().as_ref() {
                        let b = b.clone();
                        tokio::select! {
                            result = tokio::time::timeout(OP_TIMEOUT, b.get_object_definition(&name, schema.as_deref(), &object_type)) => {
                                if !token.is_cancelled() {
                                    if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
                                        match result {
                                            Ok(Ok(def)) => {
                                                match &mut tab.tab_type {
                                                    TabType::TriggerView { definition, .. } => {
                                                        *definition = Some(def);
                                                    }
                                                    TabType::FunctionView { definition, .. } => {
                                                        *definition = Some(def);
                                                    }
                                                    _ => {}
                                                }
                                                tab.error = None;
                                            }
                                            Ok(Err(e)) => {
                                                tab.error = Some(e.to_string());
                                            }
                                            Err(_) => {
                                                tab.error = Some(timeout_error_message("Request"));
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
        }
    });

    let (object_name, definition, error, is_loading, label) = {
        let tm = tab_manager.read();
        let tab = tm.tab_by_id(tab_id);
        match tab {
            Some(t) => {
                let (name, def, label) = match &t.tab_type {
                    TabType::TriggerView {
                        object_name,
                        definition,
                    } => (object_name.clone(), definition.clone(), "Trigger"),
                    TabType::FunctionView {
                        object_name,
                        definition,
                    } => (object_name.clone(), definition.clone(), "Function"),
                    _ => (String::new(), None, ""),
                };
                (name, def, t.error.clone(), t.is_loading, label)
            }
            None => (String::new(), None, None, false, ""),
        }
    };

    let elapsed_ms = *loading_elapsed_ms.read();
    let elapsed_secs = elapsed_ms as f64 / 1000.0;
    let taking_longer = is_loading && elapsed_ms >= SLOW_WARNING_MS;

    rsx! {
        div { class: Styles::definition_view,
            div { class: Styles::definition_header,
                span { "{label}: {object_name}" }
            }
            if is_loading {
                div { class: Styles::loading, "Loading definition... {elapsed_secs:.1}s" }
            }
            if taking_longer {
                div { class: Styles::slow_warning,
                    "{slow_warning_message()}"
                }
            }
            if let Some(err) = &error {
                div { class: "error", "{err}" }
            }
            if let Some(def) = &definition {
                pre { class: Styles::definition_content, "{def}" }
            }
        }
    }
}
