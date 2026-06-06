use std::fmt::Write as _;

use color_eyre::Result;
use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::{Buffer, Cell},
    style::{Color, Modifier},
};

use crate::{
    app::App,
    core::model::{CliTool, DoctorReport, VerificationStatus},
};

use super::view;

const DEFAULT_FG: &str = "#f0f3f6";
const TERMINAL_BG: &str = "#050607";
const CELL_WIDTH: usize = 9;
const CELL_HEIGHT: usize = 18;
const PADDING: usize = 28;

pub fn docs_screenshot_svg(width: u16, height: u16) -> Result<String> {
    let mut app = App::new_fixture(CliTool::Codex, CliTool::Hermes)?;
    app.show_launch = true;
    app.launch_review = true;
    app.pending_target = CliTool::Hermes;
    app.doctor_report = DoctorReport {
        version: 1,
        status: VerificationStatus::Pass,
        ready: true,
        checks: Vec::new(),
    };

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|frame| view::render(frame, &app))?;

    Ok(buffer_to_svg(terminal.backend().buffer()))
}

fn buffer_to_svg(buffer: &Buffer) -> String {
    let terminal_width = buffer.area.width as usize * CELL_WIDTH;
    let terminal_height = buffer.area.height as usize * CELL_HEIGHT;
    let width = terminal_width + PADDING * 2;
    let height = terminal_height + PADDING * 2;

    let mut svg = String::new();
    write!(
        svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" role="img" aria-labelledby="title desc">
  <title id="title">Moonbox TUI screenshot</title>
  <desc id="desc">A generated terminal screenshot of Moonbox showing Launch Review, verifier readiness details, and Vim-style key hints.</desc>
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
    )
    .expect("write svg header");

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
        writeln!(
            svg,
            r##"  <rect x="{px}" y="{}" width="{width}" height="{CELL_HEIGHT}" fill="{}"/>"##,
            py.saturating_sub(CELL_HEIGHT - 3),
            color_hex(cell.bg, TERMINAL_BG)
        )
        .expect("write bg");
    }

    let weight = if cell.modifier.contains(Modifier::BOLD) {
        "800"
    } else {
        "500"
    };
    writeln!(
        svg,
        r##"  <text x="{px}" y="{py}" class="mono" font-size="14" font-weight="{weight}" fill="{}">{}</text>"##,
        color_hex(cell.fg, DEFAULT_FG),
        escape_xml(text)
    )
    .expect("write text");
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
    fn docs_screenshot_contains_launch_review_state() {
        let svg = docs_screenshot_svg(120, 36).expect("svg");

        assert!(svg.contains("Launch Review"));
        assert!(svg.contains("Readiness details"));
        assert!(svg.contains("moonbox launch --execute"));
        assert!(svg.contains("enter disabled"));
        assert!(svg.contains("<svg"));
    }
}
