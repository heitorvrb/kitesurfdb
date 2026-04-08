use std::sync::Arc;

use app_core::config::{self, Theme};
use app_core::connection_manager::ConnectionManager;
use app_core::tab_manager::TabManager;
use db::traits::DbBackend;
use db::types::SchemaInfo;
use dioxus::prelude::*;

use crate::components::{ConnectionBar, EditorArea, Sidebar};

#[css_module("/assets/styles/main.css")]
struct Styles;

#[component]
pub fn App() -> Element {
    let backend: Signal<Option<Arc<dyn DbBackend>>> = use_signal(|| None);
    let is_connected = use_signal(|| false);
    let tab_manager: Signal<TabManager> = use_signal(TabManager::new);
    let schema_info: Signal<Option<SchemaInfo>> = use_signal(|| None);
    let connection_manager: Signal<ConnectionManager> = use_signal(ConnectionManager::new);
    let theme: Signal<Theme> = use_signal(|| config::load_preferences().theme);

    let theme_str = theme.read().as_str();

    rsx! {
        div {
            class: Styles::app,
            "data-theme": theme_str,
            ConnectionBar {
                backend,
                is_connected,
                tab_manager,
                schema_info,
                connection_manager,
                theme,
            }
            div { class: Styles::main_layout,
                Sidebar {
                    schema_info,
                    tab_manager,
                    is_connected,
                    backend,
                    connection_manager,
                }
                EditorArea {
                  tab_manager,
                  backend,
                  schema_info,
                  theme
                }
            }
        }
    }
}
