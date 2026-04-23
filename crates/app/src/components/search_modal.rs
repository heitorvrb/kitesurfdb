use app_core::tab_manager::TabManager;
use db::types::{DbObject, ObjectType, SchemaInfo};
use dioxus::prelude::*;

#[css_module("/assets/styles/search_modal.css")]
struct Styles;

#[component]
pub fn SearchModal(
    show_search_modal: Signal<bool>,
    schema_info: Signal<Option<SchemaInfo>>,
    tab_manager: Signal<TabManager>,
    is_connected: Signal<bool>,
) -> Element {
    let mut query = use_signal(String::new);
    let mut selected_key = use_signal(String::new);

    use_effect(move || {
        if *show_search_modal.read() {
            document::eval(
                r#"
                requestAnimationFrame(() => {
                    const el = document.getElementById('object-search-input');
                    if (el) {
                        el.focus();
                        el.select();
                    }
                });
                "#,
            );
        }
    });

    let objects = schema_info
        .read()
        .as_ref()
        .map(all_objects)
        .unwrap_or_default();

    let q = query.read().trim().to_lowercase();
    let results: Vec<DbObject> = if q.is_empty() {
        Vec::new()
    } else {
        objects
            .into_iter()
            .filter(|obj| {
                let name = obj.name.to_lowercase();
                let qualified = qualified_name(obj).to_lowercase();
                matches_pattern(&name, &q) || matches_pattern(&qualified, &q)
            })
            .take(200)
            .collect()
    };

    let results_for_submit = results.clone();
    let results_for_keys = results.clone();

    let results_for_effect = results.clone();
    use_effect(move || {
        let current = selected_key.read().clone();
        let has_current = results_for_effect
            .iter()
            .any(|obj| object_key(obj) == current);
        if !has_current {
            if let Some(first) = results_for_effect.first() {
                selected_key.set(object_key(first));
            } else {
                selected_key.set(String::new());
            }
        }
    });

    let selected_key_value = selected_key.read().clone();
    use_effect(move || {
        selected_key.read();
        scroll_selected_result_into_view();
    });

    rsx! {
        div {
            class: Styles::overlay,
            onclick: move |_| show_search_modal.set(false),
            onkeydown: move |evt: KeyboardEvent| {
                if evt.key() == Key::Escape {
                    evt.prevent_default();
                    show_search_modal.set(false);
                }
            },
            div {
                class: Styles::dialog,
                onclick: move |evt| evt.stop_propagation(),
                div { class: Styles::header,
                    h2 { "Search database objects" }
                    button {
                        class: Styles::close_btn,
                        onclick: move |_| show_search_modal.set(false),
                        "x"
                    }
                }
                div { class: Styles::shortcut_hint, "Shortcut: Ctrl+F" }
                input {
                    id: "object-search-input",
                    class: Styles::search_input,
                    value: "{query}",
                    placeholder: "Type a table, view, trigger, or function name",
                    oninput: move |evt| query.set(evt.value()),
                    onkeydown: move |evt: KeyboardEvent| {
                        if evt.key() == Key::Enter {
                            evt.prevent_default();
                            let current = selected_key.read().clone();
                            let selected = selected_result(&results, current.as_str()).or_else(|| results.first());
                            if let Some(selected) = selected {
                                open_object(&mut tab_manager.write(), selected);
                                show_search_modal.set(false);
                            }
                        }
                    },
                }

                if !*is_connected.read() {
                    div { class: Styles::empty_state, "Connect to a database to search objects" }
                } else if query.read().trim().is_empty() {
                    div { class: Styles::empty_state, "Start typing to search by object name" }
                } else if results.is_empty() {
                    div { class: Styles::empty_state, "No matching objects found" }
                } else {
                    form {
                        class: Styles::results,
                        onsubmit: move |evt| {
                            evt.prevent_default();
                            if let Some(selected) = selected_result(&results_for_submit, selected_key.read().as_str()) {
                                open_object(&mut tab_manager.write(), selected);
                                show_search_modal.set(false);
                            }
                        },
                        div {
                            id: "object-search-results",
                            class: Styles::results_list,
                            role: "listbox",
                            tabindex: "0",
                            onkeydown: move |evt: KeyboardEvent| {
                                let key = evt.key();
                                let direction = match key {
                                    Key::ArrowDown => Some(1),
                                    Key::ArrowUp => Some(-1),
                                    Key::Character(ref value) if value == "j" => Some(1),
                                    Key::Character(ref value) if value == "k" => Some(-1),
                                    _ => None,
                                };

                                if let Some(direction) = direction {
                                    evt.prevent_default();
                                    let current = selected_key.read().clone();
                                    let current_index = selected_result_index(&results_for_keys, &current).unwrap_or(0);
                                    let next_index = if direction > 0 {
                                        (current_index + 1).min(results_for_keys.len().saturating_sub(1))
                                    } else {
                                        current_index.saturating_sub(1)
                                    };
                                    if let Some(next) = results_for_keys.get(next_index) {
                                        selected_key.set(object_key(next));
                                    }
                                } else if key == Key::Enter {
                                    evt.prevent_default();
                                    let current = selected_key.read().clone();
                                    if let Some(selected) = selected_result(&results_for_keys, current.as_str()) {
                                        open_object(&mut tab_manager.write(), selected);
                                        show_search_modal.set(false);
                                    }
                                }
                            },
                            for obj in &results {
                                button {
                                    key: "{object_key(obj)}",
                                    class: if object_key(obj) == selected_key_value.as_str() {
                                        format!("{} {}", Styles::result_item, Styles::result_item_selected)
                                    } else {
                                        Styles::result_item.to_string()
                                    },
                                    r#type: "button",
                                    role: "option",
                                    aria_selected: "{object_key(obj) == selected_key_value.as_str()}",
                                    tabindex: "-1",
                                    onclick: {
                                        let key = object_key(obj);
                                        move |_| selected_key.set(key.clone())
                                    },
                                    ondoubleclick: {
                                        let obj = obj.clone();
                                        move |_| {
                                            open_object(&mut tab_manager.write(), &obj);
                                            show_search_modal.set(false);
                                        }
                                    },
                                    "{qualified_name(obj)} [{object_type_label(&obj.object_type)}]"
                                }
                            }
                        }
                        div { class: Styles::actions,
                            button {
                                class: Styles::open_btn,
                                r#type: "submit",
                                "Open"
                            }
                        }
                    }
                }
                div {
                    class: Styles::wildcard_help,
                    "Supported wildcards: *, $."
                }
            }
        }
    }
}

fn matches_pattern(candidate: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }

    let ends_with = pattern.ends_with('$');
    let pattern = if ends_with {
        &pattern[..pattern.len() - 1]
    } else {
        pattern
    };

    if pattern.is_empty() {
        return true;
    }

    let parts: Vec<&str> = pattern.split('*').collect();

    if parts.len() == 1 {
        return if ends_with {
            candidate.ends_with(parts[0])
        } else {
            candidate.contains(parts[0])
        };
    }

    let mut pos = 0usize;
    for part in &parts {
        if part.is_empty() {
            continue;
        }
        let Some(found_at) = candidate[pos..].find(part) else {
            return false;
        };
        pos += found_at + part.len();
    }

    if ends_with {
        if let Some(last) = parts.iter().rev().find(|p| !p.is_empty()) {
            candidate.ends_with(last)
        } else {
            true
        }
    } else {
        true
    }
}

fn selected_result<'a>(results: &'a [DbObject], key: &str) -> Option<&'a DbObject> {
    results.iter().find(|obj| object_key(obj) == key)
}

fn selected_result_index(results: &[DbObject], key: &str) -> Option<usize> {
    results.iter().position(|obj| object_key(obj) == key)
}

fn scroll_selected_result_into_view() {
    document::eval(
        r#"
        requestAnimationFrame(() => {
            const selected = document.querySelector('#object-search-results [aria-selected="true"]');
            if (selected) {
                selected.scrollIntoView({ block: 'nearest' });
            }
        });
        "#,
    );
}

fn object_key(obj: &DbObject) -> String {
    format!(
        "{}|{}|{}",
        obj.schema.as_deref().unwrap_or(""),
        object_type_label(&obj.object_type),
        obj.name,
    )
}

fn open_object(tab_manager: &mut TabManager, obj: &DbObject) {
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

fn all_objects(info: &SchemaInfo) -> Vec<DbObject> {
    let mut objects = info
        .tables
        .iter()
        .chain(&info.views)
        .chain(&info.triggers)
        .chain(&info.functions)
        .cloned()
        .collect::<Vec<_>>();

    objects.sort_by(|a, b| {
        let a_key = (
            a.schema.as_deref().unwrap_or(""),
            object_type_label(&a.object_type),
            a.name.as_str(),
        );
        let b_key = (
            b.schema.as_deref().unwrap_or(""),
            object_type_label(&b.object_type),
            b.name.as_str(),
        );
        a_key.cmp(&b_key)
    });

    objects
}

fn qualified_name(obj: &DbObject) -> String {
    match &obj.schema {
        Some(schema) if !schema.is_empty() => format!("{schema}.{}", obj.name),
        _ => obj.name.clone(),
    }
}

fn object_type_label(object_type: &ObjectType) -> &'static str {
    match object_type {
        ObjectType::Table => "Table",
        ObjectType::View => "View",
        ObjectType::Trigger => "Trigger",
        ObjectType::Function => "Function",
    }
}
