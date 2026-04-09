use std::collections::BTreeMap;
use std::sync::Arc;

use app_core::connection_manager::ConnectionManager;
use app_core::tab_manager::TabManager;
use db::traits::DbBackend;
use db::types::{DbObject, ObjectType, SchemaInfo};
use dioxus::prelude::*;

#[css_module("/assets/styles/sidebar.css")]
struct Styles;

/// All objects belonging to a single schema, split by type.
#[derive(Clone, PartialEq)]
struct SchemaObjects {
    tables: Vec<DbObject>,
    views: Vec<DbObject>,
    triggers: Vec<DbObject>,
    functions: Vec<DbObject>,
}

/// Group all objects from SchemaInfo by schema name.
/// Pre-seeds the map with all known schema names so empty schemas get entries.
/// Returns (schema_name, SchemaObjects) pairs sorted by schema.
fn group_by_schema(info: &SchemaInfo) -> Vec<(String, SchemaObjects)> {
    let mut map: BTreeMap<String, SchemaObjects> = BTreeMap::new();

    // Pre-seed with all known schema names so empty schemas appear.
    for schema_name in &info.schemas {
        map.entry(schema_name.clone()).or_insert_with(|| SchemaObjects {
            tables: Vec::new(),
            views: Vec::new(),
            triggers: Vec::new(),
            functions: Vec::new(),
        });
    }

    let all_objects = info
        .tables
        .iter()
        .chain(&info.views)
        .chain(&info.triggers)
        .chain(&info.functions);

    for obj in all_objects {
        let key = obj.schema.clone().unwrap_or_default();
        let entry = map.entry(key).or_insert_with(|| SchemaObjects {
            tables: Vec::new(),
            views: Vec::new(),
            triggers: Vec::new(),
            functions: Vec::new(),
        });
        match obj.object_type {
            ObjectType::Table => entry.tables.push(obj.clone()),
            ObjectType::View => entry.views.push(obj.clone()),
            ObjectType::Trigger => entry.triggers.push(obj.clone()),
            ObjectType::Function => entry.functions.push(obj.clone()),
        }
    }

    map.into_iter().collect()
}

fn has_any_schema(info: &SchemaInfo) -> bool {
    info.tables.iter().any(|o| o.schema.is_some())
        || info.views.iter().any(|o| o.schema.is_some())
        || info.triggers.iter().any(|o| o.schema.is_some())
        || info.functions.iter().any(|o| o.schema.is_some())
}

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
    let mut functions_expanded = use_signal(|| true);
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
                if has_any_schema(schema) {
                    // Schema-grouped view (Postgres): schemas as root, object types nested
                    {
                        let groups = group_by_schema(schema);
                        rsx! {
                            for (schema_name, schema_objs) in groups {
                                SchemaSection {
                                    schema_name: schema_name,
                                    objects: schema_objs,
                                    tab_manager,
                                }
                            }
                        }
                    }
                } else if schema.tables.is_empty() && schema.views.is_empty() && schema.triggers.is_empty() && schema.functions.is_empty() {
                    div { class: Styles::sidebar_empty, "No schema objects found" }
                } else {
                    // Flat view (SQLite): object types as root, no schema grouping
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
                    if !schema.functions.is_empty() {
                        ObjectSection {
                            title: "Functions",
                            expanded: functions_expanded,
                            on_toggle: move |_| functions_expanded.toggle(),
                            objects: schema.functions.clone(),
                            tab_manager,
                        }
                    }
                }
            } else {
                div { class: Styles::sidebar_empty, "Loading schema..." }
            }
        }
    }
}

fn open_object_tab(tab_manager: &mut TabManager, obj: &DbObject) {
    match obj.object_type {
        ObjectType::Table | ObjectType::View => {
            tab_manager.open_table_browser(obj.name.clone(), obj.schema.clone());
        }
        ObjectType::Trigger => {
            tab_manager.open_trigger_view(obj.name.clone(), obj.schema.clone());
        }
        ObjectType::Function => {
            tab_manager.open_function_view(obj.name.clone(), obj.schema.clone());
        }
    }
}

/// A schema as root node, with object types nested inside.
#[component]
fn SchemaSection(
    schema_name: String,
    objects: SchemaObjects,
    tab_manager: Signal<TabManager>,
) -> Element {
    let mut expanded = use_signal(|| true);

    rsx! {
        div {
            div {
                class: Styles::section_header,
                onclick: move |_| expanded.toggle(),
                span { class: Styles::toggle,
                    if *expanded.read() { "v" } else { ">" }
                }
                "{schema_name}"
            }
            if *expanded.read() {
                if objects.tables.is_empty() && objects.views.is_empty() && objects.triggers.is_empty() && objects.functions.is_empty() {
                    div { class: Styles::schema_empty, "(no objects)" }
                } else {
                    if !objects.tables.is_empty() {
                        ObjectTypeGroup { title: "Tables", objects: objects.tables.clone(), tab_manager }
                    }
                    if !objects.views.is_empty() {
                        ObjectTypeGroup { title: "Views", objects: objects.views.clone(), tab_manager }
                    }
                    if !objects.triggers.is_empty() {
                        ObjectTypeGroup { title: "Triggers", objects: objects.triggers.clone(), tab_manager }
                    }
                    if !objects.functions.is_empty() {
                        ObjectTypeGroup { title: "Functions", objects: objects.functions.clone(), tab_manager }
                    }
                }
            }
        }
    }
}

/// An object type group nested inside a schema (e.g. "Tables (3)" under "public").
#[component]
fn ObjectTypeGroup(
    title: &'static str,
    objects: Vec<DbObject>,
    tab_manager: Signal<TabManager>,
) -> Element {
    let mut expanded = use_signal(|| true);
    let count = objects.len();

    rsx! {
        div {
            div {
                class: Styles::schema_header,
                onclick: move |_| expanded.toggle(),
                span { class: Styles::toggle,
                    if *expanded.read() { "v" } else { ">" }
                }
                "{title} ({count})"
            }
            if *expanded.read() {
                for obj in &objects {
                    {
                        let obj_clone = obj.clone();
                        rsx! {
                            div {
                                class: Styles::schema_object_item,
                                onclick: move |_| {
                                    open_object_tab(&mut tab_manager.write(), &obj_clone);
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

/// Flat object type section for databases without schemas (SQLite).
#[component]
fn ObjectSection(
    title: &'static str,
    expanded: Signal<bool>,
    on_toggle: EventHandler<()>,
    objects: Vec<DbObject>,
    tab_manager: Signal<TabManager>,
) -> Element {
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
                for obj in &objects {
                    {
                        let obj_clone = obj.clone();
                        rsx! {
                            div {
                                class: Styles::object_item,
                                onclick: move |_| {
                                    open_object_tab(&mut tab_manager.write(), &obj_clone);
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
