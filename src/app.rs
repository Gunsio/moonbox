use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::{
    config, demo,
    model::{CliTool, DemoData, SessionSummary},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sessions,
    Timeline,
    Capsule,
    Branches,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionFilter {
    All,
    Tool(CliTool),
}

impl SessionFilter {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Tool(CliTool::Codex) => "Codex",
            Self::Tool(CliTool::Claude) => "Claude",
            Self::Tool(CliTool::Hermes) => "Hermes",
        }
    }

    fn matches(self, session: &SessionSummary) -> bool {
        match self {
            Self::All => true,
            Self::Tool(tool) => session.cli == tool,
        }
    }

    fn next(self) -> Self {
        match self {
            Self::All => Self::Tool(CliTool::Codex),
            Self::Tool(CliTool::Codex) => Self::Tool(CliTool::Claude),
            Self::Tool(CliTool::Claude) => Self::Tool(CliTool::Hermes),
            Self::Tool(CliTool::Hermes) => Self::All,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::All => Self::Tool(CliTool::Hermes),
            Self::Tool(CliTool::Codex) => Self::All,
            Self::Tool(CliTool::Claude) => Self::Tool(CliTool::Codex),
            Self::Tool(CliTool::Hermes) => Self::Tool(CliTool::Claude),
        }
    }
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
    pub show_open_original: bool,
    pub show_diff: bool,
    pub session_filter: SessionFilter,
    pub search_query: String,
    pub verify_passed: bool,
    pub compile_status: &'static str,
    pub pending_g: bool,
    should_quit: bool,
}

impl App {
    pub fn new(source: CliTool, target: CliTool) -> Self {
        Self {
            data: demo::demo_data(source, target),
            focus: Focus::Sessions,
            selected_session: 0,
            selected_event: 6,
            selected_compiler: 0,
            command_mode: false,
            command_input: String::new(),
            show_help: false,
            show_launch: false,
            show_open_original: false,
            show_diff: false,
            session_filter: SessionFilter::All,
            search_query: String::new(),
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

        if self.show_launch {
            self.handle_launch_key(key);
            return;
        }

        match key.code {
            KeyCode::Esc => self.back_or_quit(),
            KeyCode::Char('q') => self.back_or_quit(),
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Char('[') => self.cycle_session_filter(false),
            KeyCode::Char(']') => self.cycle_session_filter(true),
            KeyCode::Char('f') => self.cycle_session_filter(true),
            KeyCode::Char('o') => self.show_open_original = true,
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
                if let Some(query) = command.strip_prefix('/') {
                    self.search_query = query.trim().to_string();
                    self.clamp_selected_session();
                    return;
                }
                match command.as_str() {
                    "q" | "quit" => self.should_quit = true,
                    "open" | "o" => self.show_open_original = true,
                    "compile" | "c" => self.compile_capsule(),
                    "verify" | "v" => self.verify_passed = true,
                    "help" | "?" => self.show_help = true,
                    "diff" | "d" => self.show_diff = !self.show_diff,
                    "filter" | "filter next" => self.cycle_session_filter(true),
                    "filter prev" | "filter previous" => self.cycle_session_filter(false),
                    "filter all" | "filter clear" => self.apply_session_filter(SessionFilter::All),
                    "filter codex" | "source codex" => {
                        self.apply_session_filter(SessionFilter::Tool(CliTool::Codex))
                    }
                    "filter claude" => {
                        self.apply_session_filter(SessionFilter::Tool(CliTool::Claude))
                    }
                    "filter hermes" => {
                        self.apply_session_filter(SessionFilter::Tool(CliTool::Hermes))
                    }
                    "source claude" => {
                        self.apply_session_filter(SessionFilter::Tool(CliTool::Claude))
                    }
                    "source hermes" => {
                        self.apply_session_filter(SessionFilter::Tool(CliTool::Hermes))
                    }
                    "source" | "source next" => self.cycle_session_filter(true),
                    "source prev" | "source previous" => self.cycle_session_filter(false),
                    "target" | "launch" => self.show_launch = true,
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

    fn handle_launch_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.show_launch = false,
            KeyCode::Enter => self.confirm_launch_target(),
            KeyCode::Char('j')
            | KeyCode::Char('l')
            | KeyCode::Down
            | KeyCode::Right
            | KeyCode::Char('}') => self.cycle_target(true),
            KeyCode::Char('k')
            | KeyCode::Char('h')
            | KeyCode::Up
            | KeyCode::Left
            | KeyCode::Char('{') => self.cycle_target(false),
            _ => {}
        }
    }

    fn back_or_quit(&mut self) {
        if self.show_diff {
            self.show_diff = false;
        } else if self.show_open_original {
            self.show_open_original = false;
        } else if self.show_launch {
            self.show_launch = false;
        } else if self.show_help {
            self.show_help = false;
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
            Focus::Sessions => self.move_session(true),
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
            Focus::Sessions => self.move_session(false),
            Focus::Timeline => self.selected_event = self.selected_event.saturating_sub(1),
            Focus::Capsule | Focus::Branches => {}
        }
        self.pending_g = false;
    }

    fn move_top(&mut self) {
        match self.focus {
            Focus::Sessions => {
                if let Some(first) = self.visible_session_indices().first() {
                    self.selected_session = *first;
                }
            }
            Focus::Timeline => self.selected_event = 0,
            Focus::Capsule | Focus::Branches => {}
        }
        self.pending_g = false;
    }

    fn move_bottom(&mut self) {
        match self.focus {
            Focus::Sessions => {
                if let Some(last) = self.visible_session_indices().last() {
                    self.selected_session = *last;
                }
            }
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

    pub fn current_session(&self) -> Option<&SessionSummary> {
        self.visible_session_indices()
            .contains(&self.selected_session)
            .then(|| self.data.sessions.get(self.selected_session))
            .flatten()
    }

    pub fn visible_session_indices(&self) -> Vec<usize> {
        let query = self.search_query.trim().to_ascii_lowercase();
        self.data
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, session)| self.session_filter.matches(session))
            .filter(|(_, session)| {
                query.is_empty()
                    || session.id.to_ascii_lowercase().contains(&query)
                    || session.title.to_ascii_lowercase().contains(&query)
                    || session.cwd.to_ascii_lowercase().contains(&query)
                    || session.cli.id().contains(&query)
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn move_session(&mut self, forward: bool) {
        let visible = self.visible_session_indices();
        if visible.is_empty() {
            return;
        }
        let current = visible
            .iter()
            .position(|index| *index == self.selected_session)
            .unwrap_or(0);
        let next = if forward {
            (current + 1).min(visible.len().saturating_sub(1))
        } else {
            current.saturating_sub(1)
        };
        self.selected_session = visible[next];
    }

    fn cycle_session_filter(&mut self, forward: bool) {
        let filter = if forward {
            self.session_filter.next()
        } else {
            self.session_filter.previous()
        };
        self.apply_session_filter(filter);
    }

    pub fn apply_session_filter(&mut self, filter: SessionFilter) {
        self.session_filter = filter;
        self.clamp_selected_session();
        self.pending_g = false;
    }

    fn clamp_selected_session(&mut self) {
        let visible = self.visible_session_indices();
        if visible.is_empty() {
            self.selected_session = 0;
        } else if !visible.contains(&self.selected_session) {
            self.selected_session = visible[0];
        }
    }

    fn cycle_target(&mut self, forward: bool) {
        let target = if forward {
            self.data.target.next()
        } else {
            self.data.target.previous()
        };
        self.replace_demo_data(self.active_source(), target);
    }

    fn confirm_launch_target(&mut self) {
        let _ = config::save_last_target(self.data.target);
        self.show_launch = false;
        self.pending_g = false;
    }

    fn active_source(&self) -> CliTool {
        self.current_session()
            .map(|session| session.cli)
            .unwrap_or(self.data.source)
    }

    fn replace_demo_data(&mut self, source: CliTool, target: CliTool) {
        let selected_compiler = self.selected_compiler;
        self.data = demo::demo_data(source, target);
        self.selected_session = self
            .selected_session
            .min(self.data.sessions.len().saturating_sub(1));
        self.clamp_selected_session();
        self.selected_event = self
            .selected_event
            .min(self.data.timeline.len().saturating_sub(1));
        self.selected_compiler = selected_compiler.min(self.data.compilers.len().saturating_sub(1));
        self.data.capsule.compiler = self.data.compilers[self.selected_compiler].clone();
        self.compile_status = "ACTIVE";
        self.verify_passed = true;
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

    #[test]
    fn session_filter_limits_visible_sessions() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key('f'));
        assert!(
            app.visible_session_indices()
                .iter()
                .all(|index| app.data.sessions[*index].cli == CliTool::Codex)
        );
    }

    #[test]
    fn current_session_respects_empty_filter_results() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes);
        app.search_query = "no-match".into();
        app.clamp_selected_session();

        assert!(app.visible_session_indices().is_empty());
        assert!(app.current_session().is_none());
    }

    #[test]
    fn source_filter_cycles_in_tui() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key(']'));
        assert_eq!(app.session_filter, SessionFilter::Tool(CliTool::Codex));

        app.handle_key(key(']'));
        assert_eq!(app.session_filter, SessionFilter::Tool(CliTool::Claude));
        assert!(
            app.visible_session_indices()
                .iter()
                .all(|index| app.data.sessions[*index].cli == CliTool::Claude)
        );

        app.handle_key(key('['));
        assert_eq!(app.session_filter, SessionFilter::Tool(CliTool::Codex));
    }

    #[test]
    fn target_cycles_inside_launch_picker() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(app.show_launch);

        app.handle_key(key('j'));
        assert_eq!(app.data.target, CliTool::Codex);
        assert_eq!(app.data.source, CliTool::Codex);
        assert_eq!(app.data.capsule.source_cli, CliTool::Codex);
        assert_eq!(app.data.capsule.source_session, "codex-cxcp-design");
        assert!(app.data.capsule.target_branch.contains("codex"));
        assert!(app.data.branches[2].label.contains("codex"));
    }
}
