use db::types::{ForeignKeyInfo, ObjectType, QueryResult};
use std::collections::BTreeMap;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::sql_ordering::{cycle_order_by, parse_order_items, sort_column_key};
use crate::sql_where::{set_where_clause, strip_leading_where, validate_where_predicate};

pub use crate::sql_ordering::{ColumnOrderInfo, SortDirection};

pub const PAGE_SIZE: usize = 100;

#[derive(Debug, Clone, PartialEq)]
pub enum TabType {
    SqlEditor {
        sql_content: String,
    },
    TableBrowser {
        object_name: String,
        generated_sql: String,
        count_sql: String,
        object_type: ObjectType,
        where_clause: String,
        /// Cached list of primary key column names. `None` means we have not
        /// fetched it yet; `Some(empty)` means the table has no primary key.
        primary_keys: Option<Vec<String>>,
        /// Cached single-column FKs for this table. `None` = not fetched yet;
        /// `Some(empty)` = no FKs. Only populated for `object_type == Table`.
        foreign_keys: Option<Vec<ForeignKeyInfo>>,
        /// Pending edits the user has typed but not yet saved.
        /// Outer key is the row index in `Tab.result.rows`; inner key is the
        /// column name; value is the user's typed string for that cell.
        edited_cells: BTreeMap<usize, BTreeMap<String, String>>,
    },
    TriggerView {
        object_name: String,
        definition: Option<String>,
    },
    FunctionView {
        object_name: String,
        definition: Option<String>,
    },
    ViewSource {
        object_name: String,
        definition: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct Tab {
    pub id: Uuid,
    pub title: String,
    pub tab_type: TabType,
    pub result: Option<QueryResult>,
    pub error: Option<String>,
    pub is_loading: bool,
    pub cancellation_token: CancellationToken,
    pub total_count: Option<u64>,
}

pub struct TabManager {
    tabs: Vec<Tab>,
    active_tab_id: Option<Uuid>,
}

impl TabManager {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active_tab_id: None,
        }
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn active_tab_id(&self) -> Option<Uuid> {
        self.active_tab_id
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.active_tab_id
            .and_then(|id| self.tabs.iter().find(|t| t.id == id))
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.active_tab_id
            .and_then(|id| self.tabs.iter_mut().find(|t| t.id == id))
    }

    pub fn tab_by_id(&self, id: Uuid) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }

    pub fn tab_by_id_mut(&mut self, id: Uuid) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    /// Reset a tab's result state so it re-fetches on next render.
    /// Used by both the Refresh button and the F5 shortcut.
    pub fn reset_for_refresh(&mut self, id: Uuid) {
        if let Some(tab) = self.tab_by_id_mut(id) {
            tab.result = None;
            tab.total_count = None;
            tab.error = None;
            tab.is_loading = false;
            if let TabType::TableBrowser { edited_cells, .. } = &mut tab.tab_type {
                edited_cells.clear();
            }
        }
    }

    /// Record (or clear) a pending edit for a single cell. If `new_value` is
    /// `None`, any existing edit is removed. Returns `true` if the call mutated
    /// state.
    pub fn set_edited_cell(
        &mut self,
        id: Uuid,
        row_idx: usize,
        col_name: &str,
        new_value: Option<String>,
    ) -> bool {
        let Some(tab) = self.tab_by_id_mut(id) else {
            return false;
        };
        let TabType::TableBrowser { edited_cells, .. } = &mut tab.tab_type else {
            return false;
        };

        match new_value {
            Some(val) => {
                edited_cells
                    .entry(row_idx)
                    .or_default()
                    .insert(col_name.to_string(), val);
                true
            }
            None => {
                if let Some(row_edits) = edited_cells.get_mut(&row_idx) {
                    let removed = row_edits.remove(col_name).is_some();
                    if row_edits.is_empty() {
                        edited_cells.remove(&row_idx);
                    }
                    removed
                } else {
                    false
                }
            }
        }
    }

    pub fn clear_edited_cells(&mut self, id: Uuid) {
        if let Some(tab) = self.tab_by_id_mut(id)
            && let TabType::TableBrowser { edited_cells, .. } = &mut tab.tab_type
        {
            edited_cells.clear();
        }
    }

    pub fn set_table_browser_primary_keys(&mut self, id: Uuid, pks: Vec<String>) {
        if let Some(tab) = self.tab_by_id_mut(id)
            && let TabType::TableBrowser { primary_keys, .. } = &mut tab.tab_type
        {
            *primary_keys = Some(pks);
        }
    }

    pub fn set_table_browser_foreign_keys(&mut self, id: Uuid, fks: Vec<ForeignKeyInfo>) {
        if let Some(tab) = self.tab_by_id_mut(id)
            && let TabType::TableBrowser { foreign_keys, .. } = &mut tab.tab_type
        {
            *foreign_keys = Some(fks);
        }
    }

    /// Total number of rows that have at least one pending edit.
    pub fn total_edited_rows(tab: &Tab) -> usize {
        match &tab.tab_type {
            TabType::TableBrowser { edited_cells, .. } => edited_cells.len(),
            _ => 0,
        }
    }

    pub fn open_tab(&mut self, title: String, tab_type: TabType) -> Uuid {
        let id = Uuid::new_v4();
        self.tabs.push(Tab {
            id,
            title,
            tab_type,
            result: None,
            error: None,
            is_loading: false,
            cancellation_token: CancellationToken::new(),
            total_count: None,
        });
        self.active_tab_id = Some(id);
        id
    }

    pub fn close_tab(&mut self, id: Uuid) -> bool {
        let Some(index) = self.tabs.iter().position(|t| t.id == id) else {
            return false;
        };

        self.tabs[index].cancellation_token.cancel();
        self.tabs.remove(index);

        if self.active_tab_id == Some(id) {
            self.active_tab_id = if self.tabs.is_empty() {
                None
            } else {
                let new_index = if index > 0 { index - 1 } else { 0 };
                Some(self.tabs[new_index].id)
            };
        }

        true
    }

    pub fn set_active(&mut self, id: Uuid) -> bool {
        if self.tabs.iter().any(|t| t.id == id) {
            self.active_tab_id = Some(id);
            true
        } else {
            false
        }
    }

    pub fn activate_next_tab(&mut self) -> bool {
        if self.tabs.is_empty() {
            return false;
        }

        let current_index = self
            .active_tab_id
            .and_then(|id| self.tabs.iter().position(|t| t.id == id))
            .unwrap_or(0);
        let next_index = (current_index + 1) % self.tabs.len();
        self.active_tab_id = Some(self.tabs[next_index].id);
        true
    }

    pub fn activate_previous_tab(&mut self) -> bool {
        if self.tabs.is_empty() {
            return false;
        }

        let current_index = self
            .active_tab_id
            .and_then(|id| self.tabs.iter().position(|t| t.id == id))
            .unwrap_or(0);
        let prev_index = (current_index + self.tabs.len() - 1) % self.tabs.len();
        self.active_tab_id = Some(self.tabs[prev_index].id);
        true
    }

    pub fn open_sql_editor(&mut self) -> Uuid {
        let n = self
            .tabs
            .iter()
            .filter_map(|t| {
                if matches!(t.tab_type, TabType::SqlEditor { .. }) {
                    t.title
                        .strip_prefix("Query ")
                        .and_then(|n| n.parse::<usize>().ok())
                } else {
                    None
                }
            })
            .max()
            .unwrap_or(0)
            + 1;
        self.open_tab(
            format!("Query {n}"),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        )
    }

    pub fn open_trigger_view(&mut self, trigger_name: String, schema: Option<String>) -> Uuid {
        let qualified_name = match &schema {
            Some(s) => format!("{s}.{trigger_name}"),
            None => trigger_name.clone(),
        };

        if let Some(existing) = self.tabs.iter().find(|t| {
            matches!(&t.tab_type, TabType::TriggerView { object_name, .. } if object_name == &qualified_name)
        }) {
            let id = existing.id;
            self.active_tab_id = Some(id);
            return id;
        }

        self.open_tab(
            trigger_name,
            TabType::TriggerView {
                object_name: qualified_name,
                definition: None,
            },
        )
    }

    pub fn open_function_view(&mut self, function_name: String, schema: Option<String>) -> Uuid {
        let qualified_name = match &schema {
            Some(s) => format!("{s}.{function_name}"),
            None => function_name.clone(),
        };

        if let Some(existing) = self.tabs.iter().find(|t| {
            matches!(&t.tab_type, TabType::FunctionView { object_name, .. } if object_name == &qualified_name)
        }) {
            let id = existing.id;
            self.active_tab_id = Some(id);
            return id;
        }

        self.open_tab(
            function_name,
            TabType::FunctionView {
                object_name: qualified_name,
                definition: None,
            },
        )
    }

    pub fn open_view_source(&mut self, view_name: String, schema: Option<String>) -> Uuid {
        let qualified_name = match &schema {
            Some(s) => format!("{s}.{view_name}"),
            None => view_name.clone(),
        };

        if let Some(existing) = self.tabs.iter().find(|t| {
            matches!(&t.tab_type, TabType::ViewSource { object_name, .. } if object_name == &qualified_name)
        }) {
            let id = existing.id;
            self.active_tab_id = Some(id);
            return id;
        }

        self.open_tab(
            format!("{view_name} source"),
            TabType::ViewSource {
                object_name: qualified_name,
                definition: None,
            },
        )
    }

    pub fn open_table_browser(&mut self, table_name: String, schema: Option<String>) -> Uuid {
        self.open_object_browser(table_name, schema, ObjectType::Table)
    }

    pub fn open_view_browser(&mut self, view_name: String, schema: Option<String>) -> Uuid {
        self.open_object_browser(view_name, schema, ObjectType::View)
    }

    fn open_object_browser(
        &mut self,
        object_name: String,
        schema: Option<String>,
        object_type: ObjectType,
    ) -> Uuid {
        let qualified_name = match &schema {
            Some(s) => format!("{s}.{object_name}"),
            None => object_name.clone(),
        };

        // If a tab for this table already exists, just activate it
        if let Some(existing) = self.tabs.iter().find(|t| {
            matches!(&t.tab_type, TabType::TableBrowser { object_name, .. } if object_name == &qualified_name)
        }) {
            let id = existing.id;
            self.active_tab_id = Some(id);
            return id;
        }

        let quoted = match &schema {
            Some(s) => format!("\"{s}\".\"{object_name}\""),
            None => format!("\"{object_name}\""),
        };
        let sql = format!("SELECT * FROM {quoted} LIMIT {PAGE_SIZE}");
        let count_sql = format!("SELECT COUNT(*) FROM {quoted}");
        self.open_tab(
            object_name,
            TabType::TableBrowser {
                object_name: qualified_name,
                generated_sql: sql,
                count_sql,
                object_type,
                where_clause: String::new(),
                primary_keys: None,
                foreign_keys: None,
                edited_cells: BTreeMap::new(),
            },
        )
    }

    pub fn set_table_browser_where(&mut self, id: Uuid, text: String) -> bool {
        let normalized = strip_leading_where(text.trim()).to_string();
        let validation = validate_where_predicate(&normalized);

        let Some(tab) = self.tab_by_id_mut(id) else {
            return false;
        };
        if !matches!(tab.tab_type, TabType::TableBrowser { .. }) {
            return false;
        }

        if let Err(msg) = validation {
            tab.error = Some(msg);
            return false;
        }

        if let TabType::TableBrowser {
            generated_sql,
            count_sql,
            where_clause,
            ..
        } = &mut tab.tab_type
        {
            *generated_sql = set_where_clause(generated_sql, &normalized);
            *count_sql = set_where_clause(count_sql, &normalized);
            *where_clause = normalized;
        }

        tab.result = None;
        tab.total_count = None;
        tab.error = None;
        tab.is_loading = false;

        true
    }

    pub fn cycle_order_by_column(&mut self, id: Uuid, column_name: &str) -> Option<String> {
        let tab = self.tab_by_id_mut(id)?;
        let updated_sql = match &mut tab.tab_type {
            TabType::SqlEditor { sql_content } => {
                let updated = cycle_order_by(sql_content, column_name);
                *sql_content = updated.clone();
                updated
            }
            TabType::TableBrowser { generated_sql, .. } => {
                let updated = cycle_order_by(generated_sql, column_name);
                *generated_sql = updated.clone();
                updated
            }
            _ => return None,
        };

        tab.result = None;
        tab.total_count = None;
        tab.error = None;
        tab.is_loading = false;

        Some(updated_sql)
    }

    pub fn tab_column_ordering(&self, id: Uuid) -> Vec<ColumnOrderInfo> {
        let sql = self
            .tab_by_id(id)
            .and_then(|tab| tab.result.as_ref().map(|r| r.query.as_str()));

        let Some(sql) = sql else {
            return Vec::new();
        };

        parse_order_items(sql)
            .into_iter()
            .enumerate()
            .filter_map(|(idx, (expr, direction))| {
                sort_column_key(&expr).map(|column_key| ColumnOrderInfo {
                    column_key,
                    direction,
                    precedence: idx + 1,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tab_manager_is_empty() {
        let tm = TabManager::new();
        assert!(tm.tabs().is_empty());
        assert!(tm.active_tab_id().is_none());
        assert!(tm.active_tab().is_none());
    }

    #[test]
    fn test_open_tab_sets_active() {
        let mut tm = TabManager::new();
        let id = tm.open_tab(
            "Test".into(),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        );
        assert_eq!(tm.tabs().len(), 1);
        assert_eq!(tm.active_tab_id(), Some(id));
        assert_eq!(tm.active_tab().unwrap().title, "Test");
    }

    #[test]
    fn test_open_multiple_tabs_last_is_active() {
        let mut tm = TabManager::new();
        let _id1 = tm.open_tab(
            "First".into(),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        );
        let id2 = tm.open_tab(
            "Second".into(),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        );
        assert_eq!(tm.tabs().len(), 2);
        assert_eq!(tm.active_tab_id(), Some(id2));
    }

    #[test]
    fn test_set_active() {
        let mut tm = TabManager::new();
        let id1 = tm.open_tab(
            "First".into(),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        );
        let _id2 = tm.open_tab(
            "Second".into(),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        );
        assert!(tm.set_active(id1));
        assert_eq!(tm.active_tab_id(), Some(id1));
    }

    #[test]
    fn test_set_active_nonexistent_returns_false() {
        let mut tm = TabManager::new();
        assert!(!tm.set_active(Uuid::new_v4()));
    }

    #[test]
    fn test_close_tab() {
        let mut tm = TabManager::new();
        let id = tm.open_tab(
            "Test".into(),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        );
        assert!(tm.close_tab(id));
        assert!(tm.tabs().is_empty());
        assert!(tm.active_tab_id().is_none());
    }

    #[test]
    fn test_close_tab_selects_left_neighbor() {
        let mut tm = TabManager::new();
        let id1 = tm.open_tab(
            "First".into(),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        );
        let _id2 = tm.open_tab(
            "Second".into(),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        );
        let id3 = tm.open_tab(
            "Third".into(),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        );
        tm.set_active(id3);
        tm.close_tab(id3);
        // Should select "Second" (left neighbor at index 1)
        assert_eq!(tm.active_tab().unwrap().title, "Second");

        // Now close "Second", should select "First"
        let second_id = tm.active_tab_id().unwrap();
        tm.close_tab(second_id);
        assert_eq!(tm.active_tab_id(), Some(id1));
    }

    #[test]
    fn test_close_tab_selects_right_when_leftmost() {
        let mut tm = TabManager::new();
        let id1 = tm.open_tab(
            "First".into(),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        );
        let id2 = tm.open_tab(
            "Second".into(),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        );
        tm.set_active(id1);
        tm.close_tab(id1);
        assert_eq!(tm.active_tab_id(), Some(id2));
    }

    #[test]
    fn test_close_nonexistent_returns_false() {
        let mut tm = TabManager::new();
        assert!(!tm.close_tab(Uuid::new_v4()));
    }

    #[test]
    fn test_close_tab_cancels_token() {
        let mut tm = TabManager::new();
        let id = tm.open_tab(
            "Test".into(),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        );
        let token = tm.tab_by_id(id).unwrap().cancellation_token.clone();
        assert!(!token.is_cancelled());
        tm.close_tab(id);
        assert!(token.is_cancelled());
    }

    #[test]
    fn test_open_sql_editor() {
        let mut tm = TabManager::new();
        let id = tm.open_sql_editor();
        let tab = tm.tab_by_id(id).unwrap();
        assert_eq!(tab.title, "Query 1");
        assert_eq!(
            tab.tab_type,
            TabType::SqlEditor {
                sql_content: String::new()
            }
        );

        let id2 = tm.open_sql_editor();
        let tab2 = tm.tab_by_id(id2).unwrap();
        assert_eq!(tab2.title, "Query 2");
    }

    #[test]
    fn test_open_sql_editor_skips_closed_numbers() {
        let mut tm = TabManager::new();
        let id1 = tm.open_sql_editor(); // Query 1
        tm.open_sql_editor(); // Query 2
        tm.close_tab(id1); // close Query 1; Query 2 remains
        let id3 = tm.open_sql_editor(); // should be Query 3, not Query 2
        assert_eq!(tm.tab_by_id(id3).unwrap().title, "Query 3");
    }

    #[test]
    fn test_open_table_browser() {
        let mut tm = TabManager::new();
        let id = tm.open_table_browser("users".into(), None);
        let tab = tm.tab_by_id(id).unwrap();
        assert_eq!(tab.title, "users");
        assert_eq!(
            tab.tab_type,
            TabType::TableBrowser {
                object_name: "users".into(),
                generated_sql: "SELECT * FROM \"users\" LIMIT 100".into(),
                count_sql: "SELECT COUNT(*) FROM \"users\"".into(),
                object_type: ObjectType::Table,
                where_clause: String::new(),
                primary_keys: None,
                foreign_keys: None,
                edited_cells: BTreeMap::new(),
            }
        );
    }

    #[test]
    fn test_open_table_browser_with_schema() {
        let mut tm = TabManager::new();
        let id = tm.open_table_browser("user".into(), Some("testschema".into()));
        let tab = tm.tab_by_id(id).unwrap();
        assert_eq!(tab.title, "user");
        assert_eq!(
            tab.tab_type,
            TabType::TableBrowser {
                object_name: "testschema.user".into(),
                generated_sql: "SELECT * FROM \"testschema\".\"user\" LIMIT 100".into(),
                count_sql: "SELECT COUNT(*) FROM \"testschema\".\"user\"".into(),
                object_type: ObjectType::Table,
                where_clause: String::new(),
                primary_keys: None,
                foreign_keys: None,
                edited_cells: BTreeMap::new(),
            }
        );
    }

    #[test]
    fn test_open_view_browser() {
        let mut tm = TabManager::new();
        let id = tm.open_view_browser("active_users".into(), Some("public".into()));
        let tab = tm.tab_by_id(id).unwrap();
        assert_eq!(tab.title, "active_users");
        assert_eq!(
            tab.tab_type,
            TabType::TableBrowser {
                object_name: "public.active_users".into(),
                generated_sql: "SELECT * FROM \"public\".\"active_users\" LIMIT 100".into(),
                count_sql: "SELECT COUNT(*) FROM \"public\".\"active_users\"".into(),
                object_type: ObjectType::View,
                where_clause: String::new(),
                primary_keys: None,
                foreign_keys: None,
                edited_cells: BTreeMap::new(),
            }
        );
    }

    #[test]
    fn test_open_table_browser_reuses_existing_tab() {
        let mut tm = TabManager::new();
        let id1 = tm.open_table_browser("users".into(), None);
        let id_other = tm.open_sql_editor();
        assert_eq!(tm.active_tab_id(), Some(id_other));

        // Opening the same table again should reuse the existing tab
        let id2 = tm.open_table_browser("users".into(), None);
        assert_eq!(id1, id2);
        assert_eq!(tm.active_tab_id(), Some(id1));
        assert_eq!(tm.tabs().len(), 2); // not 3

        // Different table should open a new tab
        let id3 = tm.open_table_browser("orders".into(), None);
        assert_ne!(id1, id3);
        assert_eq!(tm.tabs().len(), 3);
    }

    #[test]
    fn test_open_table_browser_same_name_different_schema() {
        let mut tm = TabManager::new();
        let id1 = tm.open_table_browser("users".into(), Some("public".into()));
        let id2 = tm.open_table_browser("users".into(), Some("audit".into()));
        assert_ne!(id1, id2);
        assert_eq!(tm.tabs().len(), 2);
    }

    #[test]
    fn test_open_trigger_view() {
        let mut tm = TabManager::new();
        let id = tm.open_trigger_view("my_trigger".into(), None);
        let tab = tm.tab_by_id(id).unwrap();
        assert_eq!(tab.title, "my_trigger");
        assert_eq!(
            tab.tab_type,
            TabType::TriggerView {
                object_name: "my_trigger".into(),
                definition: None,
            }
        );
    }

    #[test]
    fn test_open_trigger_view_with_schema() {
        let mut tm = TabManager::new();
        let id = tm.open_trigger_view("my_trigger".into(), Some("public".into()));
        let tab = tm.tab_by_id(id).unwrap();
        assert_eq!(
            tab.tab_type,
            TabType::TriggerView {
                object_name: "public.my_trigger".into(),
                definition: None,
            }
        );
    }

    #[test]
    fn test_open_trigger_view_reuses_existing() {
        let mut tm = TabManager::new();
        let id1 = tm.open_trigger_view("trg".into(), None);
        let _id2 = tm.open_sql_editor();
        let id3 = tm.open_trigger_view("trg".into(), None);
        assert_eq!(id1, id3);
        assert_eq!(tm.tabs().len(), 2);
    }

    #[test]
    fn test_open_function_view() {
        let mut tm = TabManager::new();
        let id = tm.open_function_view("my_func".into(), Some("public".into()));
        let tab = tm.tab_by_id(id).unwrap();
        assert_eq!(tab.title, "my_func");
        assert_eq!(
            tab.tab_type,
            TabType::FunctionView {
                object_name: "public.my_func".into(),
                definition: None,
            }
        );
    }

    #[test]
    fn test_open_function_view_reuses_existing() {
        let mut tm = TabManager::new();
        let id1 = tm.open_function_view("fn".into(), Some("public".into()));
        let _id2 = tm.open_sql_editor();
        let id3 = tm.open_function_view("fn".into(), Some("public".into()));
        assert_eq!(id1, id3);
        assert_eq!(tm.tabs().len(), 2);
    }

    #[test]
    fn test_open_view_source() {
        let mut tm = TabManager::new();
        let id = tm.open_view_source("active_users".into(), Some("public".into()));
        let tab = tm.tab_by_id(id).unwrap();
        assert_eq!(tab.title, "active_users source");
        assert_eq!(
            tab.tab_type,
            TabType::ViewSource {
                object_name: "public.active_users".into(),
                definition: None,
            }
        );
    }

    #[test]
    fn test_open_view_source_reuses_existing() {
        let mut tm = TabManager::new();
        let id1 = tm.open_view_source("v_users".into(), None);
        let _id2 = tm.open_sql_editor();
        let id3 = tm.open_view_source("v_users".into(), None);
        assert_eq!(id1, id3);
        assert_eq!(tm.tabs().len(), 2);
    }

    #[test]
    fn test_tab_by_id_mut() {
        let mut tm = TabManager::new();
        let id = tm.open_tab(
            "Test".into(),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        );
        tm.tab_by_id_mut(id).unwrap().title = "Updated".into();
        assert_eq!(tm.tab_by_id(id).unwrap().title, "Updated");
    }

    #[test]
    fn test_close_inactive_tab_keeps_active() {
        let mut tm = TabManager::new();
        let id1 = tm.open_tab(
            "First".into(),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        );
        let id2 = tm.open_tab(
            "Second".into(),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        );
        // id2 is active
        tm.close_tab(id1);
        assert_eq!(tm.active_tab_id(), Some(id2));
    }

    #[test]
    fn test_activate_next_tab_wraps() {
        let mut tm = TabManager::new();
        let id1 = tm.open_tab(
            "First".into(),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        );
        let id2 = tm.open_tab(
            "Second".into(),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        );
        let id3 = tm.open_tab(
            "Third".into(),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        );

        assert_eq!(tm.active_tab_id(), Some(id3));
        assert!(tm.activate_next_tab());
        assert_eq!(tm.active_tab_id(), Some(id1));
        assert!(tm.activate_next_tab());
        assert_eq!(tm.active_tab_id(), Some(id2));
    }

    #[test]
    fn test_activate_previous_tab_wraps() {
        let mut tm = TabManager::new();
        let id1 = tm.open_tab(
            "First".into(),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        );
        let id2 = tm.open_tab(
            "Second".into(),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        );
        let id3 = tm.open_tab(
            "Third".into(),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        );

        assert_eq!(tm.active_tab_id(), Some(id3));
        assert!(tm.activate_previous_tab());
        assert_eq!(tm.active_tab_id(), Some(id2));
        assert!(tm.activate_previous_tab());
        assert_eq!(tm.active_tab_id(), Some(id1));
        assert!(tm.activate_previous_tab());
        assert_eq!(tm.active_tab_id(), Some(id3));
    }

    #[test]
    fn test_activate_tab_navigation_empty() {
        let mut tm = TabManager::new();
        assert!(!tm.activate_next_tab());
        assert!(!tm.activate_previous_tab());
    }

    #[test]
    fn test_tab_column_ordering_reports_direction_and_precedence() {
        let mut tm = TabManager::new();
        let id = tm.open_tab(
            "Query 1".into(),
            TabType::SqlEditor {
                sql_content: "SELECT * FROM users ORDER BY \"id\" DESC, name ASC LIMIT 10".into(),
            },
        );

        let tab = tm.tab_by_id_mut(id).unwrap();
        tab.result = Some(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            rows_affected: 0,
            execution_time: std::time::Duration::from_millis(1),
            query: "SELECT * FROM users ORDER BY \"id\" DESC, name ASC LIMIT 10".into(),
        });

        let ordering = tm.tab_column_ordering(id);
        assert_eq!(
            ordering,
            vec![
                ColumnOrderInfo {
                    column_key: "id".into(),
                    direction: SortDirection::Desc,
                    precedence: 1,
                },
                ColumnOrderInfo {
                    column_key: "name".into(),
                    direction: SortDirection::Asc,
                    precedence: 2,
                }
            ]
        );
    }

    #[test]
    fn test_set_table_browser_where_injects_into_both_sqls() {
        let mut tm = TabManager::new();
        let id = tm.open_table_browser("users".into(), None);

        assert!(tm.set_table_browser_where(id, "id > 5".into()));
        let tab = tm.tab_by_id(id).unwrap();
        let TabType::TableBrowser {
            generated_sql,
            count_sql,
            where_clause,
            ..
        } = &tab.tab_type
        else {
            panic!("expected TableBrowser");
        };
        assert_eq!(
            generated_sql,
            "SELECT * FROM \"users\" WHERE id > 5 LIMIT 100"
        );
        assert_eq!(count_sql, "SELECT COUNT(*) FROM \"users\" WHERE id > 5");
        assert_eq!(where_clause, "id > 5");
    }

    #[test]
    fn test_set_table_browser_where_replaces_and_clears() {
        let mut tm = TabManager::new();
        let id = tm.open_table_browser("users".into(), None);

        tm.set_table_browser_where(id, "id > 5".into());
        tm.set_table_browser_where(id, "name = 'a'".into());
        let tab = tm.tab_by_id(id).unwrap();
        let TabType::TableBrowser {
            generated_sql,
            count_sql,
            ..
        } = &tab.tab_type
        else {
            panic!();
        };
        assert_eq!(
            generated_sql,
            "SELECT * FROM \"users\" WHERE name = 'a' LIMIT 100"
        );
        assert_eq!(count_sql, "SELECT COUNT(*) FROM \"users\" WHERE name = 'a'");

        tm.set_table_browser_where(id, "   ".into());
        let tab = tm.tab_by_id(id).unwrap();
        let TabType::TableBrowser {
            generated_sql,
            count_sql,
            where_clause,
            ..
        } = &tab.tab_type
        else {
            panic!();
        };
        assert_eq!(generated_sql, "SELECT * FROM \"users\" LIMIT 100");
        assert_eq!(count_sql, "SELECT COUNT(*) FROM \"users\"");
        assert!(where_clause.is_empty());
    }

    #[test]
    fn test_set_table_browser_where_strips_leading_keyword() {
        let mut tm = TabManager::new();
        let id = tm.open_table_browser("users".into(), None);

        tm.set_table_browser_where(id, "WHERE id = 1".into());
        let tab = tm.tab_by_id(id).unwrap();
        let TabType::TableBrowser {
            generated_sql,
            where_clause,
            ..
        } = &tab.tab_type
        else {
            panic!();
        };
        assert_eq!(
            generated_sql,
            "SELECT * FROM \"users\" WHERE id = 1 LIMIT 100"
        );
        assert_eq!(where_clause, "id = 1");
    }

    #[test]
    fn test_set_table_browser_where_rejects_filter_with_order_by() {
        let mut tm = TabManager::new();
        let id = tm.open_table_browser("users".into(), None);
        tm.set_table_browser_where(id, "id < 5".into());

        let applied = tm.set_table_browser_where(id, "id < 5 ORDER BY id DESC".into());
        assert!(!applied);

        let tab = tm.tab_by_id(id).unwrap();
        let TabType::TableBrowser {
            generated_sql,
            count_sql,
            where_clause,
            ..
        } = &tab.tab_type
        else {
            panic!();
        };
        // Existing filter is preserved; the bad input did not overwrite it
        assert_eq!(
            generated_sql,
            "SELECT * FROM \"users\" WHERE id < 5 LIMIT 100"
        );
        assert_eq!(count_sql, "SELECT COUNT(*) FROM \"users\" WHERE id < 5");
        assert_eq!(where_clause, "id < 5");

        let err = tab.error.as_ref().expect("error should be set");
        assert!(err.contains("ORDER"), "got error: {err}");
    }

    #[test]
    fn test_set_table_browser_where_returns_false_for_non_browser_tab() {
        let mut tm = TabManager::new();
        let id = tm.open_sql_editor();
        assert!(!tm.set_table_browser_where(id, "x = 1".into()));
    }

    #[test]
    fn test_set_table_browser_where_clears_result_state() {
        let mut tm = TabManager::new();
        let id = tm.open_table_browser("users".into(), None);
        let tab = tm.tab_by_id_mut(id).unwrap();
        tab.result = Some(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            rows_affected: 0,
            execution_time: std::time::Duration::from_millis(1),
            query: "stale".into(),
        });
        tab.total_count = Some(42);
        tab.error = Some("stale".into());

        tm.set_table_browser_where(id, "id = 1".into());
        let tab = tm.tab_by_id(id).unwrap();
        assert!(tab.result.is_none());
        assert!(tab.total_count.is_none());
        assert!(tab.error.is_none());
    }

    #[test]
    fn test_tab_column_ordering_uses_last_executed_query() {
        let mut tm = TabManager::new();
        let id = tm.open_tab(
            "Query 1".into(),
            TabType::SqlEditor {
                sql_content: "SELECT * FROM users ORDER BY id".into(),
            },
        );

        let tab = tm.tab_by_id_mut(id).unwrap();
        tab.result = Some(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            rows_affected: 0,
            execution_time: std::time::Duration::from_millis(1),
            query: "SELECT * FROM users".into(),
        });

        assert!(tm.tab_column_ordering(id).is_empty());
    }

    #[test]
    fn set_edited_cell_records_and_clears() {
        let mut tm = TabManager::new();
        let id = tm.open_table_browser("users".into(), None);

        assert!(tm.set_edited_cell(id, 0, "name", Some("Alice".into())));
        let tab = tm.tab_by_id(id).unwrap();
        let TabType::TableBrowser { edited_cells, .. } = &tab.tab_type else {
            panic!();
        };
        assert_eq!(edited_cells.get(&0).unwrap().get("name").unwrap(), "Alice");
        assert_eq!(TabManager::total_edited_rows(tab), 1);

        // Clearing the same cell removes it.
        assert!(tm.set_edited_cell(id, 0, "name", None));
        let tab = tm.tab_by_id(id).unwrap();
        let TabType::TableBrowser { edited_cells, .. } = &tab.tab_type else {
            panic!();
        };
        assert!(edited_cells.is_empty());
        assert_eq!(TabManager::total_edited_rows(tab), 0);
    }

    #[test]
    fn set_edited_cell_only_works_on_table_browser() {
        let mut tm = TabManager::new();
        let id = tm.open_sql_editor();
        assert!(!tm.set_edited_cell(id, 0, "name", Some("x".into())));
    }

    #[test]
    fn clear_edited_cells_empties_map() {
        let mut tm = TabManager::new();
        let id = tm.open_table_browser("users".into(), None);
        tm.set_edited_cell(id, 0, "name", Some("Alice".into()));
        tm.set_edited_cell(id, 1, "name", Some("Bob".into()));

        tm.clear_edited_cells(id);
        let tab = tm.tab_by_id(id).unwrap();
        let TabType::TableBrowser { edited_cells, .. } = &tab.tab_type else {
            panic!();
        };
        assert!(edited_cells.is_empty());
    }

    #[test]
    fn reset_for_refresh_clears_edits_too() {
        let mut tm = TabManager::new();
        let id = tm.open_table_browser("users".into(), None);
        tm.set_edited_cell(id, 0, "name", Some("Alice".into()));

        tm.reset_for_refresh(id);
        let tab = tm.tab_by_id(id).unwrap();
        let TabType::TableBrowser { edited_cells, .. } = &tab.tab_type else {
            panic!();
        };
        assert!(edited_cells.is_empty());
    }

    #[test]
    fn set_table_browser_primary_keys_caches_them() {
        let mut tm = TabManager::new();
        let id = tm.open_table_browser("users".into(), None);

        let tab = tm.tab_by_id(id).unwrap();
        let TabType::TableBrowser { primary_keys, .. } = &tab.tab_type else {
            panic!();
        };
        assert!(primary_keys.is_none());

        tm.set_table_browser_primary_keys(id, vec!["id".into()]);
        let tab = tm.tab_by_id(id).unwrap();
        let TabType::TableBrowser { primary_keys, .. } = &tab.tab_type else {
            panic!();
        };
        assert_eq!(primary_keys.as_deref(), Some(&["id".to_string()][..]));
    }

    #[test]
    fn set_table_browser_foreign_keys_caches_them() {
        let mut tm = TabManager::new();
        let id = tm.open_table_browser("orders".into(), None);

        let tab = tm.tab_by_id(id).unwrap();
        let TabType::TableBrowser { foreign_keys, .. } = &tab.tab_type else {
            panic!();
        };
        assert!(foreign_keys.is_none());

        let fk = ForeignKeyInfo {
            from_column: "user_id".into(),
            to_schema: None,
            to_table: "users".into(),
            to_column: "id".into(),
        };
        tm.set_table_browser_foreign_keys(id, vec![fk.clone()]);
        let tab = tm.tab_by_id(id).unwrap();
        let TabType::TableBrowser { foreign_keys, .. } = &tab.tab_type else {
            panic!();
        };
        assert_eq!(foreign_keys.as_deref(), Some(&[fk][..]));
    }
}
