use db::types::ConnectionConfig;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const APP_DIR: &str = "kitesurfdb";
const CONNECTIONS_FILE: &str = "connections.json";
const PREFERENCES_FILE: &str = "preferences.json";

pub fn default_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(APP_DIR))
}

fn connections_path(config_dir: &PathBuf) -> PathBuf {
    config_dir.join(CONNECTIONS_FILE)
}

pub fn load_connections_from(config_dir: &PathBuf) -> Vec<ConnectionConfig> {
    let path = connections_path(config_dir);

    let Ok(data) = fs::read_to_string(&path) else {
        return Vec::new();
    };

    serde_json::from_str(&data).unwrap_or_default()
}

pub fn save_connections_to(
    config_dir: &PathBuf,
    connections: &[ConnectionConfig],
) -> Result<(), String> {
    fs::create_dir_all(config_dir).map_err(|e| format!("Failed to create config dir: {e}"))?;

    let path = connections_path(config_dir);
    let json =
        serde_json::to_string_pretty(connections).map_err(|e| format!("Serialize error: {e}"))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write config: {e}"))?;

    Ok(())
}

pub fn load_connections() -> Vec<ConnectionConfig> {
    let Some(dir) = default_config_dir() else {
        return Vec::new();
    };
    load_connections_from(&dir)
}

pub fn save_connections(connections: &[ConnectionConfig]) -> Result<(), String> {
    let dir = default_config_dir().ok_or("Could not determine config directory")?;
    save_connections_to(&dir, connections)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    Dark,
    Light,
}

impl Theme {
    pub fn toggle(self) -> Self {
        match self {
            Theme::Dark => Theme::Light,
            Theme::Light => Theme::Dark,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Theme::Dark => "dark",
            Theme::Light => "light",
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::Dark
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default)]
    pub theme: Theme,
    #[serde(default = "default_true")]
    pub sidebar_visible: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            sidebar_visible: true,
        }
    }
}

fn preferences_path(config_dir: &PathBuf) -> PathBuf {
    config_dir.join(PREFERENCES_FILE)
}

pub fn load_preferences() -> Preferences {
    let Some(dir) = default_config_dir() else {
        return Preferences::default();
    };
    let path = preferences_path(&dir);
    let Ok(data) = fs::read_to_string(&path) else {
        return Preferences::default();
    };
    serde_json::from_str(&data).unwrap_or_default()
}

pub fn save_preferences(prefs: &Preferences) -> Result<(), String> {
    let dir = default_config_dir().ok_or("Could not determine config directory")?;
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create config dir: {e}"))?;
    let path = preferences_path(&dir);
    let json = serde_json::to_string_pretty(prefs).map_err(|e| format!("Serialize error: {e}"))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write preferences: {e}"))?;
    Ok(())
}

/// Store a password in the OS keyring for a given connection ID.
pub fn store_password(connection_id: &str, password: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(APP_DIR, connection_id).map_err(|e| e.to_string())?;
    entry.set_password(password).map_err(|e| e.to_string())
}

/// Retrieve a password from the OS keyring for a given connection ID.
pub fn get_password(connection_id: &str) -> Option<String> {
    let entry = keyring::Entry::new(APP_DIR, connection_id).ok()?;
    entry.get_password().ok()
}

/// Delete a password from the OS keyring for a given connection ID.
pub fn delete_password(connection_id: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(APP_DIR, connection_id).map_err(|e| e.to_string())?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use db::types::ConnectionConfig;

    #[test]
    fn test_theme_toggle() {
        assert_eq!(Theme::Dark.toggle(), Theme::Light);
        assert_eq!(Theme::Light.toggle(), Theme::Dark);
    }

    #[test]
    fn test_theme_as_str() {
        assert_eq!(Theme::Dark.as_str(), "dark");
        assert_eq!(Theme::Light.as_str(), "light");
    }

    #[test]
    fn test_theme_default() {
        assert_eq!(Theme::default(), Theme::Dark);
    }

    #[test]
    fn test_preferences_serialization() {
        let prefs = Preferences {
            theme: Theme::Light,
            sidebar_visible: false,
        };
        let json = serde_json::to_string(&prefs).unwrap();
        let loaded: Preferences = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.theme, Theme::Light);
        assert!(!loaded.sidebar_visible);
    }

    #[test]
    fn test_preferences_sidebar_visible_defaults_true() {
        // Old preferences.json without sidebar_visible should default to true
        let json = r#"{"theme":"Dark"}"#;
        let loaded: Preferences = serde_json::from_str(json).unwrap();
        assert!(loaded.sidebar_visible);
    }

    #[test]
    fn test_save_and_load_connections() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(APP_DIR);

        let configs = vec![
            ConnectionConfig::new_sqlite("local", "/tmp/test.db"),
            ConnectionConfig::new_postgres("prod", "db.example.com", 5432, "mydb", "admin"),
        ];

        save_connections_to(&config_dir, &configs).unwrap();
        let loaded = load_connections_from(&config_dir);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "local");
        assert_eq!(loaded[1].name, "prod");
        assert!(loaded[0].password.is_none());
        assert!(loaded[1].password.is_none());
    }

    #[test]
    fn test_load_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(APP_DIR);

        let loaded = load_connections_from(&config_dir);
        assert!(loaded.is_empty());
    }

    #[test]
    fn test_save_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(APP_DIR);

        let configs1 = vec![ConnectionConfig::new_sqlite("one", "/tmp/1.db")];
        save_connections_to(&config_dir, &configs1).unwrap();

        let configs2 = vec![
            ConnectionConfig::new_sqlite("two", "/tmp/2.db"),
            ConnectionConfig::new_sqlite("three", "/tmp/3.db"),
        ];
        save_connections_to(&config_dir, &configs2).unwrap();

        let loaded = load_connections_from(&config_dir);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "two");
    }
}
