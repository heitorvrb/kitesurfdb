mod app;
mod components;
mod highlight;

use app_core::config;
use dioxus::desktop::{Config, WindowBuilder};
use dioxus::desktop::tao::window::Theme;
use dioxus::prelude::*;

fn main() {
    let prefs = config::load_preferences();
    let theme = match prefs.theme {
        config::Theme::Dark => Theme::Dark,
        config::Theme::Light => Theme::Light,
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
