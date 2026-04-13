use syntect::highlighting::ThemeSet;
use syntect::html::highlighted_html_for_string;
use syntect::parsing::SyntaxSet;

use app_core::config::Theme;

pub fn highlight_sql(sql: &str, theme: Theme) -> String {
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();

    let syntax = ss
        .find_syntax_by_extension("sql")
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    let theme_name = match theme {
        Theme::Dark => "base16-ocean.dark",
        Theme::Light => "base16-ocean.light",
    };
    let theme_obj = &ts.themes[theme_name];

    highlighted_html_for_string(sql, &ss, syntax, theme_obj).unwrap_or_else(|_| html_escape(sql))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
