use app_core::tab_manager::TabManager;
use dioxus::prelude::*;

#[css_module("/assets/styles/tab_bar.css")]
struct Styles;

#[component]
pub fn TabBar(tab_manager: Signal<TabManager>) -> Element {
    let tabs_info: Vec<(uuid::Uuid, String, bool)> = {
        let tm = tab_manager.read();
        let active_id = tm.active_tab_id();
        tm.tabs()
            .iter()
            .map(|t| (t.id, t.title.clone(), Some(t.id) == active_id))
            .collect()
    };

    rsx! {
        div { class: Styles::tab_bar,
            for (id, title, is_active) in &tabs_info {
                {
                    let id = *id;
                    rsx! {
                        div {
                            class: if *is_active { format!("{} {}", Styles::tab, Styles::active) } else { Styles::tab.to_string() },
                            onclick: move |_| { tab_manager.write().set_active(id); },
                            span { "{title}" }
                            button {
                                class: Styles::close_btn,
                                onclick: move |evt| {
                                    evt.stop_propagation();
                                    tab_manager.write().close_tab(id);
                                },
                                "x"
                            }
                        }
                    }
                }
            }
            button {
                class: Styles::new_tab_btn,
                onclick: move |_| { tab_manager.write().open_sql_editor(); },
                "+"
            }
        }
    }
}
