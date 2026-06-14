use std::cell::Cell;

use ratatui::style::Color;

use crate::{core::config::UiThemeName, moonbox_theme};

pub type Palette = moonbox_theme::RatatuiPalette;

thread_local! {
    static CURRENT_THEME: Cell<UiThemeName> = const { Cell::new(UiThemeName::Moonbox) };
}

pub struct ThemeGuard {
    previous: UiThemeName,
}

impl Drop for ThemeGuard {
    fn drop(&mut self) {
        set_current(self.previous);
    }
}

pub fn use_current(theme: UiThemeName) -> ThemeGuard {
    let previous = current_name();
    set_current(theme);
    ThemeGuard { previous }
}

pub fn set_current(theme: UiThemeName) {
    CURRENT_THEME.with(|current| current.set(theme));
}

pub fn current_name() -> UiThemeName {
    CURRENT_THEME.with(Cell::get)
}

pub fn palette_for(theme: UiThemeName) -> Palette {
    moonbox_theme::ratatui_palette_for(theme.into(), moonbox_theme::ColorMode::from_env())
}

pub fn current_palette() -> Palette {
    palette_for(current_name())
}

pub fn border() -> Color {
    current_palette().border
}

pub fn text() -> Color {
    current_palette().text
}

pub fn muted() -> Color {
    current_palette().muted
}

pub fn blue() -> Color {
    current_palette().blue
}

pub fn cyan() -> Color {
    current_palette().cyan
}

pub fn purple() -> Color {
    current_palette().purple
}

pub fn green() -> Color {
    current_palette().green
}

pub fn gold() -> Color {
    current_palette().gold
}

pub fn red() -> Color {
    current_palette().red
}

pub fn orange() -> Color {
    current_palette().orange
}

pub fn role_rewind() -> Color {
    gold()
}

pub fn role_target() -> Color {
    cyan()
}

pub fn confidence_strong() -> Color {
    green()
}

pub fn confidence_medium() -> Color {
    gold()
}

pub fn confidence_weak() -> Color {
    orange()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palettes_have_distinct_text_colors() {
        let moonbox = moonbox_theme::ratatui_palette_for(
            UiThemeName::Moonbox.into(),
            moonbox_theme::ColorMode::TrueColor,
        );
        let tokyo = moonbox_theme::ratatui_palette_for(
            UiThemeName::TokyoNight.into(),
            moonbox_theme::ColorMode::TrueColor,
        );
        let gruvbox = moonbox_theme::ratatui_palette_for(
            UiThemeName::Gruvbox.into(),
            moonbox_theme::ColorMode::TrueColor,
        );
        let luoshen = moonbox_theme::ratatui_palette_for(
            UiThemeName::LuoshenDragon.into(),
            moonbox_theme::ColorMode::TrueColor,
        );
        assert_ne!(moonbox.text, tokyo.text);
        assert_ne!(moonbox.text, gruvbox.text);
        assert_ne!(moonbox.blue, luoshen.blue);
    }

    #[test]
    fn theme_guard_restores_previous_theme() {
        set_current(UiThemeName::Gruvbox);
        {
            let _guard = use_current(UiThemeName::TokyoNight);
            assert_eq!(current_name(), UiThemeName::TokyoNight);
        }
        assert_eq!(current_name(), UiThemeName::Gruvbox);
        set_current(UiThemeName::Moonbox);
    }
}
