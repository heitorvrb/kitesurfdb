use std::sync::Arc;

use app_core::config::Theme;
use app_core::tab_manager::{TabManager, TabType};
use db::traits::DbBackend;
use db::types::SchemaInfo;
use dioxus::prelude::*;

use super::definition_view::DefinitionView;
use super::sql_editor::SqlEditor;
use super::table_browser::TableBrowser;

#[component]
pub fn TabContainer(
    tab_manager: Signal<TabManager>,
    backend: Signal<Option<Arc<dyn DbBackend>>>,
    schema_info: Signal<Option<SchemaInfo>>,
    theme: Signal<Theme>,
) -> Element {
    let active_info = {
        let tm = tab_manager.read();
        tm.active_tab()
            .map(|t| (t.id, t.tab_type.clone()))
    };

    rsx! {
        match active_info {
            Some((id, TabType::SqlEditor { .. })) => rsx! {
                SqlEditor {
                    key: "{id}",
                    tab_id: id,
                    tab_manager,
                    backend,
                    schema_info,
                    theme,
                }
            },
            Some((id, TabType::TableBrowser { .. })) => rsx! {
                TableBrowser {
                    key: "{id}",
                    tab_id: id,
                    tab_manager,
                    backend,
                    schema_info,
                }
            },
            Some((id, TabType::TriggerView { .. })) | Some((id, TabType::FunctionView { .. })) => rsx! {
                DefinitionView {
                    key: "{id}",
                    tab_id: id,
                    tab_manager,
                    backend,
                }
            },
            None => rsx! {
                div {
                    style: "flex:1;display:flex;align-items:center;justify-content:center;color:var(--text-secondary);font-size:14px;",
                    "Open a tab to get started"
                }
            },
        }
    }
}
