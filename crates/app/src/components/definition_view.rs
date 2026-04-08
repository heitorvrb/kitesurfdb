use std::sync::Arc;

use app_core::tab_manager::{TabManager, TabType};
use db::traits::DbBackend;
use db::types::ObjectType;
use dioxus::prelude::*;
use uuid::Uuid;

#[css_module("/assets/styles/definition_view.css")]
struct Styles;

#[component]
pub fn DefinitionView(
    tab_id: Uuid,
    tab_manager: Signal<TabManager>,
    backend: Signal<Option<Arc<dyn DbBackend>>>,
) -> Element {
    let mut tab_manager = tab_manager;

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
                )),
                TabType::FunctionView {
                    object_name,
                    definition,
                } => Some((
                    object_name.clone(),
                    definition.clone(),
                    ObjectType::Function,
                )),
                _ => None,
            })
        };

        if let Some((object_name, definition, object_type)) = info {
            if definition.is_some() {
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
                            result = b.get_object_definition(&name, schema.as_deref(), &object_type) => {
                                if !token.is_cancelled() {
                                    if let Some(tab) = tab_manager.write().tab_by_id_mut(tab_id) {
                                        match result {
                                            Ok(def) => {
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

    rsx! {
        div { class: Styles::definition_view,
            div { class: Styles::definition_header,
                span { "{label}: {object_name}" }
            }
            if is_loading {
                div { class: Styles::loading, "Loading definition..." }
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
