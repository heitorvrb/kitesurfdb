use dioxus::prelude::*;

#[css_module("/assets/styles/sql_display.css")]
struct Styles;

#[component]
pub fn SqlDisplay(
    sql: String,
    #[props(default)] action_label: Option<String>,
    #[props(default)] on_action: Option<EventHandler>,
) -> Element {
    rsx! {
        div { class: Styles::sql_display,
            code { "{sql}" }
            if let (Some(label), Some(handler)) = (action_label, on_action) {
                button {
                    class: Styles::sql_action_btn,
                    onclick: move |_| handler.call(()),
                    "{label}"
                }
            }
        }
    }
}
