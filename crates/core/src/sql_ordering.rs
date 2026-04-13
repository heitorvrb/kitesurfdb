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

pub fn sort_column_key(expr: &str) -> Option<String> {
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

pub fn cycle_order_by(sql: &str, column_name: &str) -> String {
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

pub fn parse_order_items(sql: &str) -> Vec<(String, SortDirection)> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cycle_order_by_adds_updates_and_removes() {
        let sql1 = cycle_order_by("SELECT * FROM \"users\" LIMIT 100", "name");
        assert_eq!(sql1, "SELECT * FROM \"users\" ORDER BY \"name\" ASC LIMIT 100");

        let sql2 = cycle_order_by(&sql1, "name");
        assert_eq!(sql2, "SELECT * FROM \"users\" ORDER BY \"name\" DESC LIMIT 100");

        let sql3 = cycle_order_by(&sql2, "name");
        assert_eq!(sql3, "SELECT * FROM \"users\" LIMIT 100");
    }

    #[test]
    fn test_cycle_order_by_appends_multiple_columns() {
        let sql1 = cycle_order_by("SELECT id, name FROM users", "id");
        let sql2 = cycle_order_by(&sql1, "name");
        assert_eq!(
            sql2,
            "SELECT id, name FROM users ORDER BY \"id\" ASC, \"name\" ASC"
        );
    }

    #[test]
    fn test_cycle_order_by_respects_existing_order_clause() {
        let sql = cycle_order_by("SELECT * FROM users ORDER BY id DESC LIMIT 10", "id");
        assert_eq!(sql, "SELECT * FROM users LIMIT 10");
    }

    #[test]
    fn test_parse_order_items_reads_multiple_entries() {
        let items = parse_order_items("SELECT * FROM users ORDER BY \"id\" DESC, name ASC LIMIT 10");
        assert_eq!(
            items,
            vec![
                ("\"id\"".into(), SortDirection::Desc),
                ("name".into(), SortDirection::Asc),
            ]
        );
    }

    #[test]
    fn test_sort_column_key() {
        assert_eq!(sort_column_key("\"Name\""), Some("name".into()));
        assert_eq!(sort_column_key("public.id"), Some("id".into()));
        assert_eq!(sort_column_key("coalesce(a,b)"), None);
    }
}
