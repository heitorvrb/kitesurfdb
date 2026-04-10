mod app;
mod components;
mod highlight;
mod keyboard_shortcuts;

use dioxus::desktop::{Config, WindowBuilder};
use dioxus::desktop::tao::window::Theme;
use dioxus::prelude::*;

fn main() {
    let theme = match dark_light::detect() {
        Ok(dark_light::Mode::Dark) => Theme::Dark,
        _ => Theme::Light,
    };

    LaunchBuilder::desktop()
        .with_cfg(
            Config::new().with_window(
                WindowBuilder::new()
                    .with_title("Kitesurf")
                    .with_theme(Some(theme)),
            ),
        )
        .launch(app::App);
}
