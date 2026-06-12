use std::cell::Cell;

use ratatui::style::Color;

use crate::core::config::UiThemeName;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub border: Color,
    pub text: Color,
    pub muted: Color,
    pub blue: Color,
    pub cyan: Color,
    pub purple: Color,
    pub green: Color,
    pub gold: Color,
    pub red: Color,
    pub orange: Color,
}

const MOONBOX: Palette = Palette {
    border: Color::Rgb(55, 68, 82),
    text: Color::Rgb(228, 232, 236),
    muted: Color::Rgb(139, 148, 158),
    blue: Color::Rgb(92, 145, 245),
    cyan: Color::Rgb(71, 201, 218),
    purple: Color::Rgb(178, 132, 255),
    green: Color::Rgb(92, 214, 137),
    gold: Color::Rgb(230, 190, 83),
    red: Color::Rgb(241, 101, 101),
    orange: Color::Rgb(245, 151, 86),
};

const TOKYO_NIGHT: Palette = Palette {
    border: Color::Rgb(65, 72, 104),
    text: Color::Rgb(192, 202, 245),
    muted: Color::Rgb(125, 133, 178),
    blue: Color::Rgb(122, 162, 247),
    cyan: Color::Rgb(125, 207, 255),
    purple: Color::Rgb(187, 154, 247),
    green: Color::Rgb(158, 206, 106),
    gold: Color::Rgb(224, 175, 104),
    red: Color::Rgb(247, 118, 142),
    orange: Color::Rgb(255, 158, 100),
};

const GRUVBOX: Palette = Palette {
    border: Color::Rgb(80, 73, 69),
    text: Color::Rgb(235, 219, 178),
    muted: Color::Rgb(168, 153, 132),
    blue: Color::Rgb(131, 165, 152),
    cyan: Color::Rgb(142, 192, 124),
    purple: Color::Rgb(211, 134, 155),
    green: Color::Rgb(184, 187, 38),
    gold: Color::Rgb(250, 189, 47),
    red: Color::Rgb(251, 73, 52),
    orange: Color::Rgb(254, 128, 25),
};

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
    match theme {
        UiThemeName::Moonbox => MOONBOX,
        UiThemeName::TokyoNight => TOKYO_NIGHT,
        UiThemeName::Gruvbox => GRUVBOX,
    }
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
        assert_ne!(
            palette_for(UiThemeName::Moonbox).text,
            palette_for(UiThemeName::TokyoNight).text
        );
        assert_ne!(
            palette_for(UiThemeName::Moonbox).text,
            palette_for(UiThemeName::Gruvbox).text
        );
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
