use std::sync::Arc;
use std::time::{Duration, Instant};

use app_core::connection_manager::ConnectionManager;
use db::traits::DbBackend;
use db::types::{BackendType, ConnectionConfig, SchemaInfo};
use dioxus::prelude::*;
use uuid::Uuid;

#[css_module("/assets/styles/connection_dialog.css")]
struct Styles;

#[component]
pub fn ConnectionDialog(
    show_dialog: Signal<bool>,
    backend: Signal<Option<Arc<dyn DbBackend>>>,
    is_connected: Signal<bool>,
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
    let mut default_schema = use_signal(|| String::from("public"));
    let mut editing_id: Signal<Option<Uuid>> = use_signal(|| None);
    let mut is_connecting = use_signal(|| false);
    let mut connect_started_at: Signal<Option<Instant>> = use_signal(|| None);
    let mut connect_elapsed_ms: Signal<u128> = use_signal(|| 0);
    let mut connect_target = use_signal(String::new);

    let mut is_connected = is_connected;
    let mut schema_info = schema_info;
    let mut connection_error = connection_error;
    let mut show_dialog = show_dialog;

    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_millis(150)).await;
            if *is_connecting.read() {
                let started = *connect_started_at.read();
                if let Some(started) = started {
                    connect_elapsed_ms.set(started.elapsed().as_millis());
                }
            }
        }
    });

    let save_and_connect = move |_| {
        if *is_connecting.read() {
            return;
        }
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
        let default_schema_val = default_schema.read().clone();

        spawn(async move {
            connection_error.set(None);
            is_connecting.set(true);
            connect_started_at.set(Some(Instant::now()));
            connect_elapsed_ms.set(0);
            connect_target.set(name.clone());

            let mut config = match bt {
                BackendType::Sqlite => ConnectionConfig::new_sqlite(&name, &path_val),
                BackendType::Postgres => {
                    let mut c = ConnectionConfig::new_postgres(
                        &name, &host_val, port_val, &db_val, &user_val,
                    );
                    c.default_schema = if default_schema_val.trim().is_empty() {
                        None
                    } else {
                        Some(default_schema_val.trim().to_string())
                    };
                    c
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
                    is_connecting.set(false);
                    connect_started_at.set(None);
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
                    show_dialog.set(false);
                    is_connecting.set(false);
                    connect_started_at.set(None);
                }
                Err(e) => {
                    connection_error.set(Some(e.to_string()));
                    is_connecting.set(false);
                    connect_started_at.set(None);
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
            default_schema.set(config.default_schema.clone().unwrap_or_default());
            editing_id.set(Some(id));
        }
    };

    let quick_connect = move |id: Uuid| {
        if *is_connecting.read() {
            return;
        }
        let mut connection_manager = connection_manager;
        let mut backend = backend;
        let target_name = {
            let cm = connection_manager.read();
            cm.connection_by_id(id)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| "database".into())
        };
        spawn(async move {
            connection_error.set(None);
            is_connecting.set(true);
            connect_started_at.set(Some(Instant::now()));
            connect_elapsed_ms.set(0);
            connect_target.set(target_name);

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
                    is_connecting.set(false);
                    connect_started_at.set(None);
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
                    show_dialog.set(false);
                    is_connecting.set(false);
                    connect_started_at.set(None);
                }
                Err(e) => {
                    connection_error.set(Some(e.to_string()));
                    is_connecting.set(false);
                    connect_started_at.set(None);
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

    let connecting = *is_connecting.read();
    let elapsed_ms = *connect_elapsed_ms.read();
    let elapsed_secs = elapsed_ms as f64 / 1000.0;
    let taking_longer = connecting && elapsed_ms >= 3000;
    let connect_label = if connect_target.read().is_empty() {
        "database".to_string()
    } else {
        connect_target.read().clone()
    };

    rsx! {
        div { class: Styles::dialog_overlay,
            onclick: move |_| {
                if !*is_connecting.read() {
                    show_dialog.set(false);
                }
            },
            div { class: Styles::dialog,
                onclick: move |e| e.stop_propagation(),
                h2 { "Connect to Database" }

                if connecting {
                    div { class: Styles::connecting_status,
                        "Connecting to {connect_label}... {elapsed_secs:.1}s"
                    }
                    if taking_longer {
                        div { class: Styles::connecting_warning,
                            "This is taking longer than usual. Check host/port, VPN or firewall, and that PostgreSQL is running."
                        }
                    }
                }

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
                                            disabled: connecting,
                                            onclick: move |_| quick_connect(id),
                                            "Connect"
                                        }
                                        button {
                                            class: Styles::delete_btn,
                                            disabled: connecting,
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
                            disabled: connecting,
                            value: "{conn_name}",
                            oninput: move |e| conn_name.set(e.value()),
                        }
                    }
                    div { class: Styles::field,
                        label { "Backend" }
                        div { class: Styles::select_wrapper,
                            select {
                                disabled: connecting,
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
                    }

                    if *backend_type.read() == BackendType::Sqlite {
                        div { class: Styles::field,
                            label { "File Path" }
                            input {
                                disabled: connecting,
                                value: "{file_path}",
                                placeholder: ":memory: or /path/to/db.sqlite",
                                oninput: move |e| file_path.set(e.value()),
                            }
                        }
                    } else {
                        div { class: Styles::field,
                            label { "Host" }
                            input {
                                disabled: connecting,
                                value: "{host}",
                                oninput: move |e| host.set(e.value()),
                            }
                        }
                        div { class: Styles::field,
                            label { "Port" }
                            input {
                                disabled: connecting,
                                value: "{port}",
                                oninput: move |e| port.set(e.value()),
                            }
                        }
                        div { class: Styles::field,
                            label { "Database" }
                            input {
                                disabled: connecting,
                                value: "{database}",
                                oninput: move |e| database.set(e.value()),
                            }
                        }
                        div { class: Styles::field,
                            label { "Username" }
                            input {
                                disabled: connecting,
                                value: "{username}",
                                oninput: move |e| username.set(e.value()),
                            }
                        }
                        div { class: Styles::field,
                            label { "Password" }
                            input {
                                disabled: connecting,
                                r#type: "password",
                                value: "{password}",
                                placeholder: "(stored in OS keyring)",
                                oninput: move |e| password.set(e.value()),
                            }
                        }
                        div { class: Styles::field,
                            label { "Default Schema" }
                            input {
                                disabled: connecting,
                                value: "{default_schema}",
                                placeholder: "public",
                                oninput: move |e| default_schema.set(e.value()),
                            }
                        }
                    }

                    if let Some(err) = connection_error.read().as_ref() {
                        div { class: "error", "{err}" }
                    }

                    div { class: Styles::dialog_actions,
                        button {
                            class: Styles::connect_btn,
                            disabled: connecting,
                            onclick: save_and_connect,
                            if connecting {
                                "Connecting..."
                            } else if editing_id.read().is_some() {
                                "Save & Connect"
                            } else {
                                "Add & Connect"
                            }
                        }
                        button {
                            class: Styles::cancel_btn,
                            disabled: connecting,
                            onclick: move |_| show_dialog.set(false),
                            "Cancel"
                        }
                    }
                }
            }
        }
    }
}
