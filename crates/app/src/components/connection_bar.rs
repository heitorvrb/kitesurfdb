use std::sync::Arc;

use app_core::config::{self, Theme};
use app_core::connection_manager::ConnectionManager;
use app_core::tab_manager::TabManager;
use db::traits::DbBackend;
use db::types::SchemaInfo;
use dioxus::prelude::*;

use super::connection_dialog::ConnectionDialog;

#[css_module("/assets/styles/connection_bar.css")]
struct Styles;

#[component]
pub fn ConnectionBar(
    backend: Signal<Option<Arc<dyn DbBackend>>>,
    is_connected: Signal<bool>,
    tab_manager: Signal<TabManager>,
    schema_info: Signal<Option<SchemaInfo>>,
    connection_manager: Signal<ConnectionManager>,
    theme: Signal<Theme>,
    sidebar_visible: Signal<bool>,
    show_search_modal: Signal<bool>,
) -> Element {
    let connection_error: Signal<Option<String>> = use_signal(|| None);
    let mut is_connected = is_connected;
    let mut schema_info = schema_info;
    let mut tab_manager = tab_manager;
    let mut theme = theme;
    let mut sidebar_visible = sidebar_visible;
    let mut show_search_modal = show_search_modal;
    let mut show_dialog = use_signal(|| false);

    let disconnect = move |_| {
        let mut backend = backend;
        spawn(async move {
            // Cancel/close tabs first so in-flight operations stop before backend teardown.
            let ids: Vec<_> = tab_manager.read().tabs().iter().map(|t| t.id).collect();
            for id in ids {
                tab_manager.write().close_tab(id);
            }

            // Clear UI connection state immediately.
            backend.set(None);
            is_connected.set(false);
            schema_info.set(None);

            // Take and disconnect backend last.
            let prev = connection_manager.write().take_active();
            if let Some(b) = prev {
                let _ = b.disconnect().await;
            }
        });
    };

    let active_name = {
        let cm = connection_manager.read();
        cm.active_connection_id()
            .and_then(|id| cm.connection_by_id(id).map(|c| c.name.clone()))
    };

    let is_dark = *theme.read() == Theme::Dark;
    let sidebar_shown = *sidebar_visible.read();

    rsx! {
        header { class: Styles::toolbar,
            button {
                class: Styles::sidebar_toggle_btn,
                title: if sidebar_shown { "Hide sidebar" } else { "Show sidebar" },
                onclick: move |_| {
                    let new_val = !*sidebar_visible.read();
                    sidebar_visible.set(new_val);
                    let mut prefs = config::load_preferences();
                    prefs.sidebar_visible = new_val;
                    let _ = config::save_preferences(&prefs);
                },
                if sidebar_shown { "‹" } else { "›" }
            }
            h1 { "Kitesurf" }
            button {
                class: Styles::search_btn,
                disabled: !*is_connected.read(),
                onclick: move |_| show_search_modal.set(true),
                "Search"
            }
            button {
                class: Styles::theme_btn,
                onclick: move |_| {
                    let new_theme = theme.read().toggle();
                    theme.set(new_theme);
                    let mut prefs = config::load_preferences();
                    prefs.theme = new_theme;
                    let _ = config::save_preferences(&prefs);
                },
                if is_dark { "☀" } else { "☽" }
            }
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
                schema_info,
                connection_manager,
                connection_error,
            }
        }
    }
}
