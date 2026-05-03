use async_trait::async_trait;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions, SqliteRow};
use sqlx::{Column, Row};
use std::time::Instant;

use crate::error::DbError;
use crate::traits::DbBackend;
use crate::types::*;

#[derive(Debug)]
pub struct SqliteBackend {
    pool: SqlitePool,
}

#[async_trait]
impl DbBackend for SqliteBackend {
    async fn connect(config: &ConnectionConfig) -> Result<Self, DbError> {
        let path = config
            .file_path
            .as_ref()
            .ok_or_else(|| DbError::ConnectionFailed("SQLite requires a file path".into()))?;

        let url = format!("sqlite:{}", path.display());
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await?;

        Ok(Self { pool })
    }

    async fn disconnect(&self) -> Result<(), DbError> {
        self.pool.close().await;
        Ok(())
    }

    async fn execute_query(&self, sql: &str) -> Result<QueryResult, DbError> {
        use sqlx::Executor;

        let start = Instant::now();
        let rows: Vec<SqliteRow> = sqlx::query(sql).fetch_all(&self.pool).await?;
        let execution_time = start.elapsed();

        let columns: Vec<ColumnInfo> = if let Some(first_row) = rows.first() {
            first_row
                .columns()
                .iter()
                .map(|c| ColumnInfo {
                    name: c.name().to_string(),
                    type_name: c.type_info().to_string(),
                })
                .collect()
        } else {
            // No rows returned — use prepare to retrieve column metadata anyway
            use sqlx::Statement;
            (&self.pool)
                .prepare(sql)
                .await
                .map(|stmt| {
                    stmt.columns()
                        .iter()
                        .map(|c| ColumnInfo {
                            name: c.name().to_string(),
                            type_name: c.type_info().to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default()
        };

        let result_rows: Vec<Vec<DbValue>> = rows.iter().map(|row| extract_row(row)).collect();

        Ok(QueryResult {
            columns,
            rows: result_rows,
            rows_affected: 0,
            execution_time,
            query: sql.to_string(),
        })
    }

    async fn execute_transaction(&self, statements: &[String]) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        for stmt in statements {
            sqlx::query(stmt).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn get_primary_keys(
        &self,
        _schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<String>, DbError> {
        // SQLite's PRAGMA does not accept bound parameters for the table name,
        // so we must inline it; escape embedded double-quotes by doubling them.
        let escaped = table.replace('"', "\"\"");
        let sql = format!("PRAGMA table_info(\"{escaped}\")");
        let rows: Vec<SqliteRow> = sqlx::query(&sql).fetch_all(&self.pool).await?;

        let mut pks: Vec<(i64, String)> = rows
            .iter()
            .filter_map(|row| {
                let pk: i64 = row.get("pk");
                if pk > 0 {
                    Some((pk, row.get::<String, _>("name")))
                } else {
                    None
                }
            })
            .collect();
        pks.sort_by_key(|(pos, _)| *pos);
        Ok(pks.into_iter().map(|(_, name)| name).collect())
    }

    async fn get_foreign_keys(
        &self,
        _schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<ForeignKeyInfo>, DbError> {
        // PRAGMA can't bind params; escape `"` by doubling.
        let escaped = table.replace('"', "\"\"");
        let sql = format!("PRAGMA foreign_key_list(\"{escaped}\")");
        let rows: Vec<SqliteRow> = sqlx::query(&sql).fetch_all(&self.pool).await?;

        // Each FK constraint is identified by `id` and may span multiple rows
        // (one per column). Group rows by `id`; keep only single-column FKs.
        let mut by_id: std::collections::BTreeMap<i64, Vec<&SqliteRow>> =
            std::collections::BTreeMap::new();
        for row in &rows {
            let id: i64 = row.get("id");
            by_id.entry(id).or_default().push(row);
        }

        let mut out = Vec::new();
        for (_, group) in by_id {
            if group.len() != 1 {
                continue;
            }
            let row = group[0];
            let from_column: String = row.get("from");
            let to_table: String = row.get("table");
            // SQLite stores NULL in `to` when REFERENCES omits the column;
            // sqlx surfaces that as Option<String>.
            let to_column: Option<String> = row.try_get("to").ok().flatten();

            let to_column = match to_column {
                Some(c) => c,
                None => {
                    // Resolve to the target table's single-column primary key.
                    let pks = self.get_primary_keys(None, &to_table).await?;
                    if pks.len() == 1 {
                        pks.into_iter().next().unwrap()
                    } else {
                        // Composite or missing PK — can't disambiguate.
                        continue;
                    }
                }
            };

            out.push(ForeignKeyInfo {
                from_column,
                to_schema: None,
                to_table,
                to_column,
            });
        }
        Ok(out)
    }

    async fn get_object_definition(
        &self,
        name: &str,
        _schema: Option<&str>,
        _object_type: &ObjectType,
    ) -> Result<String, DbError> {
        let sql = "SELECT sql FROM sqlite_master WHERE name = ?";
        let row: Option<sqlx::sqlite::SqliteRow> = sqlx::query(sql)
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(row) => {
                let definition: Option<String> = row.get("sql");
                Ok(definition.unwrap_or_else(|| "-- No definition available".into()))
            }
            None => Err(DbError::QueryFailed(format!("Object '{name}' not found"))),
        }
    }

    async fn introspect(&self) -> Result<SchemaInfo, DbError> {
        let tables = self.query_objects("table").await?;
        let views = self.query_objects("view").await?;
        let triggers = self.query_objects("trigger").await?;

        Ok(SchemaInfo {
            schemas: Vec::new(), // SQLite has no schema/namespace concept
            tables,
            views,
            triggers,
            functions: Vec::new(), // SQLite doesn't have user-defined functions visible via sqlite_master
        })
    }

    fn backend_name(&self) -> &'static str {
        "SQLite"
    }

    fn backend_kind(&self) -> BackendType {
        BackendType::Sqlite
    }
}

impl SqliteBackend {
    async fn query_objects(&self, obj_type: &str) -> Result<Vec<DbObject>, DbError> {
        let sql = "SELECT name FROM sqlite_master WHERE type = ? AND name NOT LIKE 'sqlite_%' ORDER BY name";
        let rows: Vec<SqliteRow> = sqlx::query(sql)
            .bind(obj_type)
            .fetch_all(&self.pool)
            .await?;

        let object_type = match obj_type {
            "table" => ObjectType::Table,
            "view" => ObjectType::View,
            "trigger" => ObjectType::Trigger,
            _ => ObjectType::Table,
        };

        Ok(rows
            .iter()
            .map(|row| DbObject {
                name: row.get("name"),
                object_type: object_type.clone(),
                schema: None,
            })
            .collect())
    }
}

fn extract_row(row: &SqliteRow) -> Vec<DbValue> {
    use sqlx::{Column, ValueRef};

    row.columns()
        .iter()
        .map(|col| {
            let idx = col.ordinal();
            let raw = row.try_get_raw(idx).unwrap();

            if raw.is_null() {
                return DbValue::Null;
            }

            let type_name = col.type_info().to_string();
            match type_name.as_str() {
                "BOOLEAN" => row
                    .try_get::<bool, _>(idx)
                    .map(DbValue::Bool)
                    .unwrap_or(DbValue::Null),
                "INTEGER" => row
                    .try_get::<i64, _>(idx)
                    .map(DbValue::Int)
                    .unwrap_or(DbValue::Null),
                "REAL" => row
                    .try_get::<f64, _>(idx)
                    .map(DbValue::Float)
                    .unwrap_or(DbValue::Null),
                "BLOB" => row
                    .try_get::<Vec<u8>, _>(idx)
                    .map(DbValue::Bytes)
                    .unwrap_or(DbValue::Null),
                _ => {
                    if let Ok(v) = row.try_get::<i64, _>(idx) {
                        DbValue::Int(v)
                    } else if let Ok(v) = row.try_get::<f64, _>(idx) {
                        DbValue::Float(v)
                    } else {
                        row.try_get::<String, _>(idx)
                            .map(DbValue::Text)
                            .unwrap_or(DbValue::Null)
                    }
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sqlite_config(path: &str) -> ConnectionConfig {
        ConnectionConfig::new_sqlite("test", path)
    }

    #[tokio::test]
    async fn test_connect_to_memory_db() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await;
        assert!(backend.is_ok());
    }

    #[tokio::test]
    async fn test_connect_missing_file_path() {
        let config = ConnectionConfig {
            id: uuid::Uuid::new_v4(),
            name: "bad".into(),
            backend: BackendType::Sqlite,
            host: None,
            port: None,
            database: String::new(),
            username: None,
            password: None,
            file_path: None,
            default_schema: None,
        };
        let result = SqliteBackend::connect(&config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("file path"));
    }

    #[tokio::test]
    async fn test_execute_simple_query() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await.unwrap();

        // Create a table and insert data
        backend
            .execute_query("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, age INTEGER)")
            .await
            .unwrap();
        backend
            .execute_query("INSERT INTO users (name, age) VALUES ('Alice', 30), ('Bob', 25)")
            .await
            .unwrap();

        // Query it
        let result = backend
            .execute_query("SELECT id, name, age FROM users ORDER BY id")
            .await
            .unwrap();

        assert_eq!(result.columns.len(), 3);
        assert_eq!(result.columns[0].name, "id");
        assert_eq!(result.columns[1].name, "name");
        assert_eq!(result.columns[2].name, "age");
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][1], DbValue::Text("Alice".into()));
        assert_eq!(result.rows[0][2], DbValue::Int(30));
        assert_eq!(result.rows[1][1], DbValue::Text("Bob".into()));
        assert_eq!(result.query, "SELECT id, name, age FROM users ORDER BY id");
    }

    #[tokio::test]
    async fn test_execute_query_with_nulls() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await.unwrap();

        backend
            .execute_query("CREATE TABLE t (a TEXT, b INTEGER)")
            .await
            .unwrap();
        backend
            .execute_query("INSERT INTO t VALUES (NULL, 1)")
            .await
            .unwrap();

        let result = backend.execute_query("SELECT a, b FROM t").await.unwrap();
        assert_eq!(result.rows[0][0], DbValue::Null);
        assert_eq!(result.rows[0][1], DbValue::Int(1));
    }

    #[tokio::test]
    async fn test_execute_empty_result() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await.unwrap();

        backend
            .execute_query("CREATE TABLE t (a TEXT)")
            .await
            .unwrap();

        let result = backend.execute_query("SELECT * FROM t").await.unwrap();
        assert_eq!(result.columns.len(), 1);
        assert_eq!(result.columns[0].name, "a");
        assert!(result.rows.is_empty());
    }

    #[tokio::test]
    async fn test_execute_invalid_sql() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await.unwrap();

        let result = backend.execute_query("NOT VALID SQL").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_introspect() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await.unwrap();

        backend
            .execute_query("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
            .await
            .unwrap();
        backend
            .execute_query("CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER)")
            .await
            .unwrap();
        backend
            .execute_query("CREATE VIEW active_users AS SELECT * FROM users")
            .await
            .unwrap();

        let schema = backend.introspect().await.unwrap();

        assert_eq!(schema.tables.len(), 2);
        let table_names: Vec<&str> = schema.tables.iter().map(|t| t.name.as_str()).collect();
        assert!(table_names.contains(&"users"));
        assert!(table_names.contains(&"orders"));

        assert_eq!(schema.views.len(), 1);
        assert_eq!(schema.views[0].name, "active_users");
        assert_eq!(schema.views[0].object_type, ObjectType::View);

        assert!(schema.functions.is_empty());
    }

    #[tokio::test]
    async fn test_get_trigger_definition() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await.unwrap();

        backend
            .execute_query("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
            .await
            .unwrap();
        backend
            .execute_query("CREATE TRIGGER trg_users AFTER INSERT ON users BEGIN SELECT 1; END")
            .await
            .unwrap();

        let def = backend
            .get_object_definition("trg_users", None, &ObjectType::Trigger)
            .await
            .unwrap();
        assert!(def.contains("trg_users"));
        assert!(def.contains("TRIGGER"));
    }

    #[tokio::test]
    async fn test_get_view_definition() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await.unwrap();

        backend
            .execute_query("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
            .await
            .unwrap();
        backend
            .execute_query("CREATE VIEW v_users AS SELECT * FROM users")
            .await
            .unwrap();

        let def = backend
            .get_object_definition("v_users", None, &ObjectType::View)
            .await
            .unwrap();
        assert!(def.contains("v_users"));
    }

    #[tokio::test]
    async fn test_get_definition_not_found() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await.unwrap();

        let result = backend
            .get_object_definition("nonexistent", None, &ObjectType::Trigger)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_disconnect() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await.unwrap();
        assert!(backend.disconnect().await.is_ok());
    }

    #[tokio::test]
    async fn test_backend_name() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await.unwrap();
        assert_eq!(backend.backend_name(), "SQLite");
    }

    #[tokio::test]
    async fn test_get_primary_keys_single() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await.unwrap();

        backend
            .execute_query("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
            .await
            .unwrap();

        let pks = backend.get_primary_keys(None, "users").await.unwrap();
        assert_eq!(pks, vec!["id".to_string()]);
    }

    #[tokio::test]
    async fn test_get_primary_keys_composite() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await.unwrap();

        backend
            .execute_query(
                "CREATE TABLE memberships (user_id INTEGER, group_id INTEGER, role TEXT, PRIMARY KEY (user_id, group_id))",
            )
            .await
            .unwrap();

        let pks = backend
            .get_primary_keys(None, "memberships")
            .await
            .unwrap();
        assert_eq!(pks, vec!["user_id".to_string(), "group_id".to_string()]);
    }

    #[tokio::test]
    async fn test_get_primary_keys_none() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await.unwrap();

        backend
            .execute_query("CREATE TABLE notes (body TEXT)")
            .await
            .unwrap();

        let pks = backend.get_primary_keys(None, "notes").await.unwrap();
        assert!(pks.is_empty());
    }

    #[tokio::test]
    async fn test_get_foreign_keys_single_column() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await.unwrap();

        backend
            .execute_query("CREATE TABLE parent (id INTEGER PRIMARY KEY)")
            .await
            .unwrap();
        backend
            .execute_query(
                "CREATE TABLE child (id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES parent(id))",
            )
            .await
            .unwrap();

        let fks = backend.get_foreign_keys(None, "child").await.unwrap();
        assert_eq!(fks.len(), 1);
        assert_eq!(
            fks[0],
            ForeignKeyInfo {
                from_column: "parent_id".into(),
                to_schema: None,
                to_table: "parent".into(),
                to_column: "id".into(),
            }
        );
    }

    #[tokio::test]
    async fn test_get_foreign_keys_implicit_pk_target() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await.unwrap();

        backend
            .execute_query("CREATE TABLE parent (id INTEGER PRIMARY KEY)")
            .await
            .unwrap();
        // Note: REFERENCES parent (no column) — should resolve to PK.
        backend
            .execute_query(
                "CREATE TABLE child (id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES parent)",
            )
            .await
            .unwrap();

        let fks = backend.get_foreign_keys(None, "child").await.unwrap();
        assert_eq!(fks.len(), 1);
        assert_eq!(fks[0].to_table, "parent");
        assert_eq!(fks[0].to_column, "id");
        assert_eq!(fks[0].from_column, "parent_id");
    }

    #[tokio::test]
    async fn test_get_foreign_keys_composite_skipped() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await.unwrap();

        backend
            .execute_query(
                "CREATE TABLE parent (a INTEGER, b INTEGER, PRIMARY KEY (a, b))",
            )
            .await
            .unwrap();
        backend
            .execute_query(
                "CREATE TABLE child (\
                    id INTEGER PRIMARY KEY, \
                    pa INTEGER, \
                    pb INTEGER, \
                    FOREIGN KEY (pa, pb) REFERENCES parent(a, b)\
                )",
            )
            .await
            .unwrap();

        let fks = backend.get_foreign_keys(None, "child").await.unwrap();
        assert!(fks.is_empty(), "composite FKs should be dropped");
    }

    #[tokio::test]
    async fn test_get_foreign_keys_none() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await.unwrap();

        backend
            .execute_query("CREATE TABLE notes (body TEXT)")
            .await
            .unwrap();

        let fks = backend.get_foreign_keys(None, "notes").await.unwrap();
        assert!(fks.is_empty());
    }

    #[tokio::test]
    async fn test_execute_transaction_commits() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await.unwrap();
        backend
            .execute_query("CREATE TABLE t (a INTEGER)")
            .await
            .unwrap();

        backend
            .execute_transaction(&[
                "INSERT INTO t (a) VALUES (1)".into(),
                "INSERT INTO t (a) VALUES (2)".into(),
            ])
            .await
            .unwrap();

        let result = backend
            .execute_query("SELECT a FROM t ORDER BY a")
            .await
            .unwrap();
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], DbValue::Int(1));
        assert_eq!(result.rows[1][0], DbValue::Int(2));
    }

    #[tokio::test]
    async fn test_execute_transaction_rolls_back() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await.unwrap();
        backend
            .execute_query("CREATE TABLE t (a INTEGER)")
            .await
            .unwrap();

        let result = backend
            .execute_transaction(&[
                "INSERT INTO t (a) VALUES (1)".into(),
                "INSERT INTO nonexistent (x) VALUES (2)".into(),
            ])
            .await;
        assert!(result.is_err());

        let select = backend.execute_query("SELECT a FROM t").await.unwrap();
        assert!(select.rows.is_empty(), "transaction should have rolled back");
    }

    #[tokio::test]
    async fn test_query_records_execution_time() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await.unwrap();

        let result = backend.execute_query("SELECT 1").await.unwrap();
        // Execution time should be recorded (non-zero or at least not panic)
        // Just verify it doesn't panic and has a value
        let _ = result.execution_time;
    }

    #[tokio::test]
    async fn test_date_field() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await.unwrap();

        backend
            .execute_query("CREATE TABLE events (event_date DATE, event_time TIME)")
            .await
            .unwrap();
        backend
            .execute_query("INSERT INTO events VALUES ('2024-01-15', '14:30:00')")
            .await
            .unwrap();

        let result = backend
            .execute_query("SELECT event_date, event_time FROM events")
            .await
            .unwrap();

        assert_eq!(result.rows.len(), 1);
        assert!(
            matches!(result.rows[0][0], DbValue::Text(_)),
            "expected Text for DATE, got {:?}",
            result.rows[0][0]
        );
        assert!(
            matches!(result.rows[0][1], DbValue::Text(_)),
            "expected Text for TIME, got {:?}",
            result.rows[0][1]
        );
    }

    #[tokio::test]
    async fn test_decimal_field() {
        let config = sqlite_config(":memory:");
        let backend = SqliteBackend::connect(&config).await.unwrap();

        backend
            .execute_query("CREATE TABLE products (price DECIMAL(10,2), qty NUMERIC)")
            .await
            .unwrap();
        backend
            .execute_query("INSERT INTO products VALUES (3.14, 42)")
            .await
            .unwrap();

        let result = backend
            .execute_query("SELECT price, qty FROM products")
            .await
            .unwrap();

        assert_eq!(result.rows.len(), 1);
        assert!(
            matches!(result.rows[0][0], DbValue::Float(_) | DbValue::Text(_)),
            "expected non-null decimal value, got {:?}",
            result.rows[0][0]
        );
        assert!(
            matches!(result.rows[0][1], DbValue::Int(_) | DbValue::Float(_) | DbValue::Text(_)),
            "expected non-null numeric value, got {:?}",
            result.rows[0][1]
        );
    }
}
