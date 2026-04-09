use std::sync::Arc;

use app_core::config::Theme;
use app_core::tab_manager::TabManager;
use db::traits::DbBackend;
use db::types::SchemaInfo;
use dioxus::prelude::*;

use super::tab_bar::TabBar;
use super::tab_container::TabContainer;

#[css_module("/assets/styles/editor_area.css")]
struct Styles;

#[component]
pub fn EditorArea(
    tab_manager: Signal<TabManager>,
    backend: Signal<Option<Arc<dyn DbBackend>>>,
    schema_info: Signal<Option<SchemaInfo>>,
    theme: Signal<Theme>,
    is_connected: Signal<bool>,
) -> Element {
    rsx! {
        div { class: Styles::editor_area,
            TabBar { tab_manager, is_connected }
            TabContainer { tab_manager, backend, schema_info, theme, is_connected }
        }
    }
}
