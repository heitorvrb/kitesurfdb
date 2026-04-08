use std::sync::Arc;

use db::error::DbError;
use db::postgres::PostgresBackend;
use db::sqlite::SqliteBackend;
use db::traits::DbBackend;
use db::types::{BackendType, ConnectionConfig};
use uuid::Uuid;

use crate::config;

pub struct ConnectionManager {
    connections: Vec<ConnectionConfig>,
    active_connection_id: Option<Uuid>,
    active_backend: Option<Arc<dyn DbBackend>>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: config::load_connections(),
            active_connection_id: None,
            active_backend: None,
        }
    }

    pub fn connections(&self) -> &[ConnectionConfig] {
        &self.connections
    }

    pub fn active_connection_id(&self) -> Option<Uuid> {
        self.active_connection_id
    }

    pub fn active_backend(&self) -> Option<&Arc<dyn DbBackend>> {
        self.active_backend.as_ref()
    }

    pub fn add_connection(&mut self, config: ConnectionConfig) {
        // Store password in keyring if present
        if let Some(ref password) = config.password {
            let _ = config::store_password(&config.id.to_string(), password);
        }
        self.connections.push(config);
        let _ = config::save_connections(&self.connections);
    }

    pub fn update_connection(&mut self, config: ConnectionConfig) {
        if let Some(ref password) = config.password {
            let _ = config::store_password(&config.id.to_string(), password);
        }
        if let Some(existing) = self.connections.iter_mut().find(|c| c.id == config.id) {
            *existing = config;
        }
        let _ = config::save_connections(&self.connections);
    }

    pub fn remove_connection(&mut self, id: Uuid) {
        let _ = config::delete_password(&id.to_string());
        self.connections.retain(|c| c.id != id);
        let _ = config::save_connections(&self.connections);
    }

    pub fn connection_by_id(&self, id: Uuid) -> Option<&ConnectionConfig> {
        self.connections.iter().find(|c| c.id == id)
    }

    pub async fn connect(&mut self, id: Uuid) -> Result<Arc<dyn DbBackend>, DbError> {
        // Disconnect existing connection if any
        if self.active_backend.is_some() {
            self.disconnect().await?;
        }

        let config = self
            .connections
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| DbError::ConnectionFailed("Connection not found".into()))?;

        // Load password from keyring
        let mut config = config.clone();
        if config.password.is_none() {
            config.password = config::get_password(&config.id.to_string());
        }

        let backend: Arc<dyn DbBackend> = match config.backend {
            BackendType::Sqlite => Arc::new(SqliteBackend::connect(&config).await?),
            BackendType::Postgres => Arc::new(PostgresBackend::connect(&config).await?),
        };

        self.active_connection_id = Some(id);
        self.active_backend = Some(backend.clone());

        Ok(backend)
    }

    pub async fn disconnect(&mut self) -> Result<(), DbError> {
        if let Some(backend) = self.active_backend.take() {
            backend.disconnect().await?;
        }
        self.active_connection_id = None;
        Ok(())
    }

    pub fn is_connected(&self) -> bool {
        self.active_backend.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use db::types::ConnectionConfig;

    fn make_sqlite_config(name: &str) -> ConnectionConfig {
        ConnectionConfig::new_sqlite(name, ":memory:")
    }

    fn make_pg_config(name: &str) -> ConnectionConfig {
        ConnectionConfig::new_postgres(name, "localhost", 5432, "testdb", "postgres")
    }

    #[test]
    fn test_add_and_list_connections() {
        let mut cm = ConnectionManager {
            connections: Vec::new(),
            active_connection_id: None,
            active_backend: None,
        };

        let c1 = make_sqlite_config("local");
        let c2 = make_pg_config("prod");
        let id1 = c1.id;
        let id2 = c2.id;

        cm.connections.push(c1);
        cm.connections.push(c2);

        assert_eq!(cm.connections().len(), 2);
        assert!(cm.connection_by_id(id1).is_some());
        assert!(cm.connection_by_id(id2).is_some());
    }

    #[test]
    fn test_remove_connection() {
        let mut cm = ConnectionManager {
            connections: Vec::new(),
            active_connection_id: None,
            active_backend: None,
        };

        let c = make_sqlite_config("local");
        let id = c.id;
        cm.connections.push(c);
        cm.connections.retain(|c| c.id != id);
        assert!(cm.connections().is_empty());
    }

    #[test]
    fn test_is_connected_default_false() {
        let cm = ConnectionManager {
            connections: Vec::new(),
            active_connection_id: None,
            active_backend: None,
        };
        assert!(!cm.is_connected());
    }

    #[tokio::test]
    async fn test_connect_sqlite_memory() {
        let mut cm = ConnectionManager {
            connections: Vec::new(),
            active_connection_id: None,
            active_backend: None,
        };

        let config = make_sqlite_config("test");
        let id = config.id;
        cm.connections.push(config);

        let result = cm.connect(id).await;
        assert!(result.is_ok());
        assert!(cm.is_connected());
        assert_eq!(cm.active_connection_id(), Some(id));
        assert_eq!(cm.active_backend().unwrap().backend_name(), "SQLite");
    }

    #[tokio::test]
    async fn test_connect_nonexistent_id() {
        let mut cm = ConnectionManager {
            connections: Vec::new(),
            active_connection_id: None,
            active_backend: None,
        };

        let result = cm.connect(Uuid::new_v4()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_disconnect() {
        let mut cm = ConnectionManager {
            connections: Vec::new(),
            active_connection_id: None,
            active_backend: None,
        };

        let config = make_sqlite_config("test");
        let id = config.id;
        cm.connections.push(config);

        cm.connect(id).await.unwrap();
        assert!(cm.is_connected());

        cm.disconnect().await.unwrap();
        assert!(!cm.is_connected());
        assert!(cm.active_connection_id().is_none());
    }

    #[tokio::test]
    async fn test_connect_disconnects_previous() {
        let mut cm = ConnectionManager {
            connections: Vec::new(),
            active_connection_id: None,
            active_backend: None,
        };

        let c1 = make_sqlite_config("first");
        let c2 = make_sqlite_config("second");
        let id1 = c1.id;
        let id2 = c2.id;
        cm.connections.push(c1);
        cm.connections.push(c2);

        cm.connect(id1).await.unwrap();
        assert_eq!(cm.active_connection_id(), Some(id1));

        cm.connect(id2).await.unwrap();
        assert_eq!(cm.active_connection_id(), Some(id2));
    }
}
