use dioxus::prelude::*;

#[css_module("/assets/styles/sql_display.css")]
struct Styles;

#[component]
pub fn SqlDisplay(sql: String) -> Element {
    rsx! {
        div { class: Styles::sql_display,
            code { "{sql}" }
        }
    }
}
