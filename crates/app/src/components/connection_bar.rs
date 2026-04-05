use std::sync::Arc;

use app_core::tab_manager::TabManager;
use db::sqlite::SqliteBackend;
use db::traits::DbBackend;
use db::types::{ConnectionConfig, SchemaInfo};
use dioxus::prelude::*;

#[css_module("/assets/styles/connection_bar.css")]
struct Styles;

#[component]
pub fn ConnectionBar(
    backend: Signal<Option<Arc<SqliteBackend>>>,
    is_connected: Signal<bool>,
    tab_manager: Signal<TabManager>,
    schema_info: Signal<Option<SchemaInfo>>,
) -> Element {
    let mut db_path = use_signal(|| String::from(":memory:"));
    let mut connection_error: Signal<Option<String>> = use_signal(|| None);
    let mut is_connected = is_connected;
    let mut schema_info = schema_info;
    let mut tab_manager = tab_manager;

    let connect = move |_| {
        let path = db_path.read().clone();
        let mut backend = backend;
        spawn(async move {
            connection_error.set(None);
            let config = ConnectionConfig::new_sqlite("session", &path);
            match SqliteBackend::connect(&config).await {
                Ok(b) => {
                    let b = Arc::new(b);
                    match b.introspect().await {
                        Ok(info) => schema_info.set(Some(info)),
                        Err(_) => schema_info.set(None),
                    }
                    backend.set(Some(b));
                    is_connected.set(true);
                    tab_manager.write().open_sql_editor();
                }
                Err(e) => {
                    connection_error.set(Some(e.to_string()));
                    is_connected.set(false);
                }
            }
        });
    };

    let disconnect = move |_| {
        let mut backend = backend;
        spawn(async move {
            if let Some(b) = backend.read().as_ref() {
                let _ = b.disconnect().await;
            }
            backend.set(None);
            is_connected.set(false);
            schema_info.set(None);
            // Close all tabs
            let ids: Vec<_> = tab_manager.read().tabs().iter().map(|t| t.id).collect();
            for id in ids {
                tab_manager.write().close_tab(id);
            }
        });
    };

    rsx! {
        header { class: Styles::toolbar,
            h1 { "Kitesurf" }
            div { class: Styles::connection_bar,
                input {
                    class: Styles::db_path_input,
                    value: "{db_path}",
                    placeholder: "SQLite path (e.g. :memory: or /path/to/db.sqlite)",
                    disabled: *is_connected.read(),
                    oninput: move |evt| db_path.set(evt.value()),
                }
                if *is_connected.read() {
                    button {
                        class: Styles::disconnect_btn,
                        onclick: disconnect,
                        "Disconnect"
                    }
                } else {
                    button {
                        class: Styles::connect_btn,
                        onclick: connect,
                        "Connect"
                    }
                }
                if *is_connected.read() {
                    span { class: "{Styles::status} {Styles::connected}", "Connected" }
                } else {
                    span { class: "{Styles::status} {Styles::disconnected}", "Disconnected" }
                }
            }
        }
        if let Some(err) = connection_error.read().as_ref() {
            div { class: "error", "{err}" }
        }
    }
}
