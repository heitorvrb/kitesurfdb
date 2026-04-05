use std::sync::Arc;

use app_core::tab_manager::TabManager;
use db::sqlite::SqliteBackend;
use db::types::SchemaInfo;
use dioxus::prelude::*;

use super::tab_bar::TabBar;
use super::tab_container::TabContainer;

#[css_module("/assets/styles/editor_area.css")]
struct Styles;

#[component]
pub fn EditorArea(
    tab_manager: Signal<TabManager>,
    backend: Signal<Option<Arc<SqliteBackend>>>,
    schema_info: Signal<Option<SchemaInfo>>,
) -> Element {
    rsx! {
        div { class: Styles::editor_area,
            TabBar { tab_manager }
            TabContainer { tab_manager, backend, schema_info }
        }
    }
}
