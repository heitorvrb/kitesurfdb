use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BackendType {
    Postgres,
    Sqlite,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub id: Uuid,
    pub name: String,
    pub backend: BackendType,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: String,
    pub username: Option<String>,
    /// Password is not serialized — stored in OS keyring via `keyring` crate.
    #[serde(skip)]
    pub password: Option<String>,
    pub file_path: Option<PathBuf>,
}

impl ConnectionConfig {
    pub fn new_sqlite(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            backend: BackendType::Sqlite,
            host: None,
            port: None,
            database: String::new(),
            username: None,
            password: None,
            file_path: Some(path.into()),
        }
    }

    pub fn new_postgres(
        name: impl Into<String>,
        host: impl Into<String>,
        port: u16,
        database: impl Into<String>,
        username: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            backend: BackendType::Postgres,
            host: Some(host.into()),
            port: Some(port),
            database: database.into(),
            username: Some(username.into()),
            password: None,
            file_path: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueryResult {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<Vec<DbValue>>,
    pub rows_affected: u64,
    pub execution_time: Duration,
    pub query: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnInfo {
    pub name: String,
    pub type_name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DbValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
    Timestamp(NaiveDateTime),
}

impl fmt::Display for DbValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbValue::Null => write!(f, "NULL"),
            DbValue::Bool(b) => write!(f, "{b}"),
            DbValue::Int(i) => write!(f, "{i}"),
            DbValue::Float(v) => write!(f, "{v}"),
            DbValue::Text(s) => write!(f, "{s}"),
            DbValue::Bytes(b) => write!(f, "[{} bytes]", b.len()),
            DbValue::Timestamp(ts) => write!(f, "{ts}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SchemaInfo {
    pub tables: Vec<DbObject>,
    pub views: Vec<DbObject>,
    pub triggers: Vec<DbObject>,
    pub functions: Vec<DbObject>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ObjectType {
    Table,
    View,
    Trigger,
    Function,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DbObject {
    pub name: String,
    pub object_type: ObjectType,
    pub schema: Option<String>,
}

impl DbObject {
    /// Returns `"schema"."name"` if schema is present, otherwise `"name"`.
    pub fn quoted_qualified_name(&self) -> String {
        match &self.schema {
            Some(s) => format!("\"{s}\".\"{}\"", self.name),
            None => format!("\"{}\"", self.name),
        }
    }

    /// Returns `schema.name` if schema is present, otherwise just `name`.
    pub fn display_qualified_name(&self) -> String {
        match &self.schema {
            Some(s) => format!("{s}.{}", self.name),
            None => self.name.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_db_value_display() {
        assert_eq!(DbValue::Null.to_string(), "NULL");
        assert_eq!(DbValue::Bool(true).to_string(), "true");
        assert_eq!(DbValue::Int(42).to_string(), "42");
        assert_eq!(DbValue::Float(3.14).to_string(), "3.14");
        assert_eq!(DbValue::Text("hello".into()).to_string(), "hello");
        assert_eq!(DbValue::Bytes(vec![1, 2, 3]).to_string(), "[3 bytes]");

        let ts = NaiveDateTime::parse_from_str("2026-01-15 10:30:00", "%Y-%m-%d %H:%M:%S").unwrap();
        assert_eq!(DbValue::Timestamp(ts).to_string(), "2026-01-15 10:30:00");
    }

    #[test]
    fn test_db_value_equality() {
        assert_eq!(DbValue::Null, DbValue::Null);
        assert_eq!(DbValue::Int(1), DbValue::Int(1));
        assert_ne!(DbValue::Int(1), DbValue::Int(2));
        assert_ne!(DbValue::Int(1), DbValue::Text("1".into()));
    }

    #[test]
    fn test_connection_config_new_sqlite() {
        let config = ConnectionConfig::new_sqlite("test db", "/tmp/test.db");
        assert_eq!(config.name, "test db");
        assert_eq!(config.backend, BackendType::Sqlite);
        assert_eq!(config.file_path, Some(PathBuf::from("/tmp/test.db")));
        assert!(config.host.is_none());
        assert!(config.password.is_none());
    }

    #[test]
    fn test_connection_config_new_postgres() {
        let config = ConnectionConfig::new_postgres("prod", "localhost", 5432, "mydb", "admin");
        assert_eq!(config.name, "prod");
        assert_eq!(config.backend, BackendType::Postgres);
        assert_eq!(config.host, Some("localhost".into()));
        assert_eq!(config.port, Some(5432));
        assert_eq!(config.database, "mydb");
        assert_eq!(config.username, Some("admin".into()));
    }

    #[test]
    fn test_connection_config_password_not_serialized() {
        let mut config = ConnectionConfig::new_sqlite("test", "/tmp/test.db");
        config.password = Some("secret".into());

        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("secret"));

        let deserialized: ConnectionConfig = serde_json::from_str(&json).unwrap();
        assert!(deserialized.password.is_none());
    }

    #[test]
    fn test_backend_type_serialization() {
        let pg = BackendType::Postgres;
        let json = serde_json::to_string(&pg).unwrap();
        assert_eq!(json, "\"Postgres\"");

        let sqlite: BackendType = serde_json::from_str("\"Sqlite\"").unwrap();
        assert_eq!(sqlite, BackendType::Sqlite);
    }
}
