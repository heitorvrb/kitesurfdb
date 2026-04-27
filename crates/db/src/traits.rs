use async_trait::async_trait;

use crate::error::DbError;
use crate::types::{BackendType, ConnectionConfig, ObjectType, QueryResult, SchemaInfo};

#[async_trait]
pub trait DbBackend: Send + Sync {
    async fn connect(config: &ConnectionConfig) -> Result<Self, DbError>
    where
        Self: Sized;

    async fn disconnect(&self) -> Result<(), DbError>;

    async fn execute_query(&self, sql: &str) -> Result<QueryResult, DbError>;

    /// Run multiple statements as a single transaction (BEGIN/COMMIT).
    /// Rolls back on the first error.
    async fn execute_transaction(&self, statements: &[String]) -> Result<(), DbError>;

    /// Returns the primary key column names for a table, in PK-position order.
    /// Empty Vec means the table has no primary key.
    async fn get_primary_keys(
        &self,
        schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<String>, DbError>;

    async fn introspect(&self) -> Result<SchemaInfo, DbError>;

    async fn get_object_definition(
        &self,
        name: &str,
        schema: Option<&str>,
        object_type: &ObjectType,
    ) -> Result<String, DbError>;

    fn backend_name(&self) -> &'static str;

    fn backend_kind(&self) -> BackendType;
}
