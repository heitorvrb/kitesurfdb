use std::sync::Arc;

use app_core::tab_manager::TabManager;
use db::sqlite::SqliteBackend;
use db::types::SchemaInfo;
use dioxus::prelude::*;

use crate::components::{ConnectionBar, EditorArea, Sidebar};

#[css_module("/assets/styles/main.css")]
struct Styles;

#[component]
pub fn App() -> Element {
    let backend: Signal<Option<Arc<SqliteBackend>>> = use_signal(|| None);
    let is_connected = use_signal(|| false);
    let tab_manager: Signal<TabManager> = use_signal(TabManager::new);
    let schema_info: Signal<Option<SchemaInfo>> = use_signal(|| None);

    rsx! {
        div { class: Styles::app,
            ConnectionBar {
                backend,
                is_connected,
                tab_manager,
                schema_info,
            }
            div { class: Styles::main_layout,
                Sidebar {
                    schema_info,
                    tab_manager,
                    is_connected,
                    backend,
                }
                EditorArea {
                  tab_manager, 
                  backend,
                  schema_info
                }
            }
        }
    }
}
