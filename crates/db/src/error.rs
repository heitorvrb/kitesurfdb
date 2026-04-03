use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Query failed: {0}")]
    QueryFailed(String),

    #[error("Introspection failed: {0}")]
    IntrospectionFailed(String),

    #[error("Unsupported backend: {0}")]
    UnsupportedBackend(String),

    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
}
