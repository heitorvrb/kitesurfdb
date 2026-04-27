use db::types::{BackendType, ColumnInfo, DbValue};
use std::collections::BTreeMap;

/// Wrap a SQL identifier in double quotes, doubling any embedded `"`.
pub fn quote_ident(name: &str) -> String {
    let escaped = name.replace('"', "\"\"");
    format!("\"{escaped}\"")
}

/// Build a quoted, optionally schema-qualified table reference.
pub fn quote_qualified(schema: Option<&str>, table: &str) -> String {
    match schema {
        Some(s) if !s.is_empty() => format!("{}.{}", quote_ident(s), quote_ident(table)),
        _ => quote_ident(table),
    }
}

fn escape_single_quoted(s: &str) -> String {
    s.replace('\'', "''")
}

fn type_kind(type_name: &str) -> TypeKind {
    let t = type_name.trim().to_ascii_uppercase();
    match t.as_str() {
        "BOOL" | "BOOLEAN" => TypeKind::Bool,
        "INT2" | "INT4" | "INT8" | "INTEGER" | "BIGINT" | "SMALLINT" => TypeKind::Int,
        "FLOAT4" | "FLOAT8" | "REAL" | "DOUBLE" | "DOUBLE PRECISION" | "NUMERIC" | "DECIMAL" => {
            TypeKind::Float
        }
        "BYTEA" | "BLOB" => TypeKind::Bytes,
        _ => TypeKind::Text,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TypeKind {
    Bool,
    Int,
    Float,
    Bytes,
    Text,
}

/// Produce a SQL literal for an existing `DbValue` (used in WHERE col = literal).
pub fn format_db_value_literal(value: &DbValue, backend: BackendType) -> String {
    match value {
        DbValue::Null => "NULL".to_string(),
        DbValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        DbValue::Int(i) => i.to_string(),
        DbValue::Float(f) => f.to_string(),
        DbValue::Text(s) => format!("'{}'", escape_single_quoted(s)),
        DbValue::Timestamp(ts) => format!("'{ts}'"),
        DbValue::Bytes(b) => match backend {
            BackendType::Postgres => {
                let hex: String = b.iter().map(|byte| format!("{byte:02x}")).collect();
                format!("'\\x{hex}'::bytea")
            }
            BackendType::Sqlite => {
                let hex: String = b.iter().map(|byte| format!("{byte:02x}")).collect();
                format!("X'{hex}'")
            }
        },
    }
}

/// Format the right-hand side of a WHERE comparison: `= literal` or `IS NULL`.
pub fn format_where_match(value: &DbValue, backend: BackendType) -> String {
    match value {
        DbValue::Null => "IS NULL".to_string(),
        _ => format!("= {}", format_db_value_literal(value, backend)),
    }
}

/// Convert raw user input from an edit cell into a SQL literal, given the
/// column's reported type name.
///
/// Conventions:
/// - The literal text "NULL" (any case) → SQL `NULL`.
/// - Empty string for non-text columns → SQL `NULL`.
/// - Empty string for text columns → empty string literal `''`.
/// - Numeric/bool columns must parse cleanly; otherwise an error is returned.
pub fn format_user_input_literal(
    input: &str,
    type_name: &str,
    _backend: BackendType,
) -> Result<String, String> {
    if input.eq_ignore_ascii_case("NULL") {
        return Ok("NULL".to_string());
    }
    let kind = type_kind(type_name);
    if input.is_empty() && !matches!(kind, TypeKind::Text) {
        return Ok("NULL".to_string());
    }

    match kind {
        TypeKind::Bool => match input.trim().to_ascii_lowercase().as_str() {
            "true" | "t" | "1" | "yes" => Ok("TRUE".to_string()),
            "false" | "f" | "0" | "no" => Ok("FALSE".to_string()),
            _ => Err(format!(
                "Cannot convert `{input}` to boolean for column of type {type_name}"
            )),
        },
        TypeKind::Int => input
            .trim()
            .parse::<i64>()
            .map(|n| n.to_string())
            .map_err(|_| {
                format!("Cannot convert `{input}` to integer for column of type {type_name}")
            }),
        TypeKind::Float => input
            .trim()
            .parse::<f64>()
            .map(|n| n.to_string())
            .map_err(|_| {
                format!("Cannot convert `{input}` to number for column of type {type_name}")
            }),
        TypeKind::Bytes => Err(format!(
            "Editing binary columns is not supported (column type {type_name})"
        )),
        TypeKind::Text => Ok(format!("'{}'", escape_single_quoted(input))),
    }
}

/// Assemble one `UPDATE qualified_table SET a=1, b=2 WHERE k=3 AND ...`.
/// `set_pairs` is `[(quoted_col, literal)]`; `where_pairs` is `[(quoted_col, match_rhs)]`
/// where `match_rhs` is either `"= literal"` or `"IS NULL"`.
pub fn build_update_statement(
    qualified_table: &str,
    set_pairs: &[(String, String)],
    where_pairs: &[(String, String)],
) -> String {
    let set_clause = set_pairs
        .iter()
        .map(|(col, lit)| format!("{col} = {lit}"))
        .collect::<Vec<_>>()
        .join(", ");
    let where_clause = where_pairs
        .iter()
        .map(|(col, rhs)| format!("{col} {rhs}"))
        .collect::<Vec<_>>()
        .join(" AND ");
    format!("UPDATE {qualified_table} SET {set_clause} WHERE {where_clause}")
}

/// Build one UPDATE statement per row that has pending edits, in stable order.
///
/// `primary_keys` is the list of PK column names, ordered by PK position.
/// If empty, the WHERE clause is built from **all** columns of the original
/// row (caller is expected to confirm with the user before invoking this).
pub fn build_updates_for_tab(
    qualified_table: &str,
    columns: &[ColumnInfo],
    rows: &[Vec<DbValue>],
    edited_cells: &BTreeMap<usize, BTreeMap<String, String>>,
    primary_keys: &[String],
    backend: BackendType,
) -> Result<Vec<String>, String> {
    let col_index: BTreeMap<&str, usize> = columns
        .iter()
        .enumerate()
        .map(|(i, c)| (c.name.as_str(), i))
        .collect();

    let mut statements = Vec::with_capacity(edited_cells.len());

    for (row_idx, edits) in edited_cells {
        let row = rows.get(*row_idx).ok_or_else(|| {
            format!("Row {row_idx} no longer exists in the result; refresh and try again")
        })?;

        // SET clauses
        let mut set_pairs: Vec<(String, String)> = Vec::with_capacity(edits.len());
        for (col_name, new_value) in edits {
            let col_info = col_index
                .get(col_name.as_str())
                .map(|i| &columns[*i])
                .ok_or_else(|| format!("Unknown column `{col_name}` in edits"))?;
            let literal = format_user_input_literal(new_value, &col_info.type_name, backend.clone())
                .map_err(|e| format!("Row {row_idx}, column `{col_name}`: {e}"))?;
            set_pairs.push((quote_ident(col_name), literal));
        }

        // WHERE clauses
        let mut where_pairs: Vec<(String, String)> = Vec::new();
        if primary_keys.is_empty() {
            for (idx, col) in columns.iter().enumerate() {
                let original = &row[idx];
                where_pairs.push((
                    quote_ident(&col.name),
                    format_where_match(original, backend.clone()),
                ));
            }
        } else {
            for pk in primary_keys {
                let idx = col_index.get(pk.as_str()).ok_or_else(|| {
                    format!(
                        "Primary key column `{pk}` is not in the result columns; \
                         the SELECT must include all PK columns to enable editing"
                    )
                })?;
                let original = &row[*idx];
                where_pairs.push((
                    quote_ident(pk),
                    format_where_match(original, backend.clone()),
                ));
            }
        }

        statements.push(build_update_statement(
            qualified_table,
            &set_pairs,
            &where_pairs,
        ));
    }

    Ok(statements)
}

#[cfg(test)]
mod tests {
    use super::*;
    use db::types::ColumnInfo;

    fn col(name: &str, ty: &str) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            type_name: ty.into(),
        }
    }

    #[test]
    fn quote_ident_basic_and_escapes() {
        assert_eq!(quote_ident("users"), "\"users\"");
        assert_eq!(quote_ident("we\"ird"), "\"we\"\"ird\"");
    }

    #[test]
    fn quote_qualified_handles_schema_and_no_schema() {
        assert_eq!(quote_qualified(Some("public"), "users"), "\"public\".\"users\"");
        assert_eq!(quote_qualified(None, "users"), "\"users\"");
        assert_eq!(quote_qualified(Some(""), "users"), "\"users\"");
    }

    #[test]
    fn format_db_value_literal_for_each_variant() {
        assert_eq!(
            format_db_value_literal(&DbValue::Null, BackendType::Sqlite),
            "NULL"
        );
        assert_eq!(
            format_db_value_literal(&DbValue::Bool(true), BackendType::Sqlite),
            "TRUE"
        );
        assert_eq!(
            format_db_value_literal(&DbValue::Bool(false), BackendType::Postgres),
            "FALSE"
        );
        assert_eq!(
            format_db_value_literal(&DbValue::Int(42), BackendType::Sqlite),
            "42"
        );
        assert_eq!(
            format_db_value_literal(&DbValue::Float(3.5), BackendType::Sqlite),
            "3.5"
        );
        assert_eq!(
            format_db_value_literal(&DbValue::Text("O'Brien".into()), BackendType::Sqlite),
            "'O''Brien'"
        );
        assert_eq!(
            format_db_value_literal(&DbValue::Bytes(vec![0xAB, 0xCD]), BackendType::Sqlite),
            "X'abcd'"
        );
        assert_eq!(
            format_db_value_literal(&DbValue::Bytes(vec![0xAB, 0xCD]), BackendType::Postgres),
            "'\\xabcd'::bytea"
        );
    }

    #[test]
    fn format_where_match_uses_is_null() {
        assert_eq!(
            format_where_match(&DbValue::Null, BackendType::Sqlite),
            "IS NULL"
        );
        assert_eq!(
            format_where_match(&DbValue::Int(7), BackendType::Sqlite),
            "= 7"
        );
    }

    #[test]
    fn format_user_input_null_marker() {
        let lit = format_user_input_literal("NULL", "INTEGER", BackendType::Sqlite).unwrap();
        assert_eq!(lit, "NULL");
        let lit = format_user_input_literal("null", "TEXT", BackendType::Sqlite).unwrap();
        assert_eq!(lit, "NULL");
    }

    #[test]
    fn format_user_input_empty_string_int_to_null() {
        let lit = format_user_input_literal("", "INTEGER", BackendType::Sqlite).unwrap();
        assert_eq!(lit, "NULL");
    }

    #[test]
    fn format_user_input_empty_string_text_to_empty_literal() {
        let lit = format_user_input_literal("", "TEXT", BackendType::Sqlite).unwrap();
        assert_eq!(lit, "''");
    }

    #[test]
    fn format_user_input_text_escapes_quotes() {
        let lit = format_user_input_literal("it's", "TEXT", BackendType::Sqlite).unwrap();
        assert_eq!(lit, "'it''s'");
    }

    #[test]
    fn format_user_input_int_rejects_garbage() {
        let err = format_user_input_literal("abc", "INTEGER", BackendType::Sqlite).unwrap_err();
        assert!(err.contains("integer"), "got: {err}");
    }

    #[test]
    fn format_user_input_float_accepts_negative_decimal() {
        let lit = format_user_input_literal("-3.14", "FLOAT8", BackendType::Postgres).unwrap();
        assert_eq!(lit, "-3.14");
    }

    #[test]
    fn format_user_input_bool_accepts_common_forms() {
        for s in ["true", "T", "1", "yes"] {
            let lit = format_user_input_literal(s, "BOOL", BackendType::Postgres).unwrap();
            assert_eq!(lit, "TRUE");
        }
        for s in ["false", "F", "0", "no"] {
            let lit = format_user_input_literal(s, "BOOLEAN", BackendType::Sqlite).unwrap();
            assert_eq!(lit, "FALSE");
        }
    }

    #[test]
    fn build_update_statement_basic() {
        let stmt = build_update_statement(
            "\"users\"",
            &[("\"name\"".into(), "'Bob'".into())],
            &[("\"id\"".into(), "= 5".into())],
        );
        assert_eq!(stmt, "UPDATE \"users\" SET \"name\" = 'Bob' WHERE \"id\" = 5");
    }

    #[test]
    fn build_update_statement_composite_pk_and_multi_set() {
        let stmt = build_update_statement(
            "\"public\".\"memberships\"",
            &[
                ("\"role\"".into(), "'admin'".into()),
                ("\"active\"".into(), "TRUE".into()),
            ],
            &[
                ("\"user_id\"".into(), "= 1".into()),
                ("\"group_id\"".into(), "= 2".into()),
            ],
        );
        assert_eq!(
            stmt,
            "UPDATE \"public\".\"memberships\" SET \"role\" = 'admin', \"active\" = TRUE \
             WHERE \"user_id\" = 1 AND \"group_id\" = 2"
        );
    }

    #[test]
    fn build_updates_for_tab_with_pk() {
        let columns = vec![col("id", "INTEGER"), col("name", "TEXT")];
        let rows = vec![
            vec![DbValue::Int(1), DbValue::Text("Alice".into())],
            vec![DbValue::Int(2), DbValue::Text("Bob".into())],
        ];
        let mut edits: BTreeMap<usize, BTreeMap<String, String>> = BTreeMap::new();
        let mut row0 = BTreeMap::new();
        row0.insert("name".into(), "Alicia".into());
        edits.insert(0, row0);
        let mut row1 = BTreeMap::new();
        row1.insert("name".into(), "Robert".into());
        edits.insert(1, row1);

        let stmts = build_updates_for_tab(
            "\"users\"",
            &columns,
            &rows,
            &edits,
            &["id".into()],
            BackendType::Sqlite,
        )
        .unwrap();

        assert_eq!(
            stmts,
            vec![
                "UPDATE \"users\" SET \"name\" = 'Alicia' WHERE \"id\" = 1".to_string(),
                "UPDATE \"users\" SET \"name\" = 'Robert' WHERE \"id\" = 2".to_string(),
            ]
        );
    }

    #[test]
    fn build_updates_for_tab_no_pk_uses_all_columns_with_is_null_for_nulls() {
        let columns = vec![col("a", "INTEGER"), col("b", "TEXT")];
        let rows = vec![vec![DbValue::Int(1), DbValue::Null]];
        let mut edits: BTreeMap<usize, BTreeMap<String, String>> = BTreeMap::new();
        let mut row0 = BTreeMap::new();
        row0.insert("b".into(), "hi".into());
        edits.insert(0, row0);

        let stmts = build_updates_for_tab(
            "\"t\"",
            &columns,
            &rows,
            &edits,
            &[],
            BackendType::Sqlite,
        )
        .unwrap();

        assert_eq!(
            stmts,
            vec!["UPDATE \"t\" SET \"b\" = 'hi' WHERE \"a\" = 1 AND \"b\" IS NULL".to_string()]
        );
    }

    #[test]
    fn build_updates_for_tab_errors_on_missing_pk_column() {
        let columns = vec![col("name", "TEXT")];
        let rows = vec![vec![DbValue::Text("x".into())]];
        let mut edits: BTreeMap<usize, BTreeMap<String, String>> = BTreeMap::new();
        let mut row0 = BTreeMap::new();
        row0.insert("name".into(), "y".into());
        edits.insert(0, row0);

        let err = build_updates_for_tab(
            "\"t\"",
            &columns,
            &rows,
            &edits,
            &["id".into()],
            BackendType::Sqlite,
        )
        .unwrap_err();
        assert!(err.contains("`id`"), "got: {err}");
    }

    #[test]
    fn build_updates_for_tab_propagates_invalid_value_error() {
        let columns = vec![col("id", "INTEGER"), col("age", "INTEGER")];
        let rows = vec![vec![DbValue::Int(1), DbValue::Int(30)]];
        let mut edits: BTreeMap<usize, BTreeMap<String, String>> = BTreeMap::new();
        let mut row0 = BTreeMap::new();
        row0.insert("age".into(), "abc".into());
        edits.insert(0, row0);

        let err = build_updates_for_tab(
            "\"t\"",
            &columns,
            &rows,
            &edits,
            &["id".into()],
            BackendType::Sqlite,
        )
        .unwrap_err();
        assert!(err.contains("integer"), "got: {err}");
    }
}
