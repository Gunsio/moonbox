mod snapshot;
mod theme;
mod view;

use std::io::{self, Write};
use std::time::Duration;

use color_eyre::Result;
use crossterm::event::{self, Event};
use ratatui::DefaultTerminal;

use crate::app::App;

pub fn docs_screenshot_svg(width: u16, height: u16) -> Result<String> {
    snapshot::docs_screenshot_svg(width, height)
}

pub fn run(terminal: &mut DefaultTerminal, mut app: App) -> Result<()> {
    while !app.should_quit() {
        terminal.draw(|frame| view::render(frame, &app))?;
        if event::poll(Duration::from_millis(120))?
            && let Event::Key(key) = event::read()?
        {
            app.handle_key(key);
            if let Some(text) = app.take_clipboard_text() {
                copy_to_terminal_clipboard(&text)?;
            }
        }
    }
    Ok(())
}

fn copy_to_terminal_clipboard(text: &str) -> Result<()> {
    let encoded = base64_encode(text.as_bytes());
    print!("\x1b]52;c;{encoded}\x07");
    io::stdout().flush()?;
    Ok(())
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        output.push(TABLE[(b0 >> 2) as usize] as char);
        output.push(TABLE[(((b0 & 0b0000_0011) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(TABLE[(((b1 & 0b0000_1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::base64_encode;

    #[test]
    fn base64_encoder_handles_padding() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"m"), "bQ==");
        assert_eq!(base64_encode(b"mo"), "bW8=");
        assert_eq!(base64_encode(b"moon"), "bW9vbg==");
    }
}
