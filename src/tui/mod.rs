mod snapshot;
mod theme;
mod view;

use std::io::{self, Write};
use std::process::ExitStatus;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::DefaultTerminal;

use crate::{
    app::{App, SessionFilter, TuiExitAction},
    core::{
        launcher,
        model::{CliTool, OriginalSessionPlan},
    },
};

pub fn docs_screenshot_svg(width: u16, height: u16) -> Result<String> {
    snapshot::docs_screenshot_svg(width, height)
}

pub fn run(terminal: &mut DefaultTerminal, mut app: App) -> Result<Option<TuiExitAction>> {
    while !app.should_quit() {
        app.poll_background();
        terminal.draw(|frame| view::render(frame, &app))?;
        if event::poll(Duration::from_millis(120))?
            && let Event::Key(key) = event::read()?
        {
            app.handle_key(key);
            app.poll_background();
            if let Some(text) = app.take_clipboard_text() {
                copy_to_terminal_clipboard(&text)?;
            }
            if let Some(plan) = app.take_pending_resume() {
                suspend_and_resume(terminal, &mut app, plan)?;
            }
        }
    }
    Ok(app.take_exit_action())
}

pub fn run_with_loading(
    terminal: &mut DefaultTerminal,
    source: CliTool,
    target: CliTool,
    filter: Option<CliTool>,
) -> Result<Option<TuiExitAction>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut result = App::new(source, target);
        if let Ok(app) = result.as_mut()
            && let Some(filter) = filter
        {
            app.apply_session_filter(SessionFilter::Tool(filter));
        }
        let _ = sender.send(result);
    });

    let mut tick = 0usize;
    loop {
        terminal.draw(|frame| view::render_loading(frame, tick))?;
        if let Ok(app) = receiver.try_recv() {
            return run(terminal, app?);
        }
        if event::poll(Duration::from_millis(120))?
            && let Event::Key(key) = event::read()?
            && (matches!(key.code, KeyCode::Char('q'))
                || key.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(key.code, KeyCode::Char('c')))
        {
            return Ok(None);
        }
        tick = tick.wrapping_add(1);
    }
}

fn copy_to_terminal_clipboard(text: &str) -> Result<()> {
    let encoded = base64_encode(text.as_bytes());
    print!("\x1b]52;c;{encoded}\x07");
    io::stdout().flush()?;
    Ok(())
}

fn suspend_and_resume(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    plan: Box<OriginalSessionPlan>,
) -> Result<()> {
    suspend_terminal()?;
    let result = run_original_resume(&plan);
    restore_terminal(terminal)?;
    let outcome = match result {
        Ok(status) => original_exit_message(plan.source_session.cli.id(), status),
        Err(error) => format!("{} failed to start: {error}", plan.source_session.cli.id()),
    };
    app.complete_original_resume(&plan, outcome);
    Ok(())
}

fn suspend_terminal() -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

fn restore_terminal(terminal: &mut DefaultTerminal) -> Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    terminal.clear()?;
    Ok(())
}

fn run_original_resume(plan: &OriginalSessionPlan) -> Result<ExitStatus> {
    print!("{}", launcher::original_handoff_notice(plan));
    io::stdout().flush()?;
    Ok(launcher::run_original_interactive(plan.clone())?)
}

fn original_exit_message(cli: &str, status: ExitStatus) -> String {
    if let Some(code) = status.code() {
        format!("{cli} exited (code {code})")
    } else if let Some(signal) = exit_signal(status) {
        format!("{cli} exited (signal {signal})")
    } else {
        format!("{cli} exited")
    }
}

#[cfg(unix)]
fn exit_signal(status: ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;

    status.signal()
}

#[cfg(not(unix))]
fn exit_signal(_status: ExitStatus) -> Option<i32> {
    None
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
