use std::sync::Arc;

use app_core::tab_manager::TabManager;
use db::sqlite::SqliteBackend;
use db::traits::DbBackend;
use db::types::SchemaInfo;
use dioxus::prelude::*;

#[css_module("/assets/styles/sidebar.css")]
struct Styles;

#[component]
pub fn Sidebar(
    schema_info: Signal<Option<SchemaInfo>>,
    tab_manager: Signal<TabManager>,
    is_connected: Signal<bool>,
    backend: Signal<Option<Arc<SqliteBackend>>>,
) -> Element {
    let mut tables_expanded = use_signal(|| true);
    let mut views_expanded = use_signal(|| true);
    let mut triggers_expanded = use_signal(|| true);
    let mut schema_info = schema_info;

    let refresh = move |_| {
        spawn(async move {
            if let Some(b) = backend.read().as_ref() {
                if let Ok(info) = b.introspect().await {
                    schema_info.set(Some(info));
                }
            }
        });
    };

    rsx! {
        div { class: Styles::sidebar,
            if !*is_connected.read() {
                div { class: Styles::sidebar_empty, "Connect to a database to browse schema" }
            } else if let Some(schema) = schema_info.read().as_ref() {
                div { class: Styles::sidebar_toolbar,
                    span { class: Styles::sidebar_title, "Schema" }
                    button {
                        class: Styles::refresh_btn,
                        onclick: refresh,
                        "Refresh"
                    }
                }
                if !schema.tables.is_empty() {
                    ObjectSection {
                        title: "Tables",
                        expanded: tables_expanded,
                        on_toggle: move |_| tables_expanded.toggle(),
                        objects: schema.tables.iter().map(|o| o.name.clone()).collect(),
                        tab_manager,
                    }
                }
                if !schema.views.is_empty() {
                    ObjectSection {
                        title: "Views",
                        expanded: views_expanded,
                        on_toggle: move |_| views_expanded.toggle(),
                        objects: schema.views.iter().map(|o| o.name.clone()).collect(),
                        tab_manager,
                    }
                }
                if !schema.triggers.is_empty() {
                    ObjectSection {
                        title: "Triggers",
                        expanded: triggers_expanded,
                        on_toggle: move |_| triggers_expanded.toggle(),
                        objects: schema.triggers.iter().map(|o| o.name.clone()).collect(),
                        tab_manager,
                    }
                }
                if schema.tables.is_empty() && schema.views.is_empty() && schema.triggers.is_empty() {
                    div { class: Styles::sidebar_empty, "No schema objects found" }
                }
            } else {
                div { class: Styles::sidebar_empty, "Loading schema..." }
            }
        }
    }
}

#[component]
fn ObjectSection(
    title: &'static str,
    expanded: Signal<bool>,
    on_toggle: EventHandler<()>,
    objects: Vec<String>,
    tab_manager: Signal<TabManager>,
) -> Element {
    rsx! {
        div {
            div {
                class: Styles::section_header,
                onclick: move |_| on_toggle.call(()),
                span { class: Styles::toggle,
                    if *expanded.read() { "v" } else { ">" }
                }
                "{title} ({objects.len()})"
            }
            if *expanded.read() {
                for name in &objects {
                    {
                        let name_clone = name.clone();
                        rsx! {
                            div {
                                class: Styles::object_item,
                                onclick: move |_| {
                                    tab_manager.write().open_table_browser(name_clone.clone());
                                },
                                "{name}"
                            }
                        }
                    }
                }
            }
        }
    }
}
