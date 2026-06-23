mod i18n;
mod snapshot;
mod theme;
mod view;

use std::io::{self, Write};
#[cfg(target_os = "macos")]
use std::process::Stdio;
use std::process::{Command, ExitStatus};
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
    app::{App, LarkExportTuiPlan, SessionFilter, SetupInstallPlan, TmuxJumpPlan, TuiExitAction},
    core::{
        config, lark, launcher,
        model::{CliTool, LaunchExecution, LaunchPlan, OriginalSessionPlan},
        tmux, workbench,
    },
};

pub fn docs_screenshot_svg(width: u16, height: u16, scene: &str) -> Result<String> {
    snapshot::docs_screenshot_svg(width, height, scene)
}

pub fn run(terminal: &mut DefaultTerminal, mut app: App) -> Result<Option<TuiExitAction>> {
    while !app.should_quit() {
        app.poll_background();
        app.advance_animation();
        terminal.draw(|frame| view::render(frame, &app))?;
        if event::poll(Duration::from_millis(120))?
            && let Event::Key(key) = event::read()?
        {
            app.handle_key(key);
            app.poll_background();
            if let Some(text) = app.take_clipboard_text() {
                copy_to_terminal_clipboard(&text)?;
            }
            if let Some(plan) = app.take_pending_tmux_jump() {
                run_tmux_jump(&mut app, plan);
            }
            if let Some(plan) = app.take_pending_resume() {
                suspend_and_resume(terminal, &mut app, plan)?;
            }
            if let Some(plan) = app.take_pending_native_fork() {
                suspend_and_resume(terminal, &mut app, plan)?;
            }
            if let Some(plan) = app.take_pending_seed_prompt() {
                suspend_and_resume(terminal, &mut app, plan)?;
            }
            if let Some(plan) = app.take_pending_launch() {
                suspend_and_launch(terminal, &mut app, plan)?;
            }
            if let Some(plan) = app.take_pending_setup_install() {
                suspend_and_setup_install(terminal, &mut app, plan)?;
            }
            if let Some(plan) = app.take_pending_lark_export() {
                suspend_and_lark_export(terminal, &mut app, plan)?;
            }
        }
    }
    Ok(app.take_exit_action())
}

fn run_tmux_jump(app: &mut App, plan: Box<TmuxJumpPlan>) {
    let result = tmux::execute_jump(&plan.command);
    app.complete_tmux_jump(plan, result);
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
    #[cfg(not(test))]
    let language = config::load_ui_preferences_config().language;
    #[cfg(test)]
    let language = config::UiLanguage::English;
    loop {
        terminal.draw(|frame| view::render_loading(frame, tick, language))?;
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
    let _ = copy_to_system_clipboard(text);
    let encoded = base64_encode(text.as_bytes());
    print!("\x1b]52;c;{encoded}\x07");
    io::stdout().flush()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn copy_to_system_clipboard(text: &str) -> Result<()> {
    let mut child = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|error| color_eyre::eyre::eyre!("pbcopy failed to start: {error}"))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(text.as_bytes())?;
    }
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(color_eyre::eyre::eyre!("pbcopy exited with {status}"))
    }
}

#[cfg(not(target_os = "macos"))]
fn copy_to_system_clipboard(_text: &str) -> Result<()> {
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

fn suspend_and_launch(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    plan: Box<LaunchPlan>,
) -> Result<()> {
    suspend_terminal()?;
    let result = run_target_handoff(&plan);
    restore_terminal(terminal)?;
    app.complete_target_handoff(plan, result);
    Ok(())
}

fn suspend_and_setup_install(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    plan: Box<SetupInstallPlan>,
) -> Result<()> {
    suspend_terminal()?;
    let result = run_setup_install(&plan);
    restore_terminal(terminal)?;
    let (success, outcome) = match result {
        Ok(status) if status.success() => (true, "installed; state refreshed".into()),
        Ok(status) => (
            false,
            match status.code() {
                Some(code) => format!("installer exited with code {code}"),
                None => "installer exited without a status code".into(),
            },
        ),
        Err(error) => (false, format!("installer failed to start: {error}")),
    };
    app.complete_setup_install(&plan, outcome, success);
    Ok(())
}

fn suspend_and_lark_export(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    plan: Box<LarkExportTuiPlan>,
) -> Result<()> {
    suspend_terminal()?;
    let result = run_lark_export(&plan);
    restore_terminal(terminal)?;
    let (success, outcome) = match result {
        Ok(status) if status.success() => (true, "document created".into()),
        Ok(status) => (
            false,
            match status.code() {
                Some(code) => format!("export exited with code {code}"),
                None => "export exited without a status code".into(),
            },
        ),
        Err(error) => (false, format!("export failed to start: {error}")),
    };
    app.complete_lark_export(&plan, outcome, success);
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

fn run_target_handoff(plan: &LaunchPlan) -> Result<LaunchExecution, crate::core::error::CoreError> {
    print!("{}", launcher::target_handoff_notice(plan));
    io::stdout()
        .flush()
        .map_err(|error| crate::core::error::CoreError::LaunchStart {
            command: plan.target_command.display.clone(),
            reason: error.to_string(),
        })?;
    workbench::execute_tui_launch_plan(plan.clone())
}

fn run_setup_install(plan: &SetupInstallPlan) -> io::Result<ExitStatus> {
    println!("Moonbox setup: {}", plan.label);
    println!("Command: {}", plan.command_display);
    println!("Moonbox will return when the installer exits.\n");
    io::stdout().flush()?;
    Command::new(std::env::current_exe()?)
        .arg("setup")
        .arg("install")
        .arg(plan.target.cli_arg())
        .status()
}

fn run_lark_export(plan: &LarkExportTuiPlan) -> io::Result<ExitStatus> {
    println!("Moonbox Lark export");
    println!("Session: {}", plan.session_id);
    println!("Target: {}", plan.target);
    println!("Rewind: {}", plan.rewind);
    println!("Compiler: {}", plan.compiler);
    println!("Title: {}", plan.title);
    println!("Command: {}", plan.command_display);
    println!("Creating the Feishu/Lark document from the reviewed handoff Markdown.");
    println!("Moonbox will return when the export exits.\n");
    io::stdout().flush()?;
    let markdown_path = lark::write_markdown_temp(&lark::markdown_with_document_title(
        &plan.title,
        &plan.markdown,
    ))?;
    let markdown_arg = lark::markdown_file_arg(&markdown_path);
    let output = Command::new(lark_cli_bin())
        .arg("docs")
        .arg("+create")
        .arg("--api-version")
        .arg("v2")
        .arg("--as")
        .arg("user")
        .arg("--doc-format")
        .arg("markdown")
        .arg("--content")
        .arg(&markdown_arg)
        .output();
    let _ = std::fs::remove_file(&markdown_path);
    let output = output?;
    io::stdout().write_all(&output.stdout)?;
    io::stderr().write_all(&output.stderr)?;
    if output.status.success()
        && let Some(url) = extract_first_url(&String::from_utf8_lossy(&output.stdout))
            .or_else(|| extract_first_url(&String::from_utf8_lossy(&output.stderr)))
    {
        if let Some(request) = lark::title_patch_request(&url, &plan.title) {
            let title_output = Command::new(lark_cli_bin())
                .arg("drive")
                .arg("files")
                .arg("patch")
                .arg("--as")
                .arg("user")
                .arg("--params")
                .arg(request.params)
                .arg("--data")
                .arg(request.data)
                .output()?;
            io::stdout().write_all(&title_output.stdout)?;
            io::stderr().write_all(&title_output.stderr)?;
            if !title_output.status.success() {
                return Ok(title_output.status);
            }
        }
        if std::env::var_os("MOONBOX_LARK_DISABLE_OPEN").is_none() {
            let _ = Command::new("open").arg(url).status();
        }
    }
    Ok(output.status)
}

fn lark_cli_bin() -> std::path::PathBuf {
    std::env::var_os("MOONBOX_LARK_CLI_BIN")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("lark-cli"))
}

fn extract_first_url(text: &str) -> Option<String> {
    if let Some(start) = text.find("http") {
        let tail = &text[start..];
        let end = tail
            .find(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | ',' | ')' | ']' | '}'))
            .unwrap_or(tail.len());
        return Some(tail[..end].trim_end_matches('.').to_string());
    }
    text.split_whitespace().find_map(|part| {
        let clean = part.trim_matches(|ch: char| {
            ch == '"' || ch == '\'' || ch == ',' || ch == ')' || ch == ']' || ch == '}'
        });
        clean
            .starts_with("http")
            .then(|| clean.trim_end_matches('.').to_string())
    })
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
