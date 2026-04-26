use crate::sql_ordering::find_top_level_keyword;

const FORBIDDEN_KEYWORDS: &[&str] = &[
    "order",
    "group",
    "having",
    "limit",
    "offset",
    "fetch",
    "union",
    "intersect",
    "except",
];

pub(crate) fn validate_where_predicate(text: &str) -> Result<(), String> {
    for kw in FORBIDDEN_KEYWORDS {
        if find_top_level_keyword(text, kw, 0).is_some() {
            return Err(format!(
                "Filter cannot contain `{}`. Use only a predicate expression.",
                kw.to_uppercase()
            ));
        }
    }
    Ok(())
}

pub(crate) fn strip_leading_where(text: &str) -> &str {
    let trimmed = text.trim();
    if trimmed.len() >= 5 && trimmed[..5].eq_ignore_ascii_case("where") {
        let rest = &trimmed[5..];
        if rest.is_empty() || rest.starts_with(|c: char| c.is_whitespace()) {
            return rest.trim_start();
        }
    }
    trimmed
}

pub fn set_where_clause(sql: &str, where_text: &str) -> String {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return sql.to_string();
    }

    let had_semicolon = trimmed.ends_with(';');
    let base = if had_semicolon {
        trimmed.trim_end_matches(';').trim_end()
    } else {
        trimmed
    };

    let new_predicate = strip_leading_where(where_text);

    let where_idx = find_top_level_keyword(base, "where", 0);
    let mut suffix_idx = base.len();
    let suffix_start_min = where_idx.map(|i| i + "where".len()).unwrap_or(0);
    for keyword in ["order by", "limit", "offset", "fetch"] {
        if let Some(idx) = find_top_level_keyword(base, keyword, suffix_start_min)
            && idx < suffix_idx
        {
            suffix_idx = idx;
        }
    }

    let (prefix, suffix) = if let Some(w_idx) = where_idx {
        (base[..w_idx].trim_end(), base[suffix_idx..].trim())
    } else {
        (base[..suffix_idx].trim_end(), base[suffix_idx..].trim())
    };

    let mut rebuilt = String::new();
    rebuilt.push_str(prefix);

    if !new_predicate.is_empty() {
        if !rebuilt.is_empty() {
            rebuilt.push(' ');
        }
        rebuilt.push_str("WHERE ");
        rebuilt.push_str(new_predicate);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_where_to_bare_select() {
        let out = set_where_clause("SELECT * FROM \"users\"", "id > 5");
        assert_eq!(out, "SELECT * FROM \"users\" WHERE id > 5");
    }

    #[test]
    fn inserts_where_before_limit() {
        let out = set_where_clause("SELECT * FROM \"users\" LIMIT 100", "id > 5");
        assert_eq!(out, "SELECT * FROM \"users\" WHERE id > 5 LIMIT 100");
    }

    #[test]
    fn inserts_where_before_order_by_and_limit() {
        let out = set_where_clause(
            "SELECT * FROM \"users\" ORDER BY \"id\" ASC LIMIT 100",
            "name = 'a'",
        );
        assert_eq!(
            out,
            "SELECT * FROM \"users\" WHERE name = 'a' ORDER BY \"id\" ASC LIMIT 100"
        );
    }

    #[test]
    fn replaces_existing_where_preserving_order_and_limit() {
        let sql = "SELECT * FROM users WHERE id > 5 ORDER BY name DESC LIMIT 10";
        let out = set_where_clause(sql, "name = 'x'");
        assert_eq!(
            out,
            "SELECT * FROM users WHERE name = 'x' ORDER BY name DESC LIMIT 10"
        );
    }

    #[test]
    fn empty_input_removes_existing_where() {
        let sql = "SELECT * FROM users WHERE id > 5 LIMIT 10";
        let out = set_where_clause(sql, "");
        assert_eq!(out, "SELECT * FROM users LIMIT 10");
    }

    #[test]
    fn empty_input_on_no_where_is_noop() {
        let sql = "SELECT * FROM users LIMIT 10";
        let out = set_where_clause(sql, "   ");
        assert_eq!(out, "SELECT * FROM users LIMIT 10");
    }

    #[test]
    fn tolerates_leading_where_keyword() {
        let out = set_where_clause("SELECT * FROM users LIMIT 10", "WHERE id > 5");
        assert_eq!(out, "SELECT * FROM users WHERE id > 5 LIMIT 10");

        let out2 = set_where_clause("SELECT * FROM users", "  where  name = 'a'");
        assert_eq!(out2, "SELECT * FROM users WHERE name = 'a'");
    }

    #[test]
    fn preserves_trailing_semicolon() {
        let out = set_where_clause("SELECT * FROM users LIMIT 10;", "id > 0");
        assert_eq!(out, "SELECT * FROM users WHERE id > 0 LIMIT 10;");
    }

    #[test]
    fn works_on_count_query() {
        let out = set_where_clause("SELECT COUNT(*) FROM \"users\"", "id > 5");
        assert_eq!(out, "SELECT COUNT(*) FROM \"users\" WHERE id > 5");

        let out2 = set_where_clause(&out, "");
        assert_eq!(out2, "SELECT COUNT(*) FROM \"users\"");
    }

    #[test]
    fn validate_accepts_simple_predicate() {
        assert!(validate_where_predicate("id > 5").is_ok());
        assert!(validate_where_predicate("name = 'a' AND age < 30").is_ok());
        assert!(validate_where_predicate("").is_ok());
    }

    #[test]
    fn validate_rejects_top_level_order() {
        let err = validate_where_predicate("id < 5 ORDER BY id DESC").unwrap_err();
        assert!(err.contains("ORDER"));
    }

    #[test]
    fn validate_rejects_top_level_limit() {
        assert!(validate_where_predicate("id < 5 LIMIT 10").is_err());
    }

    #[test]
    fn validate_rejects_top_level_group_having_offset_fetch_union() {
        assert!(validate_where_predicate("id < 5 GROUP BY id").is_err());
        assert!(validate_where_predicate("id < 5 HAVING id > 0").is_err());
        assert!(validate_where_predicate("id < 5 OFFSET 10").is_err());
        assert!(validate_where_predicate("id < 5 FETCH NEXT 1 ROWS ONLY").is_err());
        assert!(validate_where_predicate("id < 5 UNION SELECT 1").is_err());
        assert!(validate_where_predicate("id < 5 INTERSECT SELECT 1").is_err());
        assert!(validate_where_predicate("id < 5 EXCEPT SELECT 1").is_err());
    }

    #[test]
    fn validate_allows_keywords_inside_string_literals() {
        assert!(validate_where_predicate("name = 'order by id'").is_ok());
        assert!(validate_where_predicate("note = 'limit'").is_ok());
    }

    #[test]
    fn validate_allows_keywords_inside_subquery_parens() {
        let predicate = "id IN (SELECT id FROM t ORDER BY x LIMIT 5)";
        assert!(validate_where_predicate(predicate).is_ok());
    }

    #[test]
    fn validate_does_not_match_partial_words() {
        // "ordering" should not trigger the "order" keyword (boundary check)
        assert!(validate_where_predicate("status = 'pre_ordering'").is_ok());
        assert!(validate_where_predicate("ordering_id = 5").is_ok());
        assert!(validate_where_predicate("groupcode = 'a'").is_ok());
    }

    #[test]
    fn ignores_where_inside_string_literal() {
        let sql = "SELECT * FROM users WHERE name = 'where it is' LIMIT 10";
        let out = set_where_clause(sql, "id = 1");
        assert_eq!(out, "SELECT * FROM users WHERE id = 1 LIMIT 10");
    }
}
