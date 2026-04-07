use db::types::QueryResult;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub enum TabType {
    SqlEditor {
        sql_content: String,
    },
    TableBrowser {
        object_name: String,
        generated_sql: String,
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

    pub fn open_sql_editor(&mut self) -> Uuid {
        let n = self.tabs.iter().filter(|t| matches!(t.tab_type, TabType::SqlEditor { .. })).count() + 1;
        self.open_tab(
            format!("Query {n}"),
            TabType::SqlEditor {
                sql_content: String::new(),
            },
        )
    }

    pub fn open_table_browser(&mut self, table_name: String) -> Uuid {
        // If a tab for this table already exists, just activate it
        if let Some(existing) = self.tabs.iter().find(|t| {
            matches!(&t.tab_type, TabType::TableBrowser { object_name, .. } if object_name == &table_name)
        }) {
            let id = existing.id;
            self.active_tab_id = Some(id);
            return id;
        }

        let sql = format!("SELECT * FROM \"{table_name}\" LIMIT 100");
        let title = table_name.clone();
        self.open_tab(
            title,
            TabType::TableBrowser {
                object_name: table_name,
                generated_sql: sql,
            },
        )
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
        let id = tm.open_tab("Test".into(), TabType::SqlEditor { sql_content: String::new() });
        assert_eq!(tm.tabs().len(), 1);
        assert_eq!(tm.active_tab_id(), Some(id));
        assert_eq!(tm.active_tab().unwrap().title, "Test");
    }

    #[test]
    fn test_open_multiple_tabs_last_is_active() {
        let mut tm = TabManager::new();
        let _id1 = tm.open_tab("First".into(), TabType::SqlEditor { sql_content: String::new() });
        let id2 = tm.open_tab("Second".into(), TabType::SqlEditor { sql_content: String::new() });
        assert_eq!(tm.tabs().len(), 2);
        assert_eq!(tm.active_tab_id(), Some(id2));
    }

    #[test]
    fn test_set_active() {
        let mut tm = TabManager::new();
        let id1 = tm.open_tab("First".into(), TabType::SqlEditor { sql_content: String::new() });
        let _id2 = tm.open_tab("Second".into(), TabType::SqlEditor { sql_content: String::new() });
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
        let id = tm.open_tab("Test".into(), TabType::SqlEditor { sql_content: String::new() });
        assert!(tm.close_tab(id));
        assert!(tm.tabs().is_empty());
        assert!(tm.active_tab_id().is_none());
    }

    #[test]
    fn test_close_tab_selects_left_neighbor() {
        let mut tm = TabManager::new();
        let id1 = tm.open_tab("First".into(), TabType::SqlEditor { sql_content: String::new() });
        let _id2 = tm.open_tab("Second".into(), TabType::SqlEditor { sql_content: String::new() });
        let id3 = tm.open_tab("Third".into(), TabType::SqlEditor { sql_content: String::new() });
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
        let id1 = tm.open_tab("First".into(), TabType::SqlEditor { sql_content: String::new() });
        let id2 = tm.open_tab("Second".into(), TabType::SqlEditor { sql_content: String::new() });
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
        let id = tm.open_tab("Test".into(), TabType::SqlEditor { sql_content: String::new() });
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
        assert_eq!(tab.tab_type, TabType::SqlEditor { sql_content: String::new() });

        let id2 = tm.open_sql_editor();
        let tab2 = tm.tab_by_id(id2).unwrap();
        assert_eq!(tab2.title, "Query 2");
    }

    #[test]
    fn test_open_table_browser() {
        let mut tm = TabManager::new();
        let id = tm.open_table_browser("users".into());
        let tab = tm.tab_by_id(id).unwrap();
        assert_eq!(tab.title, "users");
        assert_eq!(
            tab.tab_type,
            TabType::TableBrowser {
                object_name: "users".into(),
                generated_sql: "SELECT * FROM \"users\" LIMIT 100".into(),
            }
        );
    }

    #[test]
    fn test_open_table_browser_reuses_existing_tab() {
        let mut tm = TabManager::new();
        let id1 = tm.open_table_browser("users".into());
        let id_other = tm.open_sql_editor();
        assert_eq!(tm.active_tab_id(), Some(id_other));

        // Opening the same table again should reuse the existing tab
        let id2 = tm.open_table_browser("users".into());
        assert_eq!(id1, id2);
        assert_eq!(tm.active_tab_id(), Some(id1));
        assert_eq!(tm.tabs().len(), 2); // not 3

        // Different table should open a new tab
        let id3 = tm.open_table_browser("orders".into());
        assert_ne!(id1, id3);
        assert_eq!(tm.tabs().len(), 3);
    }

    #[test]
    fn test_tab_by_id_mut() {
        let mut tm = TabManager::new();
        let id = tm.open_tab("Test".into(), TabType::SqlEditor { sql_content: String::new() });
        tm.tab_by_id_mut(id).unwrap().title = "Updated".into();
        assert_eq!(tm.tab_by_id(id).unwrap().title, "Updated");
    }

    #[test]
    fn test_close_inactive_tab_keeps_active() {
        let mut tm = TabManager::new();
        let id1 = tm.open_tab("First".into(), TabType::SqlEditor { sql_content: String::new() });
        let id2 = tm.open_tab("Second".into(), TabType::SqlEditor { sql_content: String::new() });
        // id2 is active
        tm.close_tab(id1);
        assert_eq!(tm.active_tab_id(), Some(id2));
    }
}
