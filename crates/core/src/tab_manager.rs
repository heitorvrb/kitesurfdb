use db::types::QueryResult;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

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
    },
    TriggerView {
        object_name: String,
        definition: Option<String>,
    },
    FunctionView {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnOrderInfo {
    pub column_key: String,
    pub direction: SortDirection,
    pub precedence: usize,
}

fn is_ident_char(b: u8) -> bool {
    matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
}

fn can_match_keyword(sql: &str, idx: usize, keyword_len: usize) -> bool {
    let bytes = sql.as_bytes();
    let before_ok = idx == 0 || !is_ident_char(bytes[idx.saturating_sub(1)]);
    let after_idx = idx + keyword_len;
    let after_ok = after_idx >= bytes.len() || !is_ident_char(bytes[after_idx]);
    before_ok && after_ok
}

fn find_top_level_keyword(sql: &str, keyword: &str, min_idx: usize) -> Option<usize> {
    if sql.is_empty() {
        return None;
    }

    let keyword_len = keyword.len();
    let bytes = sql.as_bytes();
    let lower = sql.to_ascii_lowercase();
    let lower_bytes = lower.as_bytes();
    let keyword_bytes = keyword.as_bytes();
    let mut i = min_idx;
    let mut depth: usize = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;

    while i < bytes.len() {
        if in_line_comment {
            if bytes[i] == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }

        if in_block_comment {
            if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                in_block_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }

        if in_single {
            if bytes[i] == b'\'' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                    i += 2;
                    continue;
                }
                in_single = false;
            }
            i += 1;
            continue;
        }

        if in_double {
            if bytes[i] == b'"' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                    i += 2;
                    continue;
                }
                in_double = false;
            }
            i += 1;
            continue;
        }

        if i + 1 < bytes.len() && bytes[i] == b'-' && bytes[i + 1] == b'-' {
            in_line_comment = true;
            i += 2;
            continue;
        }

        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            in_block_comment = true;
            i += 2;
            continue;
        }

        if bytes[i] == b'\'' {
            in_single = true;
            i += 1;
            continue;
        }

        if bytes[i] == b'"' {
            in_double = true;
            i += 1;
            continue;
        }

        match bytes[i] {
            b'(' => depth += 1,
            b')' if depth > 0 => depth -= 1,
            _ => {}
        }

        if depth == 0
            && i + keyword_len <= bytes.len()
            && &lower_bytes[i..i + keyword_len] == keyword_bytes
            && can_match_keyword(sql, i, keyword_len)
        {
            return Some(i);
        }

        i += 1;
    }

    None
}

fn split_top_level_commas(clause: &str) -> Vec<String> {
    let bytes = clause.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    let mut depth: usize = 0;
    let mut in_single = false;
    let mut in_double = false;

    while i < bytes.len() {
        if in_single {
            if bytes[i] == b'\'' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                    i += 2;
                    continue;
                }
                in_single = false;
            }
            i += 1;
            continue;
        }

        if in_double {
            if bytes[i] == b'"' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                    i += 2;
                    continue;
                }
                in_double = false;
            }
            i += 1;
            continue;
        }

        match bytes[i] {
            b'\'' => {
                in_single = true;
                i += 1;
                continue;
            }
            b'"' => {
                in_double = true;
                i += 1;
                continue;
            }
            b'(' => depth += 1,
            b')' if depth > 0 => depth -= 1,
            b',' if depth == 0 => {
                let piece = clause[start..i].trim();
                if !piece.is_empty() {
                    out.push(piece.to_string());
                }
                start = i + 1;
            }
            _ => {}
        }

        i += 1;
    }

    let tail = clause[start..].trim();
    if !tail.is_empty() {
        out.push(tail.to_string());
    }

    out
}

fn strip_suffix<'a>(value: &'a str, suffix: &str) -> Option<&'a str> {
    if value.len() < suffix.len() {
        return None;
    }

    let start = value.len() - suffix.len();
    if value[start..].eq_ignore_ascii_case(suffix) {
        Some(value[..start].trim_end())
    } else {
        None
    }
}

fn normalize_identifier(value: &str) -> String {
    let trimmed = value.trim();
    let unquoted = if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    unquoted.to_ascii_lowercase()
}

fn extract_sort_column_key(expr: &str) -> Option<String> {
    let trimmed = expr.trim();
    if trimmed.is_empty()
        || trimmed.contains('(')
        || trimmed.contains(')')
        || trimmed.contains(' ')
        || trimmed.contains('\t')
        || trimmed.contains('\n')
    {
        return None;
    }

    let last = trimmed.rsplit('.').next().unwrap_or(trimmed);
    let key = normalize_identifier(last);
    if key.is_empty() {
        None
    } else {
        Some(key)
    }
}

fn parse_order_item(item: &str) -> Option<(String, SortDirection)> {
    let trimmed = item.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(base) = strip_suffix(trimmed, "desc") {
        if !base.is_empty() {
            return Some((base.to_string(), SortDirection::Desc));
        }
    }

    if let Some(base) = strip_suffix(trimmed, "asc") {
        if !base.is_empty() {
            return Some((base.to_string(), SortDirection::Asc));
        }
    }

    Some((trimmed.to_string(), SortDirection::Asc))
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn cycle_order_by(sql: &str, column_name: &str) -> String {
    let trimmed = sql.trim();
    if trimmed.is_empty() || column_name.trim().is_empty() {
        return sql.to_string();
    }

    let had_semicolon = trimmed.ends_with(';');
    let base = if had_semicolon {
        trimmed.trim_end_matches(';').trim_end()
    } else {
        trimmed
    };

    let order_idx = find_top_level_keyword(base, "order by", 0);
    let mut suffix_start = base.len();

    for keyword in ["limit", "offset", "fetch"] {
        if let Some(idx) = find_top_level_keyword(base, keyword, 0)
            && idx < suffix_start
            && order_idx.is_none_or(|o| idx > o)
        {
            suffix_start = idx;
        }
    }

    let (prefix, existing_order, suffix) = if let Some(order_start) = order_idx {
        let order_clause_start = order_start + "order by".len();
        let mut order_end = base.len();
        for keyword in ["limit", "offset", "fetch"] {
            if let Some(idx) = find_top_level_keyword(base, keyword, order_clause_start)
                && idx < order_end
            {
                order_end = idx;
            }
        }

        (
            base[..order_start].trim_end(),
            base[order_clause_start..order_end].trim(),
            base[order_end..].trim(),
        )
    } else {
        (
            base[..suffix_start].trim_end(),
            "",
            base[suffix_start..].trim(),
        )
    };

    let mut order_items: Vec<(String, SortDirection)> = split_top_level_commas(existing_order)
        .into_iter()
        .filter_map(|item| parse_order_item(&item))
        .collect();

    let target = normalize_identifier(column_name);
    if let Some((idx, (_, dir))) = order_items
        .iter()
        .enumerate()
        .find(|(_, (expr, _))| normalize_identifier(expr) == target)
    {
        match dir {
            SortDirection::Asc => order_items[idx].1 = SortDirection::Desc,
            SortDirection::Desc => {
                order_items.remove(idx);
            }
        }
    } else {
        order_items.push((quote_identifier(column_name), SortDirection::Asc));
    }

    let mut rebuilt = String::new();
    rebuilt.push_str(prefix);
    if !order_items.is_empty() {
        if !rebuilt.is_empty() {
            rebuilt.push(' ');
        }
        rebuilt.push_str("ORDER BY ");
        rebuilt.push_str(
            &order_items
                .iter()
                .map(|(expr, dir)| match dir {
                    SortDirection::Asc => format!("{expr} ASC"),
                    SortDirection::Desc => format!("{expr} DESC"),
                })
                .collect::<Vec<_>>()
                .join(", "),
        );
    }

    if !suffix.is_empty() {
        if !rebuilt.is_empty() {
            rebuilt.push(' ');
        }
        rebuilt.push_str(suffix);
    }

    if had_semicolon {
        rebuilt.push(';');
    }

    rebuilt
}

fn parse_order_items(sql: &str) -> Vec<(String, SortDirection)> {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let base = trimmed.trim_end_matches(';').trim_end();
    let Some(order_start) = find_top_level_keyword(base, "order by", 0) else {
        return Vec::new();
    };

    let order_clause_start = order_start + "order by".len();
    let mut order_end = base.len();
    for keyword in ["limit", "offset", "fetch"] {
        if let Some(idx) = find_top_level_keyword(base, keyword, order_clause_start)
            && idx < order_end
        {
            order_end = idx;
        }
    }

    let order_clause = base[order_clause_start..order_end].trim();
    split_top_level_commas(order_clause)
        .into_iter()
        .filter_map(|item| parse_order_item(&item))
        .collect()
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
        let n = self.tabs.iter()
            .filter_map(|t| {
                if matches!(t.tab_type, TabType::SqlEditor { .. }) {
                    t.title.strip_prefix("Query ").and_then(|n| n.parse::<usize>().ok())
                } else {
                    None
                }
            })
            .max()
            .unwrap_or(0) + 1;
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

    pub fn open_table_browser(&mut self, table_name: String, schema: Option<String>) -> Uuid {
        let qualified_name = match &schema {
            Some(s) => format!("{s}.{table_name}"),
            None => table_name.clone(),
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
            Some(s) => format!("\"{s}\".\"{table_name}\""),
            None => format!("\"{table_name}\""),
        };
        let sql = format!("SELECT * FROM {quoted} LIMIT {PAGE_SIZE}");
        let count_sql = format!("SELECT COUNT(*) FROM {quoted}");
        self.open_tab(
            table_name,
            TabType::TableBrowser {
                object_name: qualified_name,
                generated_sql: sql,
                count_sql,
            },
        )
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
                extract_sort_column_key(&expr).map(|column_key| ColumnOrderInfo {
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
    fn test_open_sql_editor_skips_closed_numbers() {
        let mut tm = TabManager::new();
        let id1 = tm.open_sql_editor(); // Query 1
        tm.open_sql_editor();           // Query 2
        tm.close_tab(id1);              // close Query 1; Query 2 remains
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
    fn test_cycle_order_by_adds_updates_and_removes() {
        let mut tm = TabManager::new();
        let id = tm.open_table_browser("users".into(), None);

        let sql1 = tm.cycle_order_by_column(id, "name").unwrap();
        assert_eq!(sql1, "SELECT * FROM \"users\" ORDER BY \"name\" ASC LIMIT 100");

        let sql2 = tm.cycle_order_by_column(id, "name").unwrap();
        assert_eq!(sql2, "SELECT * FROM \"users\" ORDER BY \"name\" DESC LIMIT 100");

        let sql3 = tm.cycle_order_by_column(id, "name").unwrap();
        assert_eq!(sql3, "SELECT * FROM \"users\" LIMIT 100");
    }

    #[test]
    fn test_cycle_order_by_appends_multiple_columns() {
        let mut tm = TabManager::new();
        let id = tm.open_tab(
            "Query 1".into(),
            TabType::SqlEditor {
                sql_content: "SELECT id, name FROM users".into(),
            },
        );

        tm.cycle_order_by_column(id, "id");
        let sql = tm.cycle_order_by_column(id, "name").unwrap();
        assert_eq!(
            sql,
            "SELECT id, name FROM users ORDER BY \"id\" ASC, \"name\" ASC"
        );
    }

    #[test]
    fn test_cycle_order_by_respects_existing_order_clause() {
        let mut tm = TabManager::new();
        let id = tm.open_tab(
            "Query 1".into(),
            TabType::SqlEditor {
                sql_content: "SELECT * FROM users ORDER BY id DESC LIMIT 10".into(),
            },
        );

        let sql = tm.cycle_order_by_column(id, "id").unwrap();
        assert_eq!(sql, "SELECT * FROM users LIMIT 10");
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
}
