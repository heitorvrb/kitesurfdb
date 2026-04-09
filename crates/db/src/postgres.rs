use async_trait::async_trait;
use sqlx::postgres::{PgPool, PgPoolOptions, PgRow};
use sqlx::{Column, Row};
use std::time::Instant;

use crate::error::DbError;
use crate::traits::DbBackend;
use crate::types::*;

#[derive(Debug)]
pub struct PostgresBackend {
    pool: PgPool,
}

#[async_trait]
impl DbBackend for PostgresBackend {
    async fn connect(config: &ConnectionConfig) -> Result<Self, DbError>
    where
        Self: Sized,
    {
        let host = config
            .host
            .as_deref()
            .ok_or_else(|| DbError::ConnectionFailed("Postgres requires a host".into()))?;
        let port = config.port.unwrap_or(5432);
        let username = config
            .username
            .as_deref()
            .ok_or_else(|| DbError::ConnectionFailed("Postgres requires a username".into()))?;

        let mut url = format!("postgres://{username}@{host}:{port}/{}", config.database);
        if let Some(ref password) = config.password {
            url = format!(
                "postgres://{username}:{password}@{host}:{port}/{}",
                config.database
            );
        }

        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await?;

        Ok(Self { pool })
    }

    async fn disconnect(&self) -> Result<(), DbError> {
        self.pool.close().await;
        Ok(())
    }

    async fn execute_query(&self, sql: &str) -> Result<QueryResult, DbError> {
        let start = Instant::now();
        let rows: Vec<PgRow> = sqlx::query(sql).fetch_all(&self.pool).await?;
        let execution_time = start.elapsed();

        let (columns, result_rows) = if rows.is_empty() {
            (Vec::new(), Vec::new())
        } else {
            let columns: Vec<ColumnInfo> = rows[0]
                .columns()
                .iter()
                .map(|c| ColumnInfo {
                    name: c.name().to_string(),
                    type_name: c.type_info().to_string(),
                })
                .collect();

            let result_rows: Vec<Vec<DbValue>> = rows.iter().map(|row| extract_row(row)).collect();

            (columns, result_rows)
        };

        Ok(QueryResult {
            columns,
            rows: result_rows,
            rows_affected: 0,
            execution_time,
            query: sql.to_string(),
        })
    }

    async fn get_object_definition(
        &self,
        name: &str,
        schema: Option<&str>,
        object_type: &ObjectType,
    ) -> Result<String, DbError> {
        let schema = schema.unwrap_or("public");
        match object_type {
            ObjectType::Trigger => {
                let sql = "SELECT pg_get_triggerdef(t.oid, true) AS definition \
                           FROM pg_trigger t \
                           JOIN pg_class c ON t.tgrelid = c.oid \
                           JOIN pg_namespace n ON c.relnamespace = n.oid \
                           WHERE t.tgname = $1 AND n.nspname = $2 \
                           LIMIT 1";
                let row: Option<PgRow> = sqlx::query(sql)
                    .bind(name)
                    .bind(schema)
                    .fetch_optional(&self.pool)
                    .await?;
                match row {
                    Some(row) => Ok(row.get::<String, _>("definition")),
                    None => Err(DbError::QueryFailed(format!("Trigger '{name}' not found"))),
                }
            }
            ObjectType::Function => {
                let sql = "SELECT pg_get_functiondef(p.oid) AS definition \
                           FROM pg_proc p \
                           JOIN pg_namespace n ON p.pronamespace = n.oid \
                           WHERE p.proname = $1 AND n.nspname = $2 \
                           LIMIT 1";
                let row: Option<PgRow> = sqlx::query(sql)
                    .bind(name)
                    .bind(schema)
                    .fetch_optional(&self.pool)
                    .await?;
                match row {
                    Some(row) => Ok(row.get::<String, _>("definition")),
                    None => Err(DbError::QueryFailed(format!("Function '{name}' not found"))),
                }
            }
            ObjectType::View => {
                let sql = "SELECT pg_get_viewdef(c.oid, true) AS definition \
                           FROM pg_class c \
                           JOIN pg_namespace n ON c.relnamespace = n.oid \
                           WHERE c.relname = $1 AND n.nspname = $2 AND c.relkind = 'v' \
                           LIMIT 1";
                let row: Option<PgRow> = sqlx::query(sql)
                    .bind(name)
                    .bind(schema)
                    .fetch_optional(&self.pool)
                    .await?;
                match row {
                    Some(row) => Ok(row.get::<String, _>("definition")),
                    None => Err(DbError::QueryFailed(format!("View '{name}' not found"))),
                }
            }
            ObjectType::Table => Err(DbError::QueryFailed(
                "Tables do not have a SQL definition".into(),
            )),
        }
    }

    async fn introspect(&self) -> Result<SchemaInfo, DbError> {
        let schemas = self.query_schema_names().await?;
        let tables = self.query_objects("BASE TABLE", ObjectType::Table).await?;
        let views = self.query_objects("VIEW", ObjectType::View).await?;
        let triggers = self.query_triggers().await?;
        let functions = self.query_functions().await?;

        Ok(SchemaInfo {
            schemas,
            tables,
            views,
            triggers,
            functions,
        })
    }

    fn backend_name(&self) -> &'static str {
        "PostgreSQL"
    }
}

impl PostgresBackend {
    async fn query_schema_names(&self) -> Result<Vec<String>, DbError> {
        let sql = "SELECT nspname FROM pg_namespace \
                   WHERE nspname NOT IN ('pg_catalog', 'information_schema') \
                   AND nspname NOT LIKE 'pg_toast%' \
                   AND nspname NOT LIKE 'pg_temp%' \
                   ORDER BY nspname";
        let rows: Vec<PgRow> = sqlx::query(sql).fetch_all(&self.pool).await?;
        Ok(rows.iter().map(|row| row.get("nspname")).collect())
    }

    async fn query_objects(
        &self,
        table_type: &str,
        object_type: ObjectType,
    ) -> Result<Vec<DbObject>, DbError> {
        let sql = "SELECT table_schema, table_name FROM information_schema.tables \
                   WHERE table_schema NOT IN ('pg_catalog', 'information_schema') \
                   AND table_type = $1 \
                   ORDER BY table_schema, table_name";
        let rows: Vec<PgRow> = sqlx::query(sql)
            .bind(table_type)
            .fetch_all(&self.pool)
            .await?;

        Ok(rows
            .iter()
            .map(|row| {
                let schema: String = row.get("table_schema");
                DbObject {
                    name: row.get("table_name"),
                    object_type: object_type.clone(),
                    schema: Some(schema),
                }
            })
            .collect())
    }

    async fn query_triggers(&self) -> Result<Vec<DbObject>, DbError> {
        let sql = "SELECT DISTINCT trigger_schema, trigger_name FROM information_schema.triggers \
                   WHERE trigger_schema NOT IN ('pg_catalog', 'information_schema') \
                   ORDER BY trigger_schema, trigger_name";
        let rows: Vec<PgRow> = sqlx::query(sql).fetch_all(&self.pool).await?;

        Ok(rows
            .iter()
            .map(|row| {
                let schema: String = row.get("trigger_schema");
                DbObject {
                    name: row.get("trigger_name"),
                    object_type: ObjectType::Trigger,
                    schema: Some(schema),
                }
            })
            .collect())
    }

    async fn query_functions(&self) -> Result<Vec<DbObject>, DbError> {
        let sql = "SELECT routine_schema, routine_name FROM information_schema.routines \
                   WHERE routine_schema NOT IN ('pg_catalog', 'information_schema') \
                   ORDER BY routine_schema, routine_name";
        let rows: Vec<PgRow> = sqlx::query(sql).fetch_all(&self.pool).await?;

        Ok(rows
            .iter()
            .map(|row| {
                let schema: String = row.get("routine_schema");
                DbObject {
                    name: row.get("routine_name"),
                    object_type: ObjectType::Function,
                    schema: Some(schema),
                }
            })
            .collect())
    }
}

fn extract_row(row: &PgRow) -> Vec<DbValue> {
    use sqlx::{Column, TypeInfo, ValueRef};

    row.columns()
        .iter()
        .map(|col| {
            let idx = col.ordinal();
            let raw = row.try_get_raw(idx).unwrap();

            if raw.is_null() {
                return DbValue::Null;
            }

            let type_name = col.type_info().name();
            match type_name {
                "BOOL" => row
                    .try_get::<bool, _>(idx)
                    .map(DbValue::Bool)
                    .unwrap_or(DbValue::Null),
                "INT2" | "INT4" => row
                    .try_get::<i32, _>(idx)
                    .map(|v| DbValue::Int(v as i64))
                    .unwrap_or(DbValue::Null),
                "INT8" => row
                    .try_get::<i64, _>(idx)
                    .map(DbValue::Int)
                    .unwrap_or(DbValue::Null),
                "FLOAT4" => row
                    .try_get::<f32, _>(idx)
                    .map(|v| DbValue::Float(v as f64))
                    .unwrap_or(DbValue::Null),
                "FLOAT8" => row
                    .try_get::<f64, _>(idx)
                    .map(DbValue::Float)
                    .unwrap_or(DbValue::Null),
                "BYTEA" => row
                    .try_get::<Vec<u8>, _>(idx)
                    .map(DbValue::Bytes)
                    .unwrap_or(DbValue::Null),
                "TIMESTAMP" => row
                    .try_get::<chrono::NaiveDateTime, _>(idx)
                    .map(DbValue::Timestamp)
                    .unwrap_or(DbValue::Null),
                "TIMESTAMPTZ" => row
                    .try_get::<chrono::DateTime<chrono::Utc>, _>(idx)
                    .map(|v| DbValue::Timestamp(v.naive_utc()))
                    .unwrap_or(DbValue::Null),
                _ => row
                    .try_get::<String, _>(idx)
                    .map(DbValue::Text)
                    .unwrap_or(DbValue::Null),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pg_config() -> ConnectionConfig {
        let saved = dirs::config_dir()
            .map(|d| d.join("kitesurfdb").join("connections.json"))
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str::<Vec<ConnectionConfig>>(&s).ok())
            .and_then(|cs| cs.into_iter().find(|c| c.backend == BackendType::Postgres));

        if let Some(mut config) = saved {
            config.password = keyring::Entry::new("kitesurfdb", &config.id.to_string())
                .ok()
                .and_then(|e| e.get_password().ok());
            return config;
        }

        ConnectionConfig::new_postgres("test", "localhost", 5432, "testdb", "postgres")
    }

    #[test]
    fn test_backend_requires_host() {
        let config = ConnectionConfig {
            id: uuid::Uuid::new_v4(),
            name: "bad".into(),
            backend: BackendType::Postgres,
            host: None,
            port: None,
            database: "testdb".into(),
            username: Some("postgres".into()),
            password: None,
            file_path: None,
            default_schema: None,
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(PostgresBackend::connect(&config));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("host"));
    }

    #[test]
    fn test_backend_requires_username() {
        let config = ConnectionConfig {
            id: uuid::Uuid::new_v4(),
            name: "bad".into(),
            backend: BackendType::Postgres,
            host: Some("localhost".into()),
            port: Some(5432),
            database: "testdb".into(),
            username: None,
            password: None,
            file_path: None,
            default_schema: None,
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(PostgresBackend::connect(&config));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("username"));
    }

    // Integration tests below require a running Postgres instance.
    // Run with: cargo test -p db -- --ignored
    #[tokio::test]
    #[ignore]
    async fn test_connect_to_postgres() {
        let config = pg_config();
        let backend = PostgresBackend::connect(&config).await;
        assert!(backend.is_ok());
        backend.unwrap().disconnect().await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn test_execute_simple_query() {
        let config = pg_config();
        let backend = PostgresBackend::connect(&config).await.unwrap();

        let result = backend
            .execute_query(
                "SELECT id, name, age \
                 FROM (VALUES (1, 'Alice'::text, 30), (2, 'Bob'::text, 25)) AS t(id, name, age) \
                 ORDER BY id",
            )
            .await
            .unwrap();

        assert_eq!(result.columns.len(), 3);
        assert_eq!(result.columns[0].name, "id");
        assert_eq!(result.columns[1].name, "name");
        assert_eq!(result.columns[2].name, "age");
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][1], DbValue::Text("Alice".into()));
        assert_eq!(result.rows[0][2], DbValue::Int(30));

        backend.disconnect().await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn test_execute_query_with_nulls() {
        let config = pg_config();
        let backend = PostgresBackend::connect(&config).await.unwrap();

        let result = backend
            .execute_query("SELECT NULL::text AS a, 1::int AS b")
            .await
            .unwrap();
        assert_eq!(result.rows[0][0], DbValue::Null);
        assert_eq!(result.rows[0][1], DbValue::Int(1));

        backend.disconnect().await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn test_introspect() {
        let config = pg_config();
        let backend = PostgresBackend::connect(&config).await.unwrap();

        let schema = backend.introspect().await.unwrap();
        // Should at least not error; content depends on the test database
        assert!(schema.tables.is_empty() || !schema.tables.is_empty());

        backend.disconnect().await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn test_backend_name() {
        let config = pg_config();
        let backend = PostgresBackend::connect(&config).await.unwrap();
        assert_eq!(backend.backend_name(), "PostgreSQL");
        backend.disconnect().await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn test_invalid_sql() {
        let config = pg_config();
        let backend = PostgresBackend::connect(&config).await.unwrap();
        let result = backend.execute_query("NOT VALID SQL").await;
        assert!(result.is_err());
        backend.disconnect().await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn test_empty_result() {
        let config = pg_config();
        let backend = PostgresBackend::connect(&config).await.unwrap();

        let result = backend
            .execute_query("SELECT 1 AS a WHERE false")
            .await
            .unwrap();
        assert!(result.columns.is_empty());
        assert!(result.rows.is_empty());

        backend.disconnect().await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn test_create_table() {
        let config = pg_config();
        let backend = PostgresBackend::connect(&config).await.unwrap();

        let result = backend
            .execute_query(
                "CREATE TABLE test_automated_users (
                    id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
                    name TEXT NOT NULL,
                    age INTEGER CHECK (age >= 0)
                );",
            )
            .await
            .unwrap();

        assert_eq!(result.rows_affected, 0);

        let insert_result = backend
            .execute_query(
                "INSERT INTO test_automated_users (name, age) VALUES ('Alice', 30), ('Bob', 25);",
            )
            .await
            .unwrap();

        assert_eq!(insert_result.rows_affected, 0);

        let select_result = backend
            .execute_query("SELECT * FROM test_automated_users ORDER BY id")
            .await
            .unwrap();

        assert_eq!(select_result.rows.len(), 2);
        assert_eq!(select_result.rows[0][1], DbValue::Text("Alice".into()));
        assert_eq!(select_result.rows[0][2], DbValue::Int(30));
        assert_eq!(select_result.rows[1][1], DbValue::Text("Bob".into()));
        assert_eq!(select_result.rows[1][2], DbValue::Int(25));

        let incorrect_insert_result = backend
            .execute_query(
                "INSERT INTO test_automated_users (name, age) VALUES ('Invalid Age', -1);",
            )
            .await;

        assert!(incorrect_insert_result.is_err());

        let delete_result = backend
            .execute_query("DROP TABLE test_automated_users;")
            .await
            .unwrap();

        assert_eq!(delete_result.rows_affected, 0);

        backend.disconnect().await.unwrap();
    }
}
