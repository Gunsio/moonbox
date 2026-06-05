mod theme;
mod view;

use std::time::Duration;

use color_eyre::Result;
use crossterm::event::{self, Event};
use ratatui::DefaultTerminal;

use crate::app::App;

pub fn run(terminal: &mut DefaultTerminal, mut app: App) -> Result<()> {
    while !app.should_quit() {
        terminal.draw(|frame| view::render(frame, &app))?;
        if event::poll(Duration::from_millis(120))?
            && let Event::Key(key) = event::read()?
        {
            app.handle_key(key);
        }
    }
    Ok(())
}
