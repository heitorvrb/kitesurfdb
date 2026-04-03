use async_trait::async_trait;

use crate::error::DbError;
use crate::types::{ConnectionConfig, QueryResult, SchemaInfo};

#[async_trait]
pub trait DbBackend: Send + Sync {
    async fn connect(config: &ConnectionConfig) -> Result<Self, DbError>
    where
        Self: Sized;

    async fn disconnect(&self) -> Result<(), DbError>;

    async fn execute_query(&self, sql: &str) -> Result<QueryResult, DbError>;

    async fn introspect(&self) -> Result<SchemaInfo, DbError>;

    fn backend_name(&self) -> &'static str;
}
