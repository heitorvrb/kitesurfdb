use app_core::tab_manager::{ColumnOrderInfo, SortDirection};
use db::types::{DbValue, ForeignKeyInfo, QueryResult};
use dioxus::prelude::*;
use std::collections::BTreeMap;

#[css_module("/assets/styles/results_panel.css")]
struct Styles;

#[derive(Clone, PartialEq, Debug)]
pub struct CellEdit {
    pub row: usize,
    pub column: String,
    /// `Some(s)` to record the new value; `None` to clear a pending edit
    /// (e.g. user reverted the cell back to its original value).
    pub new_value: Option<String>,
}

/// Emitted when the user CTRL+clicks a foreign-key cell.
#[derive(Clone, PartialEq, Debug)]
pub struct FkJump {
    pub fk: ForeignKeyInfo,
    pub value: DbValue,
}

fn normalize_column_key(name: &str) -> String {
    let trimmed = name.trim();
    let unquoted = if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    unquoted.to_ascii_lowercase()
}

fn is_editable_value(value: &DbValue) -> bool {
    !matches!(value, DbValue::Bytes(_))
}

const EDIT_INPUT_ID: &str = "cell-edit-input";

#[component]
pub fn ResultsPanel(
    result: Option<QueryResult>,
    error: Option<String>,
    #[props(default)] total_count: Option<u64>,
    #[props(default)] on_refresh: Option<EventHandler>,
    #[props(default)] on_sort_column: Option<EventHandler<String>>,
    #[props(default)] ordering: Vec<ColumnOrderInfo>,
    #[props(default = false)] editable: bool,
    #[props(default)] edited_cells: BTreeMap<usize, BTreeMap<String, String>>,
    #[props(default)] on_cell_edit: Option<EventHandler<CellEdit>>,
    #[props(default)] on_save: Option<EventHandler>,
    #[props(default)] foreign_keys: Vec<ForeignKeyInfo>,
    #[props(default)] on_fk_jump: Option<EventHandler<FkJump>>,
    children: Element,
) -> Element {
    let mut editing: Signal<Option<(usize, String)>> = use_signal(|| None);
    let mut edit_text: Signal<String> = use_signal(String::new);

    // Focus the input whenever a cell enters edit mode.
    use_effect(move || {
        if editing.read().is_some() {
            document::eval(&format!(
                r#"
                requestAnimationFrame(() => {{
                    const el = document.getElementById('{EDIT_INPUT_ID}');
                    if (el) {{ el.focus(); el.select(); }}
                }});
                "#
            ));
        }
    });

    let pending_count: usize = edited_cells.values().map(|r| r.len()).sum();
    let edited_rows = edited_cells.len();

    rsx! {
        div { class: Styles::results_panel,
            if let Some(err) = &error {
                div { class: "error", "{err}" }
            }
            if let Some(result) = &result {
                div { class: Styles::result_info,
                    if let Some(total) = total_count {
                        "Rows: {result.rows.len()} | Total: {total} | Time: {result.execution_time:?}"
                    } else {
                        "Rows: {result.rows.len()} | Time: {result.execution_time:?}"
                    }
                    if let Some(handler) = on_refresh {
                        button {
                            class: Styles::refresh_btn,
                            onclick: move |_| handler.call(()),
                            "↻ Refresh"
                        }
                    }
                    if pending_count > 0 {
                        if let Some(save_handler) = on_save.clone() {
                            span { class: Styles::edits_pending,
                                "{pending_count} pending edit(s) across {edited_rows} row(s)"
                            }
                            button {
                                class: Styles::save_btn,
                                onclick: move |_| save_handler.call(()),
                                "Save ({edited_rows})"
                            }
                        }
                    }
                }
            }
            if let Some(result) = &result {
                if !result.columns.is_empty() {
                    table { class: Styles::result_table,
                        thead {
                            tr {
                                for col in &result.columns {
                                    {
                                        let column_name = col.name.clone();
                                        let sort_handler = on_sort_column.clone();
                                        let column_key = normalize_column_key(&col.name);
                                        let sort_info = ordering.iter().find(|o| o.column_key == column_key);
                                        let arrow = sort_info.map(|info| {
                                            if info.direction == SortDirection::Asc { "▲" } else { "▼" }
                                        });
                                        let show_precedence = ordering.len() > 1;
                                        rsx! {
                                            th {
                                                class: if sort_handler.is_some() { Styles::sortable_header.to_string() } else { String::new() },
                                                onclick: move |_| {
                                                    if let Some(handler) = sort_handler.as_ref() {
                                                        handler.call(column_name.clone());
                                                    }
                                                },
                                                span { "{col.name}" }
                                                if let Some(a) = arrow {
                                                    span { class: Styles::sort_indicator,
                                                        " {a}"
                                                        if show_precedence {
                                                            if let Some(info) = sort_info {
                                                                "{info.precedence}"
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        tbody {
                            for (row_idx, row) in result.rows.iter().enumerate() {
                                tr {
                                    for (col_idx, cell) in row.iter().enumerate() {
                                        {
                                            let col_name = result.columns[col_idx].name.clone();
                                            let is_editing = editing.read().as_ref()
                                                .map(|(r, c)| *r == row_idx && c == &col_name)
                                                .unwrap_or(false);
                                            let edited_value = edited_cells
                                                .get(&row_idx)
                                                .and_then(|cols| cols.get(&col_name))
                                                .cloned();
                                            let is_edited = edited_value.is_some();
                                            let cell_can_be_edited = editable && is_editable_value(cell);

                                            let display_text = match &edited_value {
                                                Some(v) => v.clone(),
                                                None => cell.to_string(),
                                            };
                                            let is_null_display = edited_value.is_none()
                                                && *cell == DbValue::Null;

                                            let fk_for_cell: Option<ForeignKeyInfo> = foreign_keys
                                                .iter()
                                                .find(|fk| fk.from_column == col_name)
                                                .cloned();
                                            let is_fk_jumpable = fk_for_cell.is_some()
                                                && !is_null_display
                                                && on_fk_jump.is_some();

                                            let mut td_classes: Vec<String> = Vec::new();
                                            if is_null_display { td_classes.push(Styles::null_value.to_string()); }
                                            if is_edited { td_classes.push(Styles::edited_cell.to_string()); }
                                            if cell_can_be_edited { td_classes.push(Styles::editable_cell.to_string()); }
                                            if is_fk_jumpable { td_classes.push(Styles::fk_cell.to_string()); }
                                            let td_class = td_classes.join(" ");

                                            let title_text = if is_fk_jumpable {
                                                fk_for_cell
                                                    .as_ref()
                                                    .map(|fk| format!("Ctrl+click to open {}", fk.to_table))
                                                    .unwrap_or_default()
                                            } else {
                                                String::new()
                                            };

                                            let original_value = cell.clone();
                                            let col_for_dblclick = col_name.clone();
                                            let original_for_dblclick = original_value.clone();
                                            let edited_for_dblclick = edited_value.clone();

                                            let original_for_commit = original_value.clone();
                                            let col_for_commit = col_name.clone();
                                            let handler_for_commit = on_cell_edit.clone();

                                            let original_for_blur = original_value.clone();
                                            let col_for_blur = col_name.clone();
                                            let handler_for_blur = on_cell_edit.clone();

                                            let fk_for_click = fk_for_cell.clone();
                                            let value_for_click = original_value.clone();
                                            let handler_for_fk = on_fk_jump.clone();

                                            rsx! {
                                                td {
                                                    class: "{td_class}",
                                                    title: "{title_text}",
                                                    onclick: move |evt: MouseEvent| {
                                                        if !(evt.modifiers().ctrl() || evt.modifiers().meta()) {
                                                            return;
                                                        }
                                                        let Some(fk) = fk_for_click.clone() else { return };
                                                        if matches!(value_for_click, DbValue::Null) { return; }
                                                        let Some(handler) = handler_for_fk.as_ref() else { return };
                                                        evt.prevent_default();
                                                        evt.stop_propagation();
                                                        handler.call(FkJump {
                                                            fk,
                                                            value: value_for_click.clone(),
                                                        });
                                                    },
                                                    ondoubleclick: move |_| {
                                                        if !cell_can_be_edited { return; }
                                                        let seed = match &edited_for_dblclick {
                                                            Some(v) => v.clone(),
                                                            None => original_for_dblclick.to_string(),
                                                        };
                                                        edit_text.set(seed);
                                                        editing.set(Some((row_idx, col_for_dblclick.clone())));
                                                    },
                                                    if is_editing {
                                                        input {
                                                            id: "{EDIT_INPUT_ID}",
                                                            class: Styles::cell_edit_input,
                                                            value: "{edit_text.read()}",
                                                            oninput: move |evt| edit_text.set(evt.value()),
                                                            onkeydown: move |evt: KeyboardEvent| {
                                                                match evt.key() {
                                                                    Key::Enter => {
                                                                        evt.prevent_default();
                                                                        let new_text = edit_text.read().clone();
                                                                        let original_text = original_for_commit.to_string();
                                                                        let new_value = if new_text == original_text {
                                                                            None
                                                                        } else {
                                                                            Some(new_text)
                                                                        };
                                                                        if let Some(h) = handler_for_commit.as_ref() {
                                                                            h.call(CellEdit {
                                                                                row: row_idx,
                                                                                column: col_for_commit.clone(),
                                                                                new_value,
                                                                            });
                                                                        }
                                                                        editing.set(None);
                                                                    }
                                                                    Key::Escape => {
                                                                        evt.prevent_default();
                                                                        editing.set(None);
                                                                    }
                                                                    _ => {}
                                                                }
                                                            },
                                                            onblur: move |_| {
                                                                let new_text = edit_text.read().clone();
                                                                let original_text = original_for_blur.to_string();
                                                                let new_value = if new_text == original_text {
                                                                    None
                                                                } else {
                                                                    Some(new_text)
                                                                };
                                                                if let Some(h) = handler_for_blur.as_ref() {
                                                                    h.call(CellEdit {
                                                                        row: row_idx,
                                                                        column: col_for_blur.clone(),
                                                                        new_value,
                                                                    });
                                                                }
                                                                editing.set(None);
                                                            },
                                                        }
                                                    } else {
                                                        "{display_text}"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            {children}
        }
    }
}
