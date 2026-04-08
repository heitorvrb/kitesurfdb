use std::sync::Arc;

use app_core::connection_manager::ConnectionManager;
use app_core::tab_manager::TabManager;
use db::traits::DbBackend;
use db::types::{BackendType, ConnectionConfig, SchemaInfo};
use dioxus::prelude::*;
use uuid::Uuid;

#[css_module("/assets/styles/connection_bar.css")]
struct Styles;

#[component]
pub fn ConnectionBar(
    backend: Signal<Option<Arc<dyn DbBackend>>>,
    is_connected: Signal<bool>,
    tab_manager: Signal<TabManager>,
    schema_info: Signal<Option<SchemaInfo>>,
    connection_manager: Signal<ConnectionManager>,
) -> Element {
    let connection_error: Signal<Option<String>> = use_signal(|| None);
    let mut is_connected = is_connected;
    let mut schema_info = schema_info;
    let mut tab_manager = tab_manager;
    let mut show_dialog = use_signal(|| false);

    let disconnect = move |_| {
        let mut backend = backend;
        spawn(async move {
            // Take active backend (quick write, released before await)
            let prev = connection_manager.write().take_active();
            if let Some(b) = prev {
                let _ = b.disconnect().await;
            }
            backend.set(None);
            is_connected.set(false);
            schema_info.set(None);
            let ids: Vec<_> = tab_manager.read().tabs().iter().map(|t| t.id).collect();
            for id in ids {
                tab_manager.write().close_tab(id);
            }
        });
    };

    let active_name = {
        let cm = connection_manager.read();
        cm.active_connection_id().and_then(|id| {
            cm.connection_by_id(id).map(|c| c.name.clone())
        })
    };

    rsx! {
        header { class: Styles::toolbar,
            h1 { "Kitesurf" }
            div { class: Styles::connection_bar,
                if *is_connected.read() {
                    span { class: "{Styles::status} {Styles::connected}",
                        "Connected to {active_name.as_deref().unwrap_or(\"database\")}"
                    }
                    button {
                        class: Styles::disconnect_btn,
                        onclick: disconnect,
                        "Disconnect"
                    }
                } else {
                    span { class: "{Styles::status} {Styles::disconnected}", "Disconnected" }
                    button {
                        class: Styles::connect_btn,
                        onclick: move |_| show_dialog.set(true),
                        "Connect"
                    }
                }
            }
        }
        if let Some(err) = connection_error.read().as_ref() {
            div { class: "error", "{err}" }
        }
        if *show_dialog.read() {
            ConnectionDialog {
                show_dialog,
                backend,
                is_connected,
                tab_manager,
                schema_info,
                connection_manager,
                connection_error,
            }
        }
    }
}

#[component]
fn ConnectionDialog(
    show_dialog: Signal<bool>,
    backend: Signal<Option<Arc<dyn DbBackend>>>,
    is_connected: Signal<bool>,
    tab_manager: Signal<TabManager>,
    schema_info: Signal<Option<SchemaInfo>>,
    connection_manager: Signal<ConnectionManager>,
    connection_error: Signal<Option<String>>,
) -> Element {
    let mut backend_type = use_signal(|| BackendType::Sqlite);
    let mut conn_name = use_signal(|| String::from("New Connection"));
    let mut host = use_signal(|| String::from("localhost"));
    let mut port = use_signal(|| String::from("5432"));
    let mut database = use_signal(|| String::new());
    let mut username = use_signal(|| String::from("postgres"));
    let mut password = use_signal(|| String::new());
    let mut file_path = use_signal(|| String::from(":memory:"));
    let mut editing_id: Signal<Option<Uuid>> = use_signal(|| None);

    let mut is_connected = is_connected;
    let mut schema_info = schema_info;
    let mut tab_manager = tab_manager;
    let mut connection_error = connection_error;
    let mut show_dialog = show_dialog;

    let save_and_connect = move |_| {
        let mut connection_manager = connection_manager;
        let mut backend = backend;
        let name = conn_name.read().clone();
        let bt = backend_type.read().clone();
        let host_val = host.read().clone();
        let port_val: u16 = port.read().parse().unwrap_or(5432);
        let db_val = database.read().clone();
        let user_val = username.read().clone();
        let pass_val = password.read().clone();
        let path_val = file_path.read().clone();
        let edit_id = *editing_id.read();

        spawn(async move {
            connection_error.set(None);

            let mut config = match bt {
                BackendType::Sqlite => ConnectionConfig::new_sqlite(&name, &path_val),
                BackendType::Postgres => {
                    ConnectionConfig::new_postgres(&name, &host_val, port_val, &db_val, &user_val)
                }
            };

            if !pass_val.is_empty() {
                config.password = Some(pass_val);
            }

            // Save connection and capture its ID before moving config
            let id = if let Some(id) = edit_id {
                config.id = id;
                connection_manager.write().update_connection(config);
                id
            } else {
                let id = config.id;
                connection_manager.write().add_connection(config);
                id
            };

            // Disconnect previous (quick write, released before await)
            let prev = connection_manager.write().take_active();
            if let Some(b) = prev {
                let _ = b.disconnect().await;
            }

            // Get config with password (quick read, released before await)
            let connect_config = match connection_manager.read().get_connect_config(id) {
                Ok(c) => c,
                Err(e) => {
                    connection_error.set(Some(e.to_string()));
                    return;
                }
            };

            // Async connect — no signal borrow held
            match ConnectionManager::create_backend(&connect_config).await {
                Ok(b) => {
                    connection_manager.write().set_connected(id, b.clone());
                    match b.introspect().await {
                        Ok(info) => schema_info.set(Some(info)),
                        Err(_) => schema_info.set(None),
                    }
                    backend.set(Some(b));
                    is_connected.set(true);
                    tab_manager.write().open_sql_editor();
                    show_dialog.set(false);
                }
                Err(e) => {
                    connection_error.set(Some(e.to_string()));
                }
            }
        });
    };

    let mut load_connection = move |id: Uuid| {
        let cm = connection_manager.read();
        if let Some(config) = cm.connection_by_id(id) {
            conn_name.set(config.name.clone());
            backend_type.set(config.backend.clone());
            host.set(config.host.clone().unwrap_or_default());
            port.set(config.port.map(|p| p.to_string()).unwrap_or("5432".into()));
            database.set(config.database.clone());
            username.set(config.username.clone().unwrap_or_default());
            file_path.set(
                config
                    .file_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            );
            password.set(String::new()); // Don't show stored passwords
            editing_id.set(Some(id));
        }
    };

    let quick_connect = move |id: Uuid| {
        let mut connection_manager = connection_manager;
        let mut backend = backend;
        spawn(async move {
            connection_error.set(None);

            // Disconnect previous (quick write, released before await)
            let prev = connection_manager.write().take_active();
            if let Some(b) = prev {
                let _ = b.disconnect().await;
            }

            // Get config with password (quick read, released before await)
            let connect_config = match connection_manager.read().get_connect_config(id) {
                Ok(c) => c,
                Err(e) => {
                    connection_error.set(Some(e.to_string()));
                    return;
                }
            };

            // Async connect — no signal borrow held
            match ConnectionManager::create_backend(&connect_config).await {
                Ok(b) => {
                    connection_manager.write().set_connected(id, b.clone());
                    match b.introspect().await {
                        Ok(info) => schema_info.set(Some(info)),
                        Err(_) => schema_info.set(None),
                    }
                    backend.set(Some(b));
                    is_connected.set(true);
                    tab_manager.write().open_sql_editor();
                    show_dialog.set(false);
                }
                Err(e) => {
                    connection_error.set(Some(e.to_string()));
                }
            }
        });
    };

    let saved_connections: Vec<(Uuid, String, BackendType)> = connection_manager
        .read()
        .connections()
        .iter()
        .map(|c| (c.id, c.name.clone(), c.backend.clone()))
        .collect();

    rsx! {
        div { class: Styles::dialog_overlay,
            onclick: move |_| show_dialog.set(false),
            div { class: Styles::dialog,
                onclick: move |e| e.stop_propagation(),
                h2 { "Connect to Database" }

                // Saved connections list
                if !saved_connections.is_empty() {
                    div { class: Styles::saved_connections,
                        h3 { "Saved Connections" }
                        for (id, name, bt) in &saved_connections {
                            {
                                let id = *id;
                                let bt_label = match bt {
                                    BackendType::Sqlite => "SQLite",
                                    BackendType::Postgres => "PostgreSQL",
                                };
                                rsx! {
                                    div { class: Styles::saved_item,
                                        span {
                                            onclick: move |_| load_connection(id),
                                            "{name} ({bt_label})"
                                        }
                                        button {
                                            class: Styles::quick_connect_btn,
                                            onclick: move |_| quick_connect(id),
                                            "Connect"
                                        }
                                        button {
                                            class: Styles::delete_btn,
                                            onclick: move |_| {
                                                connection_manager.write().remove_connection(id);
                                            },
                                            "x"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // New/edit connection form
                div { class: Styles::connection_form,
                    div { class: Styles::field,
                        label { "Name" }
                        input {
                            value: "{conn_name}",
                            oninput: move |e| conn_name.set(e.value()),
                        }
                    }
                    div { class: Styles::field,
                        label { "Backend" }
                        select {
                            value: if *backend_type.read() == BackendType::Sqlite { "sqlite" } else { "postgres" },
                            onchange: move |e| {
                                let val = e.value();
                                if val == "sqlite" {
                                    backend_type.set(BackendType::Sqlite);
                                } else {
                                    backend_type.set(BackendType::Postgres);
                                }
                            },
                            option { value: "sqlite", "SQLite" }
                            option { value: "postgres", "PostgreSQL" }
                        }
                    }

                    if *backend_type.read() == BackendType::Sqlite {
                        div { class: Styles::field,
                            label { "File Path" }
                            input {
                                value: "{file_path}",
                                placeholder: ":memory: or /path/to/db.sqlite",
                                oninput: move |e| file_path.set(e.value()),
                            }
                        }
                    } else {
                        div { class: Styles::field,
                            label { "Host" }
                            input {
                                value: "{host}",
                                oninput: move |e| host.set(e.value()),
                            }
                        }
                        div { class: Styles::field,
                            label { "Port" }
                            input {
                                value: "{port}",
                                oninput: move |e| port.set(e.value()),
                            }
                        }
                        div { class: Styles::field,
                            label { "Database" }
                            input {
                                value: "{database}",
                                oninput: move |e| database.set(e.value()),
                            }
                        }
                        div { class: Styles::field,
                            label { "Username" }
                            input {
                                value: "{username}",
                                oninput: move |e| username.set(e.value()),
                            }
                        }
                        div { class: Styles::field,
                            label { "Password" }
                            input {
                                r#type: "password",
                                value: "{password}",
                                placeholder: "(stored in OS keyring)",
                                oninput: move |e| password.set(e.value()),
                            }
                        }
                    }

                    div { class: Styles::dialog_actions,
                        button {
                            class: Styles::connect_btn,
                            onclick: save_and_connect,
                            if editing_id.read().is_some() { "Save & Connect" } else { "Add & Connect" }
                        }
                        button {
                            class: Styles::cancel_btn,
                            onclick: move |_| show_dialog.set(false),
                            "Cancel"
                        }
                    }
                }
            }
        }
    }
}
