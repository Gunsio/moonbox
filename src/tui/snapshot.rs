use color_eyre::Result;
use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::{Buffer, Cell},
    style::{Color, Modifier},
};

use crate::{
    app::{App, Focus},
    core::{
        config::{UiLanguage, UiPreferencesConfig, UiThemeName},
        model::{CliTool, DoctorReport, VerificationStatus},
    },
};

use super::view;

const DEFAULT_FG: &str = "#f0f3f6";
const TERMINAL_BG: &str = "#050607";
const CELL_WIDTH: usize = 9;
const CELL_HEIGHT: usize = 18;
const PADDING: usize = 28;

pub fn docs_screenshot_svg(width: u16, height: u16, scene: &str) -> Result<String> {
    let app = docs_scene_app(scene)?;
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|frame| view::render(frame, &app))?;

    Ok(buffer_to_svg(terminal.backend().buffer(), scene))
}

fn docs_scene_app(scene: &str) -> Result<App> {
    let mut app = App::new_fixture(CliTool::Codex, CliTool::Hermes)?;
    app.set_ui_preferences_for_render(UiPreferencesConfig {
        language: UiLanguage::English,
        theme: UiThemeName::LuoshenSwan,
    });
    app.doctor_report = DoctorReport {
        version: 1,
        status: VerificationStatus::Pass,
        ready: true,
        source_adapters: Vec::new(),
        checks: Vec::new(),
    };

    match scene {
        "action-menu" => {
            app.show_action_menu = true;
            app.status_message = "Session actions".into();
        }
        "yank" => {
            app.show_share_panel = true;
            app.status_message = "Choose yank action".into();
        }
        "timeline-details" => {
            app.focus = Focus::Timeline;
            app.selected_event = app.data.timeline.len().saturating_sub(1);
            app.zoomed_focus = Some(Focus::Timeline);
            app.status_message = "Timeline zoom".into();
        }
        "handoff" => {
            app.data.compilers.insert(0, "agent:codex:handoff".into());
            app.selected_compiler = 0;
            app.data.capsule.compiler = "agent:codex:handoff".into();
            if let Some(raw_source_map) = &mut app.data.capsule.raw_source_map {
                raw_source_map.generated_by = "agent:codex:handoff".into();
            }
            app.data.capsule.handoff_runner = Some("Codex".into());
            app.data.capsule.handoff_skill = Some("handoff".into());
            app.data.capsule.handoff_artifact_path =
                Some("/tmp/moonbox-handoff-codex-to-hermes.md".into());
            app.data.capsule.handoff_artifact = Some(
                "# Handoff\n\nContinue from the selected rewind point with the exact context the target agent needs.\n\n## Next steps\n- Re-open the product thread.\n- Verify the changed TUI copy.\n- Keep source stores read-only."
                    .into(),
            );
            app.show_launch = true;
            app.launch_review = true;
            app.pending_target = CliTool::Hermes;
            app.status_message = "Handoff ready".into();
        }
        _ => {
            app.show_launch = true;
            app.launch_review = true;
            app.pending_target = CliTool::Hermes;
            app.start_handoff_trail_for_review();
            app.status_message = "Handoff Review".into();
        }
    }

    Ok(app)
}

fn buffer_to_svg(buffer: &Buffer, scene: &str) -> String {
    let terminal_width = buffer.area.width as usize * CELL_WIDTH;
    let terminal_height = buffer.area.height as usize * CELL_HEIGHT;
    let width = terminal_width + PADDING * 2;
    let height = terminal_height + PADDING * 2;

    let mut svg = String::new();
    svg.push_str(&format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" role="img" aria-labelledby="title desc">
  <title id="title">Moonbox TUI screenshot: {scene}</title>
  <desc id="desc">A generated terminal screenshot of Moonbox showing the {scene} fixture scene with session management, handoff, and Vim-style key hints.</desc>
  <defs>
    <filter id="shadow" x="-10%" y="-10%" width="120%" height="120%">
      <feDropShadow dx="0" dy="18" stdDeviation="26" flood-color="#000000" flood-opacity="0.40"/>
    </filter>
    <style>
      .mono {{ font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace; white-space: pre; }}
    </style>
  </defs>
  <rect x="{PADDING}" y="{PADDING}" width="{terminal_width}" height="{terminal_height}" rx="8" fill="{TERMINAL_BG}" stroke="#58616f" stroke-width="2" filter="url(#shadow)"/>
"##
    ));

    for y in 0..buffer.area.height {
        write_svg_row(buffer, y, &mut svg);
    }

    svg.push_str("</svg>\n");
    svg
}

fn write_svg_row(buffer: &Buffer, y: u16, svg: &mut String) {
    let mut x = 0;
    while x < buffer.area.width {
        let cell = &buffer[(x, y)];
        if cell.symbol() == " " {
            x += 1;
            continue;
        }

        let start = x;
        let mut text = String::new();
        let mut last_non_space_len = 0;
        while x < buffer.area.width {
            let next = &buffer[(x, y)];
            if !same_text_style(cell, next) {
                break;
            }
            text.push_str(next.symbol());
            if next.symbol() != " " {
                last_non_space_len = text.len();
            }
            x += 1;
        }
        text.truncate(last_non_space_len);
        write_svg_run(start, y, cell, &text, svg);
    }
}

fn write_svg_run(x: u16, y: u16, cell: &Cell, text: &str, svg: &mut String) {
    let px = PADDING + x as usize * CELL_WIDTH;
    let py = PADDING + (y as usize + 1) * CELL_HEIGHT - 4;
    let width = text.chars().count() * CELL_WIDTH;
    if cell.bg != Color::Reset {
        let y = py.saturating_sub(CELL_HEIGHT - 3);
        let fill = color_hex(cell.bg, TERMINAL_BG);
        svg.push_str(&format!(
            r##"  <rect x="{px}" y="{y}" width="{width}" height="{CELL_HEIGHT}" fill="{fill}"/>
"##
        ));
    }

    let weight = if cell.modifier.contains(Modifier::BOLD) {
        "800"
    } else {
        "500"
    };
    let fill = color_hex(cell.fg, DEFAULT_FG);
    let text = escape_xml(text);
    svg.push_str(&format!(
        r##"  <text x="{px}" y="{py}" class="mono" font-size="14" font-weight="{weight}" fill="{fill}">{text}</text>
"##
    ));
}

fn same_text_style(left: &Cell, right: &Cell) -> bool {
    left.fg == right.fg && left.bg == right.bg && left.modifier == right.modifier
}

fn color_hex(color: Color, default: &str) -> String {
    match color {
        Color::Reset => default.into(),
        Color::Black => "#000000".into(),
        Color::Red => "#cc3d3d".into(),
        Color::Green => "#4fbf67".into(),
        Color::Yellow => "#d7b84f".into(),
        Color::Blue => "#5c91f5".into(),
        Color::Magenta => "#c678dd".into(),
        Color::Cyan => "#47c9da".into(),
        Color::Gray => "#8b949e".into(),
        Color::DarkGray => "#58616f".into(),
        Color::LightRed => "#f16565".into(),
        Color::LightGreen => "#5cd689".into(),
        Color::LightYellow => "#e6be53".into(),
        Color::LightBlue => "#6ea8fe".into(),
        Color::LightMagenta => "#d78cff".into(),
        Color::LightCyan => "#6ce6f2".into(),
        Color::White => "#f0f3f6".into(),
        Color::Rgb(red, green, blue) => format!("#{red:02x}{green:02x}{blue:02x}"),
        Color::Indexed(index) => indexed_color(index).into(),
    }
}

fn indexed_color(index: u8) -> &'static str {
    match index {
        0 => "#000000",
        1 => "#cc3d3d",
        2 => "#4fbf67",
        3 => "#d7b84f",
        4 => "#5c91f5",
        5 => "#c678dd",
        6 => "#47c9da",
        7 => "#8b949e",
        _ => DEFAULT_FG,
    }
}

fn escape_xml(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docs_screenshot_contains_handoff_review_state() {
        let svg = docs_screenshot_svg(120, 36, "handoff").expect("svg");

        assert!(svg.contains("Handoff Review"));
        assert!(svg.contains("Handoff ready"));
        assert!(svg.contains("Handoff Body"));
        assert!(svg.contains("Continue from the selected rewind point"));
        assert!(svg.contains("y"));
        assert!(svg.contains("Copy text"));
        assert!(svg.contains("<svg"));
    }

    #[test]
    fn docs_screenshot_contains_action_and_yank_scenes() {
        let action = docs_screenshot_svg(120, 36, "action-menu").expect("action svg");
        let yank = docs_screenshot_svg(120, 36, "yank").expect("yank svg");

        assert!(action.contains("Action Menu"));
        assert!(action.contains("Session actions"));
        assert!(action.contains("Resume"));
        assert!(action.contains("Archive"));
        assert!(yank.contains("Yank"));
        assert!(yank.contains("First user input"));
        assert!(yank.contains("Portable JSON"));
    }

    #[test]
    fn docs_screenshot_contains_timeline_details_scene() {
        let svg = docs_screenshot_svg(120, 36, "timeline-details").expect("svg");

        assert!(svg.contains("Timeline"));
        assert!(svg.contains("Action Path"));
        assert!(!svg.contains("Session Details"));
        assert!(svg.contains("<svg"));
    }
}
