use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::{
    demo,
    model::{CliTool, DemoData},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sessions,
    Timeline,
    Capsule,
    Branches,
}

#[derive(Debug)]
pub struct App {
    pub data: DemoData,
    pub focus: Focus,
    pub selected_session: usize,
    pub selected_event: usize,
    pub selected_compiler: usize,
    pub command_mode: bool,
    pub command_input: String,
    pub show_help: bool,
    pub show_launch: bool,
    pub show_diff: bool,
    pub verify_passed: bool,
    pub compile_status: &'static str,
    pub pending_g: bool,
    should_quit: bool,
}

impl App {
    pub fn new(source: CliTool, target: CliTool) -> Self {
        Self {
            data: demo::demo_data(source, target),
            focus: Focus::Timeline,
            selected_session: 0,
            selected_event: 6,
            selected_compiler: 0,
            command_mode: false,
            command_input: String::new(),
            show_help: false,
            show_launch: false,
            show_diff: false,
            verify_passed: true,
            compile_status: "ACTIVE",
            pending_g: false,
            should_quit: false,
        }
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.command_mode {
            self.handle_command_key(key);
            return;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
            self.should_quit = true;
            return;
        }

        match key.code {
            KeyCode::Esc => self.back_or_quit(),
            KeyCode::Char('q') => self.back_or_quit(),
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Char(':') => {
                self.command_mode = true;
                self.command_input.clear();
            }
            KeyCode::Tab => self.next_focus(),
            KeyCode::BackTab => self.prev_focus(),
            KeyCode::Char('j') | KeyCode::Down => self.move_down(),
            KeyCode::Char('k') | KeyCode::Up => self.move_up(),
            KeyCode::Char('G') => self.move_bottom(),
            KeyCode::Char('g') => self.handle_g(),
            KeyCode::Char('h') | KeyCode::Left => self.prev_focus(),
            KeyCode::Char('l') | KeyCode::Right => self.next_focus(),
            KeyCode::Char('/') => {
                self.command_mode = true;
                self.command_input.push('/');
            }
            KeyCode::Char(' ') => self.set_rewind_point(),
            KeyCode::Char('c') => self.compile_capsule(),
            KeyCode::Char('v') => self.verify_passed = !self.verify_passed,
            KeyCode::Char('d') => self.show_diff = !self.show_diff,
            KeyCode::Char('s') => self.cycle_compiler(),
            KeyCode::Enter => self.show_launch = true,
            _ => self.pending_g = false,
        }
    }

    fn handle_command_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.command_mode = false;
                self.command_input.clear();
            }
            KeyCode::Enter => {
                let command = self.command_input.trim().to_ascii_lowercase();
                self.command_mode = false;
                self.command_input.clear();
                match command.as_str() {
                    "q" | "quit" => self.should_quit = true,
                    "compile" | "c" => self.compile_capsule(),
                    "verify" | "v" => self.verify_passed = true,
                    "help" | "?" => self.show_help = true,
                    "diff" | "d" => self.show_diff = !self.show_diff,
                    _ => {}
                }
            }
            KeyCode::Backspace => {
                self.command_input.pop();
            }
            KeyCode::Char(ch) => self.command_input.push(ch),
            _ => {}
        }
    }

    fn back_or_quit(&mut self) {
        if self.show_help {
            self.show_help = false;
        } else if self.show_launch {
            self.show_launch = false;
        } else if self.show_diff {
            self.show_diff = false;
        } else {
            self.should_quit = true;
        }
    }

    fn next_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Sessions => Focus::Timeline,
            Focus::Timeline => Focus::Capsule,
            Focus::Capsule => Focus::Branches,
            Focus::Branches => Focus::Sessions,
        };
        self.pending_g = false;
    }

    fn prev_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Sessions => Focus::Branches,
            Focus::Timeline => Focus::Sessions,
            Focus::Capsule => Focus::Timeline,
            Focus::Branches => Focus::Capsule,
        };
        self.pending_g = false;
    }

    fn move_down(&mut self) {
        match self.focus {
            Focus::Sessions => {
                self.selected_session =
                    (self.selected_session + 1).min(self.data.sessions.len().saturating_sub(1));
            }
            Focus::Timeline => {
                self.selected_event =
                    (self.selected_event + 1).min(self.data.timeline.len().saturating_sub(1));
            }
            Focus::Capsule | Focus::Branches => {}
        }
        self.pending_g = false;
    }

    fn move_up(&mut self) {
        match self.focus {
            Focus::Sessions => self.selected_session = self.selected_session.saturating_sub(1),
            Focus::Timeline => self.selected_event = self.selected_event.saturating_sub(1),
            Focus::Capsule | Focus::Branches => {}
        }
        self.pending_g = false;
    }

    fn move_top(&mut self) {
        match self.focus {
            Focus::Sessions => self.selected_session = 0,
            Focus::Timeline => self.selected_event = 0,
            Focus::Capsule | Focus::Branches => {}
        }
        self.pending_g = false;
    }

    fn move_bottom(&mut self) {
        match self.focus {
            Focus::Sessions => self.selected_session = self.data.sessions.len().saturating_sub(1),
            Focus::Timeline => self.selected_event = self.data.timeline.len().saturating_sub(1),
            Focus::Capsule | Focus::Branches => {}
        }
        self.pending_g = false;
    }

    fn handle_g(&mut self) {
        if self.pending_g {
            self.move_top();
        } else {
            self.pending_g = true;
        }
    }

    fn set_rewind_point(&mut self) {
        if let Some(event) = self.data.timeline.get(self.selected_event) {
            self.data.capsule.rewind_point = format!("{} / {}", event.id, event.title);
        }
        self.pending_g = false;
    }

    fn compile_capsule(&mut self) {
        self.compile_status = "COMPILED";
        self.data.capsule.compiler = self.data.compilers[self.selected_compiler].clone();
        self.pending_g = false;
    }

    fn cycle_compiler(&mut self) {
        self.selected_compiler = (self.selected_compiler + 1) % self.data.compilers.len();
        self.data.capsule.compiler = self.data.compilers[self.selected_compiler].clone();
        self.pending_g = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::empty())
    }

    #[test]
    fn space_updates_rewind_point_from_selected_event() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes);
        app.selected_event = 0;
        app.handle_key(key(' '));
        assert!(app.data.capsule.rewind_point.contains("evt-001"));
    }

    #[test]
    fn compiler_cycles() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes);
        let first = app.data.capsule.compiler.clone();
        app.handle_key(key('s'));
        assert_ne!(app.data.capsule.compiler, first);
    }
}
