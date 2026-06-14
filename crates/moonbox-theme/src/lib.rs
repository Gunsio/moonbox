#![forbid(unsafe_code)]

use ratatui::style::Color;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ThemeId {
    #[default]
    Moonbox,
    TokyoNight,
    Gruvbox,
    LuoshenSwan,
    LuoshenDragon,
    LuoshenChrysanthemum,
    LuoshenPine,
}

impl ThemeId {
    pub const ALL: [Self; 7] = [
        Self::Moonbox,
        Self::TokyoNight,
        Self::Gruvbox,
        Self::LuoshenSwan,
        Self::LuoshenDragon,
        Self::LuoshenChrysanthemum,
        Self::LuoshenPine,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            Self::Moonbox => "moonbox",
            Self::TokyoNight => "tokyo-night",
            Self::Gruvbox => "gruvbox",
            Self::LuoshenSwan => "luoshen-swan",
            Self::LuoshenDragon => "luoshen-dragon",
            Self::LuoshenChrysanthemum => "luoshen-chrysanthemum",
            Self::LuoshenPine => "luoshen-pine",
        }
    }

    pub fn label(self) -> &'static str {
        self.meta().label
    }

    pub fn short_label(self) -> &'static str {
        self.meta().short_label
    }

    pub fn meta(self) -> ThemeMeta {
        match self {
            Self::Moonbox => ThemeMeta {
                id: self,
                label: "Moonbox",
                short_label: "Moonbox",
                family: ThemeFamily::Moonbox,
                description: "Moonbox's restrained default dark theme.",
                attribution: Attribution::first_party("Moonbox"),
            },
            Self::TokyoNight => ThemeMeta {
                id: self,
                label: "Tokyo Night",
                short_label: "Tokyo Night",
                family: ThemeFamily::Reference,
                description: "A compatibility theme inspired by Tokyo Night.",
                attribution: Attribution {
                    source: "Tokyo Night",
                    homepage: Some("https://github.com/folke/tokyonight.nvim"),
                    license: Some("Apache-2.0"),
                    relationship: ThemeRelationship::InspiredBy,
                },
            },
            Self::Gruvbox => ThemeMeta {
                id: self,
                label: "Gruvbox",
                short_label: "Gruvbox",
                family: ThemeFamily::Reference,
                description: "A compatibility theme inspired by Gruvbox.",
                attribution: Attribution {
                    source: "Gruvbox",
                    homepage: Some("https://github.com/morhetz/gruvbox"),
                    license: Some("MIT"),
                    relationship: ThemeRelationship::InspiredBy,
                },
            },
            Self::LuoshenSwan => ThemeMeta {
                id: self,
                label: "翩若惊鸿 / Startled Swan",
                short_label: "Startled Swan",
                family: ThemeFamily::Luoshen,
                description: "Airy moon-white text, mist blue structure, and pale rose accents for long reading.",
                attribution: Attribution::first_party("Luoshen Theme Pack"),
            },
            Self::LuoshenDragon => ThemeMeta {
                id: self,
                label: "婉若游龙 / Coursing Dragon",
                short_label: "Coursing Dragon",
                family: ThemeFamily::Luoshen,
                description: "Deep indigo, electric cyan, and violet motion for technical workbenches.",
                attribution: Attribution::first_party("Luoshen Theme Pack"),
            },
            Self::LuoshenChrysanthemum => ThemeMeta {
                id: self,
                label: "荣曜秋菊 / Radiant Chrysanthemum",
                short_label: "Radiant Chrysanthemum",
                family: ThemeFamily::Luoshen,
                description: "Warm ink, amber hierarchy, and restrained red-orange state colors.",
                attribution: Attribution::first_party("Luoshen Theme Pack"),
            },
            Self::LuoshenPine => ThemeMeta {
                id: self,
                label: "华茂春松 / Lush Pine",
                short_label: "Lush Pine",
                family: ThemeFamily::Luoshen,
                description: "Pine green, blue-gray structure, and quiet gold emphasis for engineering focus.",
                attribution: Attribution::first_party("Luoshen Theme Pack"),
            },
        }
    }

    pub fn palette(self) -> Palette {
        palette_for(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeFamily {
    Moonbox,
    Reference,
    Luoshen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeMeta {
    pub id: ThemeId,
    pub label: &'static str,
    pub short_label: &'static str,
    pub family: ThemeFamily,
    pub description: &'static str,
    pub attribution: Attribution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attribution {
    pub source: &'static str,
    pub homepage: Option<&'static str>,
    pub license: Option<&'static str>,
    pub relationship: ThemeRelationship,
}

impl Attribution {
    pub const fn first_party(source: &'static str) -> Self {
        Self {
            source,
            homepage: Some("https://github.com/Gunsio/moonbox"),
            license: Some("MIT"),
            relationship: ThemeRelationship::FirstParty,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeRelationship {
    FirstParty,
    InspiredBy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb {
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsiColor {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    Gray,
    DarkGray,
    White,
}

impl AnsiColor {
    pub fn ratatui(self) -> Color {
        match self {
            Self::Black => Color::Black,
            Self::Red => Color::Red,
            Self::Green => Color::Green,
            Self::Yellow => Color::Yellow,
            Self::Blue => Color::Blue,
            Self::Magenta => Color::Magenta,
            Self::Cyan => Color::Cyan,
            Self::Gray => Color::Gray,
            Self::DarkGray => Color::DarkGray,
            Self::White => Color::White,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorToken {
    pub rgb: Rgb,
    pub ansi: AnsiColor,
}

impl ColorToken {
    pub const fn new(rgb: Rgb, ansi: AnsiColor) -> Self {
        Self { rgb, ansi }
    }

    pub fn ratatui(self, mode: ColorMode) -> Color {
        match mode {
            ColorMode::TrueColor => Color::Rgb(self.rgb.red, self.rgb.green, self.rgb.blue),
            ColorMode::Ansi16 => self.ansi.ratatui(),
            ColorMode::NoColor => Color::Reset,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    TrueColor,
    Ansi16,
    NoColor,
}

impl ColorMode {
    pub fn from_env() -> Self {
        Self::from_env_values(
            std::env::var_os("NO_COLOR").is_some(),
            std::env::var("COLORTERM").ok().as_deref(),
            std::env::var("TERM").ok().as_deref(),
        )
    }

    fn from_env_values(no_color: bool, colorterm: Option<&str>, term: Option<&str>) -> Self {
        if no_color {
            return Self::NoColor;
        }

        let truecolor = colorterm
            .map(|value| {
                value.eq_ignore_ascii_case("truecolor") || value.eq_ignore_ascii_case("24bit")
            })
            .unwrap_or(false)
            || term
                .map(|value| {
                    let value = value.to_ascii_lowercase();
                    value.contains("truecolor") || value.contains("direct")
                })
                .unwrap_or(false);

        if truecolor {
            Self::TrueColor
        } else {
            Self::Ansi16
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub border: ColorToken,
    pub text: ColorToken,
    pub muted: ColorToken,
    pub blue: ColorToken,
    pub cyan: ColorToken,
    pub purple: ColorToken,
    pub green: ColorToken,
    pub gold: ColorToken,
    pub red: ColorToken,
    pub orange: ColorToken,
}

impl Palette {
    pub fn ratatui(self, mode: ColorMode) -> RatatuiPalette {
        RatatuiPalette {
            border: self.border.ratatui(mode),
            text: self.text.ratatui(mode),
            muted: self.muted.ratatui(mode),
            blue: self.blue.ratatui(mode),
            cyan: self.cyan.ratatui(mode),
            purple: self.purple.ratatui(mode),
            green: self.green.ratatui(mode),
            gold: self.gold.ratatui(mode),
            red: self.red.ratatui(mode),
            orange: self.orange.ratatui(mode),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RatatuiPalette {
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

pub fn all_themes() -> &'static [ThemeId] {
    &ThemeId::ALL
}

pub fn palette_for(theme: ThemeId) -> Palette {
    match theme {
        ThemeId::Moonbox => MOONBOX,
        ThemeId::TokyoNight => TOKYO_NIGHT,
        ThemeId::Gruvbox => GRUVBOX,
        ThemeId::LuoshenSwan => LUOSHEN_SWAN,
        ThemeId::LuoshenDragon => LUOSHEN_DRAGON,
        ThemeId::LuoshenChrysanthemum => LUOSHEN_CHRYSANTHEMUM,
        ThemeId::LuoshenPine => LUOSHEN_PINE,
    }
}

pub fn ratatui_palette_for(theme: ThemeId, mode: ColorMode) -> RatatuiPalette {
    palette_for(theme).ratatui(mode)
}

const fn token(red: u8, green: u8, blue: u8, ansi: AnsiColor) -> ColorToken {
    ColorToken::new(Rgb::new(red, green, blue), ansi)
}

const MOONBOX: Palette = Palette {
    border: token(55, 68, 82, AnsiColor::DarkGray),
    text: token(228, 232, 236, AnsiColor::White),
    muted: token(139, 148, 158, AnsiColor::Gray),
    blue: token(92, 145, 245, AnsiColor::Blue),
    cyan: token(71, 201, 218, AnsiColor::Cyan),
    purple: token(178, 132, 255, AnsiColor::Magenta),
    green: token(92, 214, 137, AnsiColor::Green),
    gold: token(230, 190, 83, AnsiColor::Yellow),
    red: token(241, 101, 101, AnsiColor::Red),
    orange: token(245, 151, 86, AnsiColor::Yellow),
};

const TOKYO_NIGHT: Palette = Palette {
    border: token(65, 72, 104, AnsiColor::DarkGray),
    text: token(192, 202, 245, AnsiColor::White),
    muted: token(125, 133, 178, AnsiColor::Gray),
    blue: token(122, 162, 247, AnsiColor::Blue),
    cyan: token(125, 207, 255, AnsiColor::Cyan),
    purple: token(187, 154, 247, AnsiColor::Magenta),
    green: token(158, 206, 106, AnsiColor::Green),
    gold: token(224, 175, 104, AnsiColor::Yellow),
    red: token(247, 118, 142, AnsiColor::Red),
    orange: token(255, 158, 100, AnsiColor::Yellow),
};

const GRUVBOX: Palette = Palette {
    border: token(80, 73, 69, AnsiColor::DarkGray),
    text: token(235, 219, 178, AnsiColor::White),
    muted: token(168, 153, 132, AnsiColor::Gray),
    blue: token(131, 165, 152, AnsiColor::Blue),
    cyan: token(142, 192, 124, AnsiColor::Cyan),
    purple: token(211, 134, 155, AnsiColor::Magenta),
    green: token(184, 187, 38, AnsiColor::Green),
    gold: token(250, 189, 47, AnsiColor::Yellow),
    red: token(251, 73, 52, AnsiColor::Red),
    orange: token(254, 128, 25, AnsiColor::Yellow),
};

const LUOSHEN_SWAN: Palette = Palette {
    border: token(92, 112, 138, AnsiColor::DarkGray),
    text: token(232, 239, 244, AnsiColor::White),
    muted: token(148, 162, 178, AnsiColor::Gray),
    blue: token(116, 165, 214, AnsiColor::Blue),
    cyan: token(118, 203, 211, AnsiColor::Cyan),
    purple: token(176, 148, 203, AnsiColor::Magenta),
    green: token(130, 190, 157, AnsiColor::Green),
    gold: token(222, 187, 122, AnsiColor::Yellow),
    red: token(224, 124, 146, AnsiColor::Red),
    orange: token(226, 151, 111, AnsiColor::Yellow),
};

const LUOSHEN_DRAGON: Palette = Palette {
    border: token(52, 78, 111, AnsiColor::DarkGray),
    text: token(220, 232, 255, AnsiColor::White),
    muted: token(122, 143, 178, AnsiColor::Gray),
    blue: token(91, 143, 255, AnsiColor::Blue),
    cyan: token(63, 214, 229, AnsiColor::Cyan),
    purple: token(164, 122, 255, AnsiColor::Magenta),
    green: token(92, 210, 164, AnsiColor::Green),
    gold: token(226, 192, 96, AnsiColor::Yellow),
    red: token(239, 92, 120, AnsiColor::Red),
    orange: token(242, 142, 83, AnsiColor::Yellow),
};

const LUOSHEN_CHRYSANTHEMUM: Palette = Palette {
    border: token(93, 76, 61, AnsiColor::DarkGray),
    text: token(238, 224, 190, AnsiColor::White),
    muted: token(172, 151, 122, AnsiColor::Gray),
    blue: token(119, 157, 174, AnsiColor::Blue),
    cyan: token(121, 184, 165, AnsiColor::Cyan),
    purple: token(190, 131, 159, AnsiColor::Magenta),
    green: token(161, 184, 95, AnsiColor::Green),
    gold: token(239, 189, 73, AnsiColor::Yellow),
    red: token(237, 91, 70, AnsiColor::Red),
    orange: token(236, 128, 55, AnsiColor::Yellow),
};

const LUOSHEN_PINE: Palette = Palette {
    border: token(60, 89, 80, AnsiColor::DarkGray),
    text: token(219, 232, 218, AnsiColor::White),
    muted: token(133, 157, 145, AnsiColor::Gray),
    blue: token(104, 154, 171, AnsiColor::Blue),
    cyan: token(93, 190, 176, AnsiColor::Cyan),
    purple: token(166, 139, 185, AnsiColor::Magenta),
    green: token(116, 190, 113, AnsiColor::Green),
    gold: token(221, 184, 91, AnsiColor::Yellow),
    red: token(226, 96, 91, AnsiColor::Red),
    orange: token(224, 143, 82, AnsiColor::Yellow),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_theme_slugs_are_stable() {
        let slugs: Vec<&str> = all_themes().iter().map(|theme| theme.slug()).collect();
        assert_eq!(
            slugs,
            vec![
                "moonbox",
                "tokyo-night",
                "gruvbox",
                "luoshen-swan",
                "luoshen-dragon",
                "luoshen-chrysanthemum",
                "luoshen-pine",
            ]
        );
    }

    #[test]
    fn luoshen_theme_labels_are_bilingual() {
        assert_eq!(ThemeId::LuoshenSwan.label(), "翩若惊鸿 / Startled Swan");
        assert_eq!(ThemeId::LuoshenDragon.short_label(), "Coursing Dragon");
        assert_eq!(
            ThemeId::LuoshenChrysanthemum.meta().family,
            ThemeFamily::Luoshen
        );
        assert_eq!(
            ThemeId::LuoshenPine.meta().attribution.relationship,
            ThemeRelationship::FirstParty
        );
    }

    #[test]
    fn color_modes_degrade_to_ratatui_colors() {
        let palette = ratatui_palette_for(ThemeId::LuoshenDragon, ColorMode::Ansi16);
        assert_eq!(palette.blue, Color::Blue);
        assert_eq!(palette.gold, Color::Yellow);

        let palette = ratatui_palette_for(ThemeId::LuoshenDragon, ColorMode::NoColor);
        assert_eq!(palette.blue, Color::Reset);
        assert_eq!(palette.text, Color::Reset);
    }

    #[test]
    fn color_mode_env_detection_prefers_no_color_then_truecolor_then_ansi() {
        assert_eq!(
            ColorMode::from_env_values(true, Some("truecolor"), Some("xterm-direct")),
            ColorMode::NoColor
        );
        assert_eq!(
            ColorMode::from_env_values(false, Some("24bit"), Some("xterm-256color")),
            ColorMode::TrueColor
        );
        assert_eq!(
            ColorMode::from_env_values(false, None, Some("xterm-direct")),
            ColorMode::TrueColor
        );
        assert_eq!(
            ColorMode::from_env_values(false, None, Some("xterm-256color")),
            ColorMode::Ansi16
        );
    }

    #[test]
    fn luoshen_palettes_have_distinct_text_and_accent_colors() {
        assert_ne!(
            palette_for(ThemeId::LuoshenSwan).blue,
            palette_for(ThemeId::LuoshenDragon).blue
        );
        assert_ne!(
            palette_for(ThemeId::LuoshenChrysanthemum).gold,
            palette_for(ThemeId::LuoshenPine).gold
        );
    }

    #[test]
    fn vendored_theme_pack_matches_workspace_crate() {
        if env!("CARGO_PKG_NAME") != "moonbox" {
            return;
        }

        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let crate_path = manifest_dir.join("crates/moonbox-theme/src/lib.rs");
        if !crate_path.exists() {
            return;
        }

        let crate_source =
            std::fs::read_to_string(crate_path).expect("theme crate source should be readable");
        let vendored_source = std::fs::read_to_string(manifest_dir.join("src/theme_pack.rs"))
            .expect("vendored theme source should be readable");
        let crate_source = crate_source
            .strip_prefix("#![forbid(unsafe_code)]\n\n")
            .expect("theme crate should only wrap the shared source with unsafe policy");

        assert_eq!(vendored_source, crate_source);
    }
}
