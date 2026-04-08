use std::collections::BTreeMap;
use std::sync::Arc;

use app_core::connection_manager::ConnectionManager;
use app_core::tab_manager::TabManager;
use db::traits::DbBackend;
use db::types::{DbObject, SchemaInfo};
use dioxus::prelude::*;

#[css_module("/assets/styles/sidebar.css")]
struct Styles;

#[component]
pub fn Sidebar(
    schema_info: Signal<Option<SchemaInfo>>,
    tab_manager: Signal<TabManager>,
    is_connected: Signal<bool>,
    backend: Signal<Option<Arc<dyn DbBackend>>>,
    connection_manager: Signal<ConnectionManager>,
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
                        objects: schema.tables.clone(),
                        tab_manager,
                    }
                }
                if !schema.views.is_empty() {
                    ObjectSection {
                        title: "Views",
                        expanded: views_expanded,
                        on_toggle: move |_| views_expanded.toggle(),
                        objects: schema.views.clone(),
                        tab_manager,
                    }
                }
                if !schema.triggers.is_empty() {
                    ObjectSection {
                        title: "Triggers",
                        expanded: triggers_expanded,
                        on_toggle: move |_| triggers_expanded.toggle(),
                        objects: schema.triggers.clone(),
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

/// Group objects by schema. Returns (schema_name, objects) pairs sorted by schema.
/// Objects without a schema use "" as the key.
fn group_by_schema(objects: &[DbObject]) -> Vec<(String, Vec<DbObject>)> {
    let mut groups: BTreeMap<String, Vec<DbObject>> = BTreeMap::new();
    for obj in objects {
        let key = obj.schema.clone().unwrap_or_default();
        groups.entry(key).or_default().push(obj.clone());
    }
    groups.into_iter().collect()
}

#[component]
fn ObjectSection(
    title: &'static str,
    expanded: Signal<bool>,
    on_toggle: EventHandler<()>,
    objects: Vec<DbObject>,
    tab_manager: Signal<TabManager>,
) -> Element {
    let has_schemas = objects.iter().any(|o| o.schema.is_some());
    let groups = group_by_schema(&objects);
    let total = objects.len();

    rsx! {
        div {
            div {
                class: Styles::section_header,
                onclick: move |_| on_toggle.call(()),
                span { class: Styles::toggle,
                    if *expanded.read() { "v" } else { ">" }
                }
                "{title} ({total})"
            }
            if *expanded.read() {
                if has_schemas {
                    for (schema_name, schema_objects) in &groups {
                        SchemaGroup {
                            schema_name: schema_name.clone(),
                            objects: schema_objects.clone(),
                            tab_manager,
                        }
                    }
                } else {
                    for obj in &objects {
                        {
                            let name = obj.name.clone();
                            rsx! {
                                div {
                                    class: Styles::object_item,
                                    onclick: move |_| {
                                        tab_manager.write().open_table_browser(name.clone(), None);
                                    },
                                    "{obj.name}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn SchemaGroup(
    schema_name: String,
    objects: Vec<DbObject>,
    tab_manager: Signal<TabManager>,
) -> Element {
    let mut expanded = use_signal(|| true);

    rsx! {
        div {
            div {
                class: Styles::schema_header,
                onclick: move |_| expanded.toggle(),
                span { class: Styles::toggle,
                    if *expanded.read() { "v" } else { ">" }
                }
                "{schema_name} ({objects.len()})"
            }
            if *expanded.read() {
                for obj in &objects {
                    {
                        let name = obj.name.clone();
                        let schema = obj.schema.clone();
                        rsx! {
                            div {
                                class: Styles::schema_object_item,
                                onclick: move |_| {
                                    tab_manager.write().open_table_browser(name.clone(), schema.clone());
                                },
                                "{obj.name}"
                            }
                        }
                    }
                }
            }
        }
    }
}
