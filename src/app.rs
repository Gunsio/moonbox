use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::{
    config,
    model::{CliTool, DemoData, SessionStatus, SessionSummary},
    workbench,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchValidationState {
    Ready,
    Warning,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchValidation {
    pub state: LaunchValidationState,
    pub reasons: Vec<String>,
}

impl LaunchValidation {
    pub fn ready() -> Self {
        Self {
            state: LaunchValidationState::Ready,
            reasons: vec!["Ready".into()],
        }
    }

    pub fn summary(&self) -> String {
        self.reasons.join("; ")
    }

    fn warning(reasons: Vec<String>) -> Self {
        Self {
            state: LaunchValidationState::Warning,
            reasons,
        }
    }

    fn blocked(reasons: Vec<String>) -> Self {
        Self {
            state: LaunchValidationState::Blocked,
            reasons,
        }
    }

    fn is_blocked(&self) -> bool {
        self.state == LaunchValidationState::Blocked
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
    pub pending_target: CliTool,
    pub status_message: String,
    pub rewind_event_id: String,
    pub capsule_scroll: u16,
    pub modal_scroll: u16,
    pub verify_passed: bool,
    pub compile_status: &'static str,
    pub pending_g: bool,
    clipboard_text: Option<String>,
    should_quit: bool,
}

impl App {
    pub fn new(source: CliTool, target: CliTool) -> Self {
        let data = workbench::load_demo_workbench(source, target);
        let rewind_event_id = initial_rewind_event_id(&data);
        let selected_session = data
            .sessions
            .iter()
            .position(|session| session.id == data.capsule.source_session)
            .unwrap_or(0);
        let selected_event = rewind_event_index(&data, &rewind_event_id);
        Self {
            data,
            focus: Focus::Sessions,
            selected_session,
            selected_event,
            selected_compiler: 0,
            command_mode: false,
            command_input: String::new(),
            show_help: false,
            show_launch: false,
            show_open_original: false,
            show_diff: false,
            session_filter: SessionFilter::All,
            search_query: String::new(),
            pending_target: target,
            status_message: "Ready".into(),
            rewind_event_id,
            capsule_scroll: 0,
            modal_scroll: 0,
            verify_passed: true,
            compile_status: "ACTIVE",
            pending_g: false,
            clipboard_text: None,
            should_quit: false,
        }
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn take_clipboard_text(&mut self) -> Option<String> {
        self.clipboard_text.take()
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
        if self.has_overlay() {
            self.handle_overlay_key(key);
            return;
        }

        match key.code {
            KeyCode::Esc => self.back_or_quit(),
            KeyCode::Char('q') => self.back_or_quit(),
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Char('[') => self.cycle_session_filter(false),
            KeyCode::Char(']') => self.cycle_session_filter(true),
            KeyCode::Char('f') => self.cycle_session_filter(true),
            KeyCode::Char('a') => self.clear_session_filters(),
            KeyCode::Char('o') => self.open_original(),
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
                self.command_input = format!("/{}", self.search_query);
            }
            KeyCode::Char(' ') => self.set_rewind_point(),
            KeyCode::Char('c') => self.compile_capsule(),
            KeyCode::Char('v') => self.toggle_verify(),
            KeyCode::Char('d') => self.toggle_diff(),
            KeyCode::Char('s') => self.cycle_compiler(),
            KeyCode::Char('y') => self.copy_focused_command(),
            KeyCode::Enter => self.open_launch_picker(),
            _ => self.pending_g = false,
        }
    }

    fn handle_command_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                let was_search = self.is_search_command();
                self.command_mode = false;
                self.command_input.clear();
                if was_search {
                    self.set_search_status();
                } else {
                    self.set_status("Command cancelled");
                }
            }
            KeyCode::Enter => {
                if self.is_search_command() {
                    self.sync_live_search();
                    self.command_mode = false;
                    self.command_input.clear();
                    self.set_search_status();
                    return;
                }

                let command = self.command_input.trim().to_ascii_lowercase();
                self.command_mode = false;
                self.command_input.clear();
                match command.as_str() {
                    "q" | "quit" => self.should_quit = true,
                    "" => self.set_status("Command cancelled"),
                    "open" | "o" => self.open_original(),
                    "compile" | "c" => self.compile_capsule(),
                    "verify" | "v" => self.mark_verify_passed(),
                    "help" | "?" => self.open_help(),
                    "diff" | "d" => self.toggle_diff(),
                    "filter" | "filter next" => self.cycle_session_filter(true),
                    "filter prev" | "filter previous" => self.cycle_session_filter(false),
                    "filter all" | "filter clear" | "clear" | "all" => self.clear_session_filters(),
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
                    "target" | "launch" => self.open_launch_picker(),
                    _ => self.set_status(format!("Unknown command: {command}")),
                }
            }
            KeyCode::Backspace => {
                if self.is_search_command() {
                    if self.command_input.len() > 1 {
                        self.command_input.pop();
                    }
                    self.sync_live_search();
                } else {
                    self.command_input.pop();
                }
            }
            KeyCode::Char(ch) => {
                self.command_input.push(ch);
                if self.is_search_command() {
                    self.sync_live_search();
                }
            }
            _ => {}
        }
    }

    fn is_search_command(&self) -> bool {
        self.command_input.starts_with('/')
    }

    fn sync_live_search(&mut self) {
        if let Some(query) = self.command_input.strip_prefix('/') {
            self.search_query = query.trim().to_string();
            self.clamp_selected_session();
            self.sync_selected_session_details();
        }
    }

    fn set_search_status(&mut self) {
        if self.search_query.is_empty() {
            self.set_status("Search cleared");
        } else {
            self.set_status(format!("Search: /{}", self.search_query));
        }
    }

    fn handle_launch_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.show_launch = false;
                self.modal_scroll = 0;
                self.set_status("Launch cancelled");
            }
            KeyCode::Enter => self.confirm_launch_target(),
            KeyCode::Char('y') => self.copy_launch_command(),
            KeyCode::PageDown => self.scroll_modal(true, 6),
            KeyCode::PageUp => self.scroll_modal(false, 6),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_modal(true, 6)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_modal(false, 6)
            }
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

    fn handle_overlay_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.back_or_quit(),
            KeyCode::Char('y') => self.copy_focused_command(),
            KeyCode::Char('j') | KeyCode::Down => self.scroll_modal(true, 1),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_modal(false, 1),
            KeyCode::PageDown => self.scroll_modal(true, 6),
            KeyCode::PageUp => self.scroll_modal(false, 6),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_modal(true, 6)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_modal(false, 6)
            }
            _ => {}
        }
    }

    fn back_or_quit(&mut self) {
        if self.show_diff {
            self.show_diff = false;
            self.modal_scroll = 0;
            self.set_status("Diff closed");
        } else if self.show_open_original {
            self.show_open_original = false;
            self.modal_scroll = 0;
            self.set_status("Original preview closed");
        } else if self.show_launch {
            self.show_launch = false;
            self.modal_scroll = 0;
            self.set_status("Launch cancelled");
        } else if self.show_help {
            self.show_help = false;
            self.modal_scroll = 0;
            self.set_status("Help closed");
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
            Focus::Capsule => self.scroll_capsule(true, 1),
            Focus::Branches => {}
        }
        self.pending_g = false;
    }

    fn move_up(&mut self) {
        match self.focus {
            Focus::Sessions => self.move_session(false),
            Focus::Timeline => self.selected_event = self.selected_event.saturating_sub(1),
            Focus::Capsule => self.scroll_capsule(false, 1),
            Focus::Branches => {}
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
            Focus::Capsule => self.capsule_scroll = 0,
            Focus::Branches => {}
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
            Focus::Capsule => self.scroll_capsule(true, 999),
            Focus::Branches => {}
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
        if let Some((id, title)) = self
            .data
            .timeline
            .get(self.selected_event)
            .map(|event| (event.id.clone(), event.title.clone()))
        {
            self.apply_rewind_event(id.clone(), title);
            self.set_status(format!("Rewind set: {id}"));
        } else {
            self.set_status("No rewind point available");
        }
        self.pending_g = false;
    }

    fn compile_capsule(&mut self) {
        let compiler = self.data.compilers[self.selected_compiler].clone();
        self.compile_status = "COMPILED";
        self.data.capsule.compiler = compiler.clone();
        self.set_status(format!("Capsule compiled: {compiler}"));
        self.pending_g = false;
    }

    fn cycle_compiler(&mut self) {
        self.selected_compiler = (self.selected_compiler + 1) % self.data.compilers.len();
        self.data.capsule.compiler = self.data.compilers[self.selected_compiler].clone();
        self.set_status(format!("Skill: {}", self.data.capsule.compiler));
        self.pending_g = false;
    }

    fn toggle_verify(&mut self) {
        self.run_verification();
        self.pending_g = false;
    }

    fn mark_verify_passed(&mut self) {
        self.run_verification();
        self.pending_g = false;
    }

    fn run_verification(&mut self) {
        let session_id = self.current_session().map(|session| session.id.clone());
        let Some(report) = workbench::verify_launch(session_id.as_deref(), self.data.target) else {
            self.verify_passed = false;
            self.set_status("Verify: FAIL No session selected");
            return;
        };
        self.verify_passed = report.ready;
        self.set_status(format!(
            "Verify: {} ({} checks)",
            report.status,
            report.checks.len()
        ));
    }

    fn toggle_diff(&mut self) {
        self.show_diff = !self.show_diff;
        self.modal_scroll = 0;
        if self.show_diff {
            self.set_status("Diff opened");
        } else {
            self.set_status("Diff closed");
        }
        self.pending_g = false;
    }

    fn open_help(&mut self) {
        self.show_help = true;
        self.modal_scroll = 0;
        self.set_status("Help opened");
        self.pending_g = false;
    }

    fn open_original(&mut self) {
        self.show_open_original = true;
        self.modal_scroll = 0;
        if let Some(session) = self.current_session() {
            self.set_status(format!("Original ready: {} {}", session.cli, session.id));
        } else {
            self.set_status("No session selected");
        }
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
                    || session
                        .branch
                        .as_ref()
                        .is_some_and(|branch| branch.to_ascii_lowercase().contains(&query))
                    || session
                        .health_reason
                        .as_ref()
                        .is_some_and(|reason| reason.to_ascii_lowercase().contains(&query))
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
        self.sync_selected_session_details();
        if let Some(session) = self.current_session() {
            self.set_status(format!("Session: {} {}", session.cli, session.title));
        }
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
        self.sync_selected_session_details();
        self.set_status(format!("Filter: {}", self.session_filter.label()));
        self.pending_g = false;
    }

    fn clear_session_filters(&mut self) {
        self.session_filter = SessionFilter::All;
        self.search_query.clear();
        self.clamp_selected_session();
        self.sync_selected_session_details();
        self.set_status("Filters cleared");
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

    fn has_overlay(&self) -> bool {
        self.show_help || self.show_open_original || self.show_diff
    }

    fn cycle_target(&mut self, forward: bool) {
        self.pending_target = if forward {
            self.pending_target.next()
        } else {
            self.pending_target.previous()
        };
        self.set_status(format!("Target: {}", self.pending_target));
    }

    fn open_launch_picker(&mut self) {
        if self.current_session().is_none() {
            self.show_launch = false;
            self.set_status("No session selected");
            self.pending_g = false;
            return;
        }
        self.pending_target = self.data.target;
        self.show_launch = true;
        self.modal_scroll = 0;
        self.set_status("Choose target CLI");
        self.pending_g = false;
    }

    fn confirm_launch_target(&mut self) {
        let target = self.pending_target;
        let validation = self.validate_launch_for_target(target);
        if validation.is_blocked() {
            self.set_status(format!("Target blocked: {}", validation.summary()));
            self.pending_g = false;
            return;
        }
        self.replace_data_for_target(target);
        let _ = config::save_last_target(target);
        self.show_launch = false;
        self.modal_scroll = 0;
        if validation.state == LaunchValidationState::Warning {
            self.set_status(format!("Target saved: {target} ({})", validation.summary()));
        } else {
            self.set_status(format!("Target saved: {target}"));
        }
        self.pending_g = false;
    }

    fn replace_data_for_target(&mut self, target: CliTool) {
        let selected_compiler = self.selected_compiler;
        let rewind_event_id = self.rewind_event_id.clone();
        let session_id = self.current_session().map(|session| session.id.clone());
        if let Some(session_id) = session_id {
            if let Some(data) = workbench::load_demo_workbench_for_session(&session_id, target) {
                self.data = data;
            }
        } else {
            self.data = workbench::load_demo_workbench(self.data.source, target);
        }
        self.selected_session = self
            .selected_session
            .min(self.data.sessions.len().saturating_sub(1));
        self.clamp_selected_session();
        self.selected_event = self
            .selected_event
            .min(self.data.timeline.len().saturating_sub(1));
        self.selected_compiler = selected_compiler.min(self.data.compilers.len().saturating_sub(1));
        self.data.capsule.compiler = self.data.compilers[self.selected_compiler].clone();
        if let Some(title) = self.timeline_event_title(&rewind_event_id) {
            self.apply_rewind_event(rewind_event_id, title);
        } else {
            self.rewind_event_id = initial_rewind_event_id(&self.data);
        }
        self.compile_status = "ACTIVE";
        self.verify_passed = true;
        self.pending_g = false;
    }

    fn sync_selected_session_details(&mut self) {
        let Some(session_id) = self
            .data
            .sessions
            .get(self.selected_session)
            .map(|session| session.id.clone())
        else {
            return;
        };
        let target = self.data.target;
        let selected_session = self.selected_session;
        let selected_compiler = self.selected_compiler;
        if let Some(data) = workbench::load_demo_workbench_for_session(&session_id, target) {
            let rewind_event_id = initial_rewind_event_id(&data);
            let selected_event = rewind_event_index(&data, &rewind_event_id);
            self.data = data;
            self.selected_session =
                selected_session.min(self.data.sessions.len().saturating_sub(1));
            self.selected_event = selected_event;
            self.selected_compiler =
                selected_compiler.min(self.data.compilers.len().saturating_sub(1));
            self.data.capsule.compiler = self.data.compilers[self.selected_compiler].clone();
            self.rewind_event_id = rewind_event_id;
            self.capsule_scroll = 0;
        }
    }

    fn set_status(&mut self, message: impl Into<String>) {
        self.status_message = message.into();
    }

    fn scroll_capsule(&mut self, forward: bool, amount: u16) {
        self.capsule_scroll = if forward {
            self.capsule_scroll.saturating_add(amount)
        } else {
            self.capsule_scroll.saturating_sub(amount)
        };
    }

    fn scroll_modal(&mut self, forward: bool, amount: u16) {
        self.modal_scroll = if forward {
            self.modal_scroll.saturating_add(amount)
        } else {
            self.modal_scroll.saturating_sub(amount)
        };
    }

    fn copy_text(&mut self, label: &str, text: String) {
        self.clipboard_text = Some(text);
        self.set_status(format!("Copied {label} command"));
    }

    fn copy_focused_command(&mut self) {
        if self.show_launch {
            self.copy_launch_command();
        } else if self.show_open_original {
            self.copy_original_command();
        } else {
            self.set_status("No command to copy");
        }
        self.pending_g = false;
    }

    fn copy_launch_command(&mut self) {
        let validation = self.validate_launch_for_target(self.pending_target);
        if validation.is_blocked() {
            self.set_status(format!("Target blocked: {}", validation.summary()));
            return;
        }
        self.copy_text("launch", self.launch_command());
    }

    fn copy_original_command(&mut self) {
        if let Some(command) = self.original_resume_command() {
            self.copy_text("original", command);
        } else {
            self.set_status("No session selected");
        }
    }

    pub fn launch_command(&self) -> String {
        let session = self
            .current_session()
            .map(|session| session.id.as_str())
            .unwrap_or("no-session");
        format!(
            "moonbox launch --target {} --session {} --capsule {}",
            self.pending_target.id(),
            session,
            self.capsule_path()
        )
    }

    pub fn launch_branch(&self) -> String {
        format!(
            "moonbox/{}-rewind-{}",
            self.pending_target.id(),
            self.rewind_event_id
        )
    }

    pub fn capsule_path(&self) -> String {
        format!("~/.moonbox/capsules/{}.json", self.rewind_event_id)
    }

    pub fn original_resume_command(&self) -> Option<String> {
        self.current_session()
            .map(|session| session.resume_command.clone())
    }

    pub fn validate_launch_for_target(&self, target: CliTool) -> LaunchValidation {
        let Some(session) = self.current_session() else {
            return LaunchValidation::blocked(vec!["No session selected".into()]);
        };

        let mut blockers = Vec::new();
        let mut warnings = Vec::new();

        if self.data.capsule.source_session != session.id {
            blockers.push(format!(
                "Capsule source {} does not match selected session {}",
                self.data.capsule.source_session, session.id
            ));
        }

        if !self
            .data
            .timeline
            .iter()
            .any(|event| event.id == self.rewind_event_id)
        {
            blockers.push(format!(
                "Rewind {} is not present in the selected timeline",
                self.rewind_event_id
            ));
        }

        if self.data.capsule.target_cli != target {
            warnings.push(format!(
                "Confirm will refresh capsule target from {} to {}",
                self.data.capsule.target_cli, target
            ));
        }

        if target == session.cli {
            warnings.push("Same-CLI handoff; use o for original resume".into());
        }

        if session.status == SessionStatus::Failed {
            let reason = session
                .health_reason
                .as_deref()
                .unwrap_or("source health is failed");
            warnings.push(format!("Source health: {reason}"));
        }

        if session.status == SessionStatus::Failed && target == session.cli {
            blockers.push(format!(
                "{} raw resume is known failed for this session",
                target
            ));
        }

        if !blockers.is_empty() {
            LaunchValidation::blocked(blockers)
        } else if !warnings.is_empty() {
            LaunchValidation::warning(warnings)
        } else {
            LaunchValidation::ready()
        }
    }

    fn apply_rewind_event(&mut self, id: String, title: String) {
        self.rewind_event_id = id.clone();
        self.data.capsule.rewind_point = format!("{id} / {title}");
        self.data.capsule.target_branch = format!("moonbox/{}-rewind-{id}", self.data.target.id());
    }

    fn timeline_event_title(&self, id: &str) -> Option<String> {
        self.data
            .timeline
            .iter()
            .find(|event| event.id == id)
            .map(|event| event.title.clone())
    }
}

fn initial_rewind_event_id(data: &DemoData) -> String {
    data.capsule
        .rewind_point
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string()
}

fn rewind_event_index(data: &DemoData, rewind_event_id: &str) -> usize {
    data.timeline
        .iter()
        .position(|event| event.id == rewind_event_id)
        .unwrap_or_else(|| data.timeline.len().saturating_sub(1))
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
        assert_eq!(app.rewind_event_id, "evt-001");
        assert!(app.data.capsule.rewind_point.contains("evt-001"));
        assert!(app.data.capsule.target_branch.contains("evt-001"));
    }

    #[test]
    fn compiler_cycles() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes);
        let first = app.data.capsule.compiler.clone();
        app.handle_key(key('s'));
        assert_ne!(app.data.capsule.compiler, first);
    }

    #[test]
    fn new_selects_requested_source_session() {
        let app = App::new(CliTool::Hermes, CliTool::Codex);

        assert_eq!(
            app.current_session().map(|session| session.id.as_str()),
            Some("hermes-cxcp-502")
        );
        assert_eq!(app.data.source, CliTool::Hermes);
        assert_eq!(app.data.target, CliTool::Codex);
        assert_eq!(app.rewind_event_id, "evt-052");
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
    fn slash_search_filters_while_typing() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('/'));
        app.handle_key(key('5'));

        assert!(app.command_mode);
        assert_eq!(app.search_query, "5");
        assert_eq!(app.visible_session_indices().len(), 1);
        assert_eq!(
            app.current_session().map(|session| session.id.as_str()),
            Some("hermes-cxcp-502")
        );
        assert_eq!(app.data.source, CliTool::Hermes);
        assert_eq!(app.data.capsule.source_session, "hermes-cxcp-502");
        assert_eq!(app.rewind_event_id, "evt-052");
    }

    #[test]
    fn slash_search_escape_keeps_filter_result() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('/'));
        app.handle_key(key('5'));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));

        assert!(!app.command_mode);
        assert_eq!(app.search_query, "5");
        assert_eq!(app.visible_session_indices().len(), 1);
    }

    #[test]
    fn clear_filter_resets_source_and_search() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes);

        app.apply_session_filter(SessionFilter::Tool(CliTool::Hermes));
        app.handle_key(key('/'));
        app.handle_key(key('5'));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        app.handle_key(key('a'));

        assert_eq!(app.session_filter, SessionFilter::All);
        assert!(app.search_query.is_empty());
        assert_eq!(app.visible_session_indices().len(), 3);
        assert_eq!(app.status_message, "Filters cleared");
    }

    #[test]
    fn source_filter_cycles_in_tui() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key(']'));
        assert_eq!(app.session_filter, SessionFilter::Tool(CliTool::Codex));

        app.handle_key(key(']'));
        assert_eq!(app.session_filter, SessionFilter::Tool(CliTool::Claude));
        assert_eq!(app.data.source, CliTool::Claude);
        assert_eq!(app.data.capsule.source_session, "claude-qc-platform");
        assert_eq!(app.rewind_event_id, "evt-074");
        assert!(
            app.visible_session_indices()
                .iter()
                .all(|index| app.data.sessions[*index].cli == CliTool::Claude)
        );

        app.handle_key(key('['));
        assert_eq!(app.session_filter, SessionFilter::Tool(CliTool::Codex));
    }

    #[test]
    fn moving_session_reloads_timeline_capsule_and_rewind() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('j'));

        assert_eq!(
            app.current_session().map(|session| session.id.as_str()),
            Some("claude-qc-platform")
        );
        assert_eq!(app.data.source, CliTool::Claude);
        assert_eq!(app.data.capsule.source_cli, CliTool::Claude);
        assert_eq!(app.data.capsule.source_session, "claude-qc-platform");
        assert_eq!(app.rewind_event_id, "evt-074");
        assert_eq!(app.selected_event, 4);
        assert!(app.data.timeline[0].detail.contains("QC platform"));
        assert!(app.data.branches[1].label.contains("evt-074"));
        assert_eq!(
            app.status_message,
            "Session: Claude QC platform trace repair"
        );
    }

    #[test]
    fn target_cycles_inside_launch_picker() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(app.show_launch);
        assert_eq!(app.status_message, "Choose target CLI");

        app.handle_key(key('j'));
        assert_eq!(app.pending_target, CliTool::Codex);
        assert_eq!(app.data.target, CliTool::Hermes);
        assert_eq!(app.status_message, "Target: Codex");

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(!app.show_launch);
        assert_eq!(app.data.target, CliTool::Codex);
        assert!(app.status_message.starts_with("Target saved: Codex"));
        assert_eq!(app.data.source, CliTool::Codex);
        assert_eq!(app.data.capsule.source_cli, CliTool::Codex);
        assert_eq!(app.data.capsule.source_session, "codex-cxcp-design");
        assert!(app.data.capsule.target_branch.contains("codex"));
        assert!(app.data.branches[2].label.contains("codex"));
    }

    #[test]
    fn target_change_preserves_selected_rewind_point() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes);
        app.selected_event = 0;
        app.handle_key(key(' '));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert_eq!(app.rewind_event_id, "evt-001");
        assert!(app.data.capsule.rewind_point.contains("evt-001"));
        assert!(app.data.capsule.target_branch.contains("evt-001"));
    }

    #[test]
    fn launch_picker_cancel_discards_pending_target() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        app.handle_key(key('j'));
        app.handle_key(key('q'));

        assert!(!app.show_launch);
        assert_eq!(app.pending_target, CliTool::Codex);
        assert_eq!(app.data.target, CliTool::Hermes);
        assert_eq!(app.status_message, "Launch cancelled");
    }

    #[test]
    fn launch_validation_warns_for_same_cli_handoff() {
        let app = App::new(CliTool::Codex, CliTool::Codex);

        let validation = app.validate_launch_for_target(CliTool::Codex);

        assert_eq!(validation.state, LaunchValidationState::Warning);
        assert!(validation.summary().contains("Same-CLI handoff"));
    }

    #[test]
    fn target_picker_blocks_failed_same_cli_resume_path() {
        let mut app = App::new(CliTool::Hermes, CliTool::Hermes);

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(app.show_launch);
        assert_eq!(app.data.target, CliTool::Hermes);
        assert!(app.status_message.starts_with("Target blocked:"));
        assert!(app.status_message.contains("raw resume is known failed"));
    }

    #[test]
    fn blocked_target_cannot_copy_launch_command() {
        let mut app = App::new(CliTool::Hermes, CliTool::Hermes);

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        app.handle_key(key('y'));

        assert!(app.take_clipboard_text().is_none());
        assert!(app.status_message.starts_with("Target blocked:"));
    }

    #[test]
    fn launch_picker_requires_visible_session() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes);
        app.search_query = "no-match".into();
        app.clamp_selected_session();

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(!app.show_launch);
        assert_eq!(app.status_message, "No session selected");
    }

    #[test]
    fn verify_toggle_reports_status() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('v'));

        assert!(app.verify_passed);
        assert_eq!(app.status_message, "Verify: PASS (6 checks)");
    }

    #[test]
    fn launch_copy_queues_clipboard_text() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        app.handle_key(key('y'));

        let copied = app.take_clipboard_text().expect("clipboard text");
        assert!(copied.starts_with("moonbox launch --target"));
        assert!(copied.contains("--session codex-cxcp-design"));
        assert!(copied.contains("evt-091.json"));
        assert_eq!(app.status_message, "Copied launch command");
    }

    #[test]
    fn original_copy_queues_resume_command() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key('o'));
        app.handle_key(key('y'));

        assert_eq!(
            app.take_clipboard_text().as_deref(),
            Some("codex resume codex-cxcp-design")
        );
        assert_eq!(app.status_message, "Copied original command");
    }

    #[test]
    fn overlay_navigation_scrolls_modal_without_moving_timeline() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes);
        app.focus = Focus::Timeline;
        app.selected_event = 3;
        app.handle_key(key('?'));
        app.handle_key(key('j'));

        assert_eq!(app.selected_event, 3);
        assert_eq!(app.modal_scroll, 1);
    }

    #[test]
    fn capsule_panel_scrolls_with_vim_navigation() {
        let mut app = App::new(CliTool::Codex, CliTool::Hermes);
        app.focus = Focus::Capsule;

        app.handle_key(key('j'));
        app.handle_key(key('j'));
        app.handle_key(key('k'));

        assert_eq!(app.capsule_scroll, 1);
    }
}
