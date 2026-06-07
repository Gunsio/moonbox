use std::{
    fmt,
    sync::mpsc::{self, TryRecvError},
    thread,
    time::Instant,
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::{
    config, doctor,
    error::CoreError,
    launcher,
    model::{
        CliTool, DoctorReport, LaunchPlan, LaunchValidation, LaunchValidationState,
        OriginalSessionPlan, SessionAction, SessionSummary, TimelineKind, VerificationReport,
        WorkCapsule, WorkbenchData,
    },
    verifier, workbench,
};

type SessionLoadResult = Result<WorkbenchData, CoreError>;

struct PendingSessionLoad {
    request_id: u64,
    session_id: String,
    target: CliTool,
    started_at: Instant,
    receiver: mpsc::Receiver<SessionLoadResult>,
}

impl fmt::Debug for PendingSessionLoad {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingSessionLoad")
            .field("request_id", &self.request_id)
            .field("session_id", &self.session_id)
            .field("target", &self.target)
            .finish_non_exhaustive()
    }
}

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

#[derive(Debug, Clone)]
pub enum TuiExitAction {
    OriginalResume(OriginalSessionPlan),
    TargetHandoff(LaunchPlan),
}

#[derive(Debug)]
pub struct App {
    pub data: WorkbenchData,
    pub focus: Focus,
    pub selected_session: usize,
    pub selected_event: usize,
    pub selected_compiler: usize,
    pub command_mode: bool,
    pub command_input: String,
    pub show_help: bool,
    pub show_launch: bool,
    pub launch_review: bool,
    pub show_open_original: bool,
    pub show_doctor: bool,
    pub show_diff: bool,
    pub session_filter: SessionFilter,
    pub search_query: String,
    visible_session_indices: Vec<usize>,
    pub pending_target: CliTool,
    pub status_message: String,
    pub rewind_event_id: String,
    pub capsule_scroll: u16,
    pub modal_scroll: u16,
    pub verify_passed: bool,
    pub doctor_report: DoctorReport,
    pub compile_status: &'static str,
    pub pending_g: bool,
    session_load_request_id: u64,
    pending_session_load: Option<PendingSessionLoad>,
    clipboard_text: Option<String>,
    exit_action: Option<TuiExitAction>,
    should_quit: bool,
}

impl App {
    pub fn new(source: CliTool, target: CliTool) -> Result<Self, CoreError> {
        let data = workbench::load_workbench(source, target)?;
        Ok(Self::from_data(data, target))
    }

    pub fn new_fixture(source: CliTool, target: CliTool) -> Result<Self, CoreError> {
        let data = workbench::load_fixture_workbench(source, target)?;
        Ok(Self::from_data(data, target))
    }

    fn from_data(data: WorkbenchData, target: CliTool) -> Self {
        let rewind_event_id = initial_rewind_event_id(&data);
        let selected_session = data
            .sessions
            .iter()
            .position(|session| session.id == data.capsule.source_session)
            .unwrap_or(0);
        let selected_event = rewind_event_index(&data, &rewind_event_id);
        let doctor_report = doctor::diagnose_with_inventory(&data.sessions, &data.source_adapters);
        let mut app = Self {
            data,
            focus: Focus::Sessions,
            selected_session,
            selected_event,
            selected_compiler: 0,
            command_mode: false,
            command_input: String::new(),
            show_help: false,
            show_launch: false,
            launch_review: false,
            show_open_original: false,
            show_doctor: false,
            show_diff: false,
            session_filter: SessionFilter::All,
            search_query: String::new(),
            pending_target: target,
            status_message: "Ready".into(),
            rewind_event_id,
            capsule_scroll: 0,
            modal_scroll: 0,
            verify_passed: true,
            doctor_report,
            compile_status: "ACTIVE",
            pending_g: false,
            session_load_request_id: 0,
            pending_session_load: None,
            clipboard_text: None,
            exit_action: None,
            should_quit: false,
            visible_session_indices: Vec::new(),
        };
        app.refresh_visible_sessions();
        app
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn take_clipboard_text(&mut self) -> Option<String> {
        self.clipboard_text.take()
    }

    pub fn take_exit_action(&mut self) -> Option<TuiExitAction> {
        self.exit_action.take()
    }

    pub fn is_session_load_pending(&self) -> bool {
        self.pending_session_load.is_some()
    }

    pub fn poll_background(&mut self) -> bool {
        let Some(pending) = self.pending_session_load.take() else {
            return false;
        };

        match pending.receiver.try_recv() {
            Ok(result) => {
                self.apply_session_load_result(pending, result);
                true
            }
            Err(TryRecvError::Empty) => {
                self.pending_session_load = Some(pending);
                false
            }
            Err(TryRecvError::Disconnected) => {
                self.compile_status = "FAILED";
                self.set_status(format!("Session load failed: {}", pending.session_id));
                true
            }
        }
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
            KeyCode::Esc => self.cancel_main_escape(),
            KeyCode::Char('q') => self.back_or_quit(),
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Char('[') => self.cycle_session_filter(false),
            KeyCode::Char(']') => self.cycle_session_filter(true),
            KeyCode::Char('f') => self.cycle_session_filter(true),
            KeyCode::Char('a') => self.clear_session_filters(),
            KeyCode::Char('o') => self.open_original(),
            KeyCode::Char('x') | KeyCode::Char('t') | KeyCode::Char('H') => {
                self.open_launch_picker()
            }
            KeyCode::Char('D') => self.open_doctor(),
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
            KeyCode::Enter => self.queue_original_resume(),
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
                    "doctor" | "diag" | "health" => self.open_doctor(),
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
                    "handoff" | "target" | "launch" | "x" => self.open_launch_picker(),
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
            self.refresh_visible_sessions();
            self.clamp_selected_session();
            self.request_selected_session_details();
        }
    }

    fn set_search_status(&mut self) {
        let suffix = if self.is_session_load_pending() {
            " - loading selected session"
        } else {
            ""
        };
        if self.search_query.is_empty() {
            self.set_status(format!("Search cleared{suffix}"));
        } else {
            self.set_status(format!("Search: /{}{suffix}", self.search_query));
        }
    }

    fn handle_launch_key(&mut self, key: KeyEvent) {
        if self.launch_review {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.show_launch = false;
                    self.launch_review = false;
                    self.modal_scroll = 0;
                    self.set_status("Launch review closed");
                }
                KeyCode::Char('y') => self.copy_launch_command(),
                KeyCode::Enter => self.queue_target_handoff(),
                KeyCode::PageDown => self.scroll_modal(true, 6),
                KeyCode::PageUp => self.scroll_modal(false, 6),
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.scroll_modal(true, 6)
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.scroll_modal(false, 6)
                }
                KeyCode::Char('j') | KeyCode::Down => self.scroll_modal(true, 1),
                KeyCode::Char('k') | KeyCode::Up => self.scroll_modal(false, 1),
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.show_launch = false;
                self.launch_review = false;
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
            KeyCode::Char('r') if self.show_doctor => self.refresh_doctor(),
            KeyCode::Char('y') if self.show_doctor => self.copy_doctor_report(),
            KeyCode::Char('y') => self.copy_focused_command(),
            KeyCode::Enter if self.show_open_original => self.queue_original_resume(),
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
        } else if self.show_doctor {
            self.show_doctor = false;
            self.modal_scroll = 0;
            self.set_status("Doctor closed");
        } else if self.show_open_original {
            self.show_open_original = false;
            self.modal_scroll = 0;
            self.set_status("Original preview closed");
        } else if self.show_launch {
            self.show_launch = false;
            self.launch_review = false;
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

    fn cancel_main_escape(&mut self) {
        self.pending_g = false;
        self.set_status("Press q or Ctrl-C to quit");
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
                self.selected_event = next_visible_timeline_event(
                    &self.data,
                    &self.rewind_event_id,
                    self.selected_event,
                );
            }
            Focus::Capsule => self.scroll_capsule(true, 1),
            Focus::Branches => {}
        }
        self.pending_g = false;
    }

    fn move_up(&mut self) {
        match self.focus {
            Focus::Sessions => self.move_session(false),
            Focus::Timeline => {
                self.selected_event = previous_visible_timeline_event(
                    &self.data,
                    &self.rewind_event_id,
                    self.selected_event,
                );
            }
            Focus::Capsule => self.scroll_capsule(false, 1),
            Focus::Branches => {}
        }
        self.pending_g = false;
    }

    fn move_top(&mut self) {
        match self.focus {
            Focus::Sessions => {
                if let Some(first) = self.visible_session_indices.first().copied() {
                    self.selected_session = first;
                    self.request_selected_session_details();
                }
            }
            Focus::Timeline => {
                self.selected_event =
                    first_visible_timeline_event(&self.data, &self.rewind_event_id)
            }
            Focus::Capsule => self.capsule_scroll = 0,
            Focus::Branches => {}
        }
        self.pending_g = false;
    }

    fn move_bottom(&mut self) {
        match self.focus {
            Focus::Sessions => {
                if let Some(last) = self.visible_session_indices.last().copied() {
                    self.selected_session = last;
                    self.request_selected_session_details();
                }
            }
            Focus::Timeline => {
                self.selected_event = last_visible_timeline_event(&self.data, &self.rewind_event_id)
            }
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
        if !self.ensure_session_details_ready("Rewind") {
            return;
        }
        self.selected_event =
            nearest_visible_timeline_event(&self.data, &self.rewind_event_id, self.selected_event);
        if let Some((id, title)) = self
            .data
            .timeline
            .get(self.selected_event)
            .and_then(|event| {
                timeline_event_is_rewind_anchor(event)
                    .then(|| (event.id.clone(), event.title.clone()))
            })
        {
            self.apply_rewind_event(id.clone(), title);
            self.set_status(format!("Rewind set: {id}"));
        } else {
            self.set_status("Rewind anchor must be a User turn");
        }
        self.pending_g = false;
    }

    fn compile_capsule(&mut self) {
        if !self.ensure_session_details_ready("Compile") {
            return;
        }
        let compiler = self.data.compilers[self.selected_compiler].clone();
        let Some(session_id) = self.current_session().map(|session| session.id.clone()) else {
            self.compile_status = "FAILED";
            self.set_status("Compile failed: no session selected");
            self.pending_g = false;
            return;
        };
        match workbench::compile_capsule(
            &session_id,
            self.data.target,
            &self.rewind_event_id,
            &compiler,
        ) {
            Ok(Some(capsule)) => {
                self.compile_status = "COMPILED";
                self.data.capsule = capsule;
                self.set_status(format!("Capsule compiled: {compiler}"));
            }
            Ok(None) => {
                self.compile_status = "FAILED";
                self.set_status("Compile failed: session not found");
            }
            Err(error) => {
                self.compile_status = "FAILED";
                self.set_status(format!("Compile failed: {error}"));
            }
        }
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
        if !self.ensure_session_details_ready("Verify") {
            return;
        }
        let session_id = self.current_session().map(|session| session.id.clone());
        let report = match workbench::verify_launch(session_id.as_deref(), self.data.target, None) {
            Ok(Some(report)) => report,
            Ok(None) => {
                self.verify_passed = false;
                self.set_status("Verify: FAIL No session selected");
                return;
            }
            Err(error) => {
                self.verify_passed = false;
                self.set_status(format!("Verify: FAIL {error}"));
                return;
            }
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

    fn open_doctor(&mut self) {
        self.refresh_doctor();
        self.show_doctor = true;
        self.modal_scroll = 0;
        self.pending_g = false;
    }

    fn refresh_doctor(&mut self) {
        self.doctor_report = doctor::diagnose();
        self.set_status(format!(
            "Doctor: {} ({} checks)",
            self.doctor_report.status,
            self.doctor_report.checks.len()
        ));
    }

    fn open_original(&mut self) {
        if !self.ensure_session_details_ready("Original") {
            return;
        }
        self.show_open_original = true;
        self.modal_scroll = 0;
        if let Some(session) = self.current_session() {
            self.set_status(format!("Original ready: {} {}", session.cli, session.id));
        } else {
            self.set_status("No session selected");
        }
        self.pending_g = false;
    }

    fn queue_original_resume(&mut self) {
        if !self.ensure_session_details_ready("Original") {
            return;
        }
        let Some(session) = self.current_session().cloned() else {
            self.set_status("No session selected");
            return;
        };
        let command = launcher::original_command(&session);
        self.exit_action = Some(TuiExitAction::OriginalResume(OriginalSessionPlan {
            version: 1,
            action: SessionAction::OriginalResume,
            dry_run: true,
            source_session: session.clone(),
            command,
        }));
        self.should_quit = true;
        self.set_status(format!("Opening original: {} {}", session.cli, session.id));
    }

    pub fn current_session(&self) -> Option<&SessionSummary> {
        self.visible_session_indices
            .contains(&self.selected_session)
            .then(|| self.data.sessions.get(self.selected_session))
            .flatten()
    }

    pub fn visible_session_indices(&self) -> &[usize] {
        &self.visible_session_indices
    }

    fn refresh_visible_sessions(&mut self) {
        let query = self.search_query.trim().to_ascii_lowercase();
        self.visible_session_indices = self
            .data
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, session)| self.session_filter.matches(session))
            .filter(|(_, session)| session_matches_query(session, &query))
            .map(|(index, _)| index)
            .collect();
    }

    fn move_session(&mut self, forward: bool) {
        if self.visible_session_indices.is_empty() {
            return;
        }
        let current = self
            .visible_session_indices
            .iter()
            .position(|index| *index == self.selected_session)
            .unwrap_or(0);
        let next = if forward {
            (current + 1).min(self.visible_session_indices.len().saturating_sub(1))
        } else {
            current.saturating_sub(1)
        };
        self.selected_session = self.visible_session_indices[next];
        self.request_selected_session_details();
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
        self.refresh_visible_sessions();
        self.clamp_selected_session();
        self.request_selected_session_details();
        if self.is_session_load_pending() {
            self.set_status(format!(
                "Filter: {} - loading selected session",
                self.session_filter.label()
            ));
        } else {
            self.set_status(format!("Filter: {}", self.session_filter.label()));
        }
        self.pending_g = false;
    }

    fn clear_session_filters(&mut self) {
        self.session_filter = SessionFilter::All;
        self.search_query.clear();
        self.refresh_visible_sessions();
        self.clamp_selected_session();
        self.request_selected_session_details();
        if self.is_session_load_pending() {
            self.set_status("Filters cleared - loading selected session");
        } else {
            self.set_status("Filters cleared");
        }
        self.pending_g = false;
    }

    fn clamp_selected_session(&mut self) {
        if self.visible_session_indices.is_empty() {
            self.selected_session = 0;
        } else if !self
            .visible_session_indices
            .contains(&self.selected_session)
        {
            self.selected_session = self.visible_session_indices[0];
        }
    }

    fn has_overlay(&self) -> bool {
        self.show_help || self.show_open_original || self.show_doctor || self.show_diff
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
        if !self.ensure_session_details_ready("Launch") {
            return;
        }
        if self.current_session().is_none() {
            self.show_launch = false;
            self.set_status("No session selected");
            self.pending_g = false;
            return;
        }
        self.pending_target = self.data.target;
        self.show_launch = true;
        self.launch_review = false;
        self.modal_scroll = 0;
        self.set_status("Choose target CLI");
        self.pending_g = false;
    }

    fn ensure_session_details_ready(&mut self, action: &str) -> bool {
        if self.is_session_load_pending() {
            self.set_status(format!("{action} waits for selected session to load"));
            self.pending_g = false;
            return false;
        }
        true
    }

    fn confirm_launch_target(&mut self) {
        let target = self.pending_target;
        let validation = self.validate_launch_for_target(target);
        if validation.is_blocked() {
            self.set_status(format!("Target blocked: {}", validation.summary()));
            self.pending_g = false;
            return;
        }
        if let Err(error) = self.replace_data_for_target(target) {
            self.set_status(format!("Target failed: {error}"));
            self.pending_g = false;
            return;
        }
        let _ = config::save_last_target(target);
        self.show_launch = true;
        self.launch_review = true;
        self.modal_scroll = 0;
        if validation.state == LaunchValidationState::Warning {
            self.set_status(format!(
                "Review launch: {target} ({})",
                validation.summary()
            ));
        } else {
            self.set_status(format!("Review launch: {target}"));
        }
        self.pending_g = false;
    }

    fn replace_data_for_target(&mut self, target: CliTool) -> Result<(), CoreError> {
        let selected_compiler = self.selected_compiler;
        let rewind_event_id = self.rewind_event_id.clone();
        let session_id = self.current_session().map(|session| session.id.clone());
        if let Some(session_id) = session_id {
            if let Some(data) = workbench::load_workbench_for_session(&session_id, target)? {
                self.data = data;
                self.refresh_visible_sessions();
            }
        } else {
            self.data = workbench::load_workbench(self.data.source, target)?;
            self.refresh_visible_sessions();
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
        Ok(())
    }

    fn request_selected_session_details(&mut self) {
        let Some(session) = self.data.sessions.get(self.selected_session).cloned() else {
            self.pending_session_load = None;
            return;
        };
        let target = self.data.target;
        if self.data.capsule.source_session == session.id
            && self.data.target == target
            && self.pending_session_load.is_none()
        {
            return;
        }

        self.session_load_request_id = self.session_load_request_id.wrapping_add(1);
        let request_id = self.session_load_request_id;
        let selected_session = self.selected_session;
        let selected_compiler = self.selected_compiler;
        let sessions = self.data.sessions.clone();
        let source_adapters = self.data.source_adapters.clone();
        let worker_session = session.clone();
        let worker_session_id = session.id.clone();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = workbench::load_workbench_from_session_snapshot(
                worker_session,
                sessions,
                source_adapters,
                target,
            );
            let _ = sender.send(result);
        });

        self.pending_session_load = Some(PendingSessionLoad {
            request_id,
            session_id: worker_session_id,
            target,
            started_at: Instant::now(),
            receiver,
        });
        self.selected_session = selected_session.min(self.data.sessions.len().saturating_sub(1));
        self.selected_compiler = selected_compiler.min(self.data.compilers.len().saturating_sub(1));
        self.compile_status = "LOADING";
        self.verify_passed = false;
        self.set_status(format!(
            "Loading session: {} {}",
            session.cli, session.title
        ));
    }

    fn apply_session_load_result(
        &mut self,
        pending: PendingSessionLoad,
        result: SessionLoadResult,
    ) {
        if self.session_load_request_id != pending.request_id
            || self
                .data
                .sessions
                .get(self.selected_session)
                .is_none_or(|session| session.id != pending.session_id)
            || self.data.target != pending.target
        {
            return;
        }

        let elapsed = pending.started_at.elapsed();
        let selected_compiler = self.selected_compiler;
        match result {
            Ok(data) => {
                let rewind_event_id = initial_rewind_event_id(&data);
                let selected_event = rewind_event_index(&data, &rewind_event_id);
                self.data = data;
                self.refresh_visible_sessions();
                self.selected_session = self
                    .data
                    .sessions
                    .iter()
                    .position(|session| session.id == pending.session_id)
                    .unwrap_or(self.selected_session)
                    .min(self.data.sessions.len().saturating_sub(1));
                self.selected_event = selected_event;
                self.selected_compiler =
                    selected_compiler.min(self.data.compilers.len().saturating_sub(1));
                self.data.capsule.compiler = self.data.compilers[self.selected_compiler].clone();
                self.rewind_event_id = rewind_event_id;
                self.capsule_scroll = 0;
                self.compile_status = "ACTIVE";
                self.verify_passed = true;
                if let Some(session) = self.current_session() {
                    self.set_status(format!(
                        "Session: {} {} ({} events, {} ms)",
                        session.cli,
                        session.title,
                        self.data.timeline.len(),
                        elapsed.as_millis()
                    ));
                } else {
                    self.set_status(format!("Session loaded ({} ms)", elapsed.as_millis()));
                }
            }
            Err(error) => {
                self.compile_status = "FAILED";
                self.set_status(format!(
                    "Session reload failed: {error} ({} ms)",
                    elapsed.as_millis()
                ));
            }
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
        if !self.launch_review {
            self.set_status("Confirm target first with enter");
            return;
        }
        self.copy_text("launch", self.launch_command());
    }

    fn copy_original_command(&mut self) {
        if let Some(command) = self.original_open_command() {
            self.copy_text("original", command);
        } else {
            self.set_status("No session selected");
        }
    }

    fn queue_target_handoff(&mut self) {
        let validation = self.validate_launch_for_target(self.pending_target);
        if validation.is_blocked() {
            self.set_status(format!("Target blocked: {}", validation.summary()));
            return;
        }
        let Some(session) = self.current_session().cloned() else {
            self.set_status("No session selected");
            return;
        };
        let capsule = self.launch_capsule_for_target(self.pending_target);
        let target_command = match launcher::target_command(self.pending_target, &session, &capsule)
        {
            Ok(command) => command,
            Err(error) => {
                self.set_status(format!("Target failed: {error}"));
                return;
            }
        };
        let verification =
            verifier::verify_capsule(&capsule, &session, &self.data.timeline, self.pending_target);
        let command = target_command.display.clone();
        let target_branch = capsule.target_branch;
        self.exit_action = Some(TuiExitAction::TargetHandoff(LaunchPlan {
            version: 1,
            action: SessionAction::TargetHandoff,
            dry_run: true,
            source_session: session,
            target_cli: self.pending_target,
            target_branch,
            capsule_path: None,
            command,
            target_command,
            verification,
        }));
        self.should_quit = true;
        self.set_status(format!("Launching target: {}", self.pending_target));
    }

    fn copy_doctor_report(&mut self) {
        match serde_json::to_string_pretty(&self.doctor_report) {
            Ok(report) => {
                self.clipboard_text = Some(report);
                self.set_status("Copied doctor report");
            }
            Err(error) => self.set_status(format!("Doctor copy failed: {error}")),
        }
    }

    pub fn launch_command(&self) -> String {
        let session = self
            .current_session()
            .map(|session| session.id.as_str())
            .unwrap_or("no-session");
        workbench::moonbox_execute_command(self.pending_target, session, None)
    }

    pub fn launch_branch(&self) -> String {
        format!(
            "moonbox/{}-rewind-{}",
            self.pending_target.id(),
            self.rewind_event_id
        )
    }

    pub fn original_open_command(&self) -> Option<String> {
        self.current_session()
            .map(|session| workbench::moonbox_open_execute_command(&session.id))
    }

    pub fn original_resume_display_command(&self) -> Option<String> {
        self.current_session()
            .map(|session| launcher::original_command(session).display)
    }

    pub fn validate_launch_for_target(&self, target: CliTool) -> LaunchValidation {
        let Some(report) = self.launch_verification_for_target(target) else {
            return LaunchValidation::blocked(vec!["No session selected".into()]);
        };
        verifier::validation_from_report(&report)
    }

    pub fn launch_verification_for_target(&self, target: CliTool) -> Option<VerificationReport> {
        let session = self.current_session()?;
        let capsule = self.launch_capsule_for_target(target);
        Some(verifier::verify_capsule(
            &capsule,
            session,
            &self.data.timeline,
            target,
        ))
    }

    fn launch_capsule_for_target(&self, target: CliTool) -> WorkCapsule {
        let mut capsule = self.data.capsule.clone();
        capsule.target_cli = target;
        capsule.target_branch = format!("moonbox/{}-rewind-{}", target.id(), self.rewind_event_id);
        capsule
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

fn initial_rewind_event_id(data: &WorkbenchData) -> String {
    data.capsule
        .rewind_point
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string()
}

fn rewind_event_index(data: &WorkbenchData, rewind_event_id: &str) -> usize {
    data.timeline
        .iter()
        .position(|event| event.id == rewind_event_id)
        .unwrap_or_else(|| data.timeline.len().saturating_sub(1))
}

fn timeline_event_is_visible(data: &WorkbenchData, rewind_event_id: &str, index: usize) -> bool {
    data.timeline
        .get(index)
        .is_some_and(|event| event.id == rewind_event_id || event.kind != TimelineKind::Tool)
}

fn timeline_event_is_rewind_anchor(event: &crate::core::model::TimelineEvent) -> bool {
    matches!(event.kind, TimelineKind::User | TimelineKind::RewindPoint)
}

fn first_visible_timeline_event(data: &WorkbenchData, rewind_event_id: &str) -> usize {
    (0..data.timeline.len())
        .find(|index| timeline_event_is_visible(data, rewind_event_id, *index))
        .unwrap_or(0)
}

fn last_visible_timeline_event(data: &WorkbenchData, rewind_event_id: &str) -> usize {
    (0..data.timeline.len())
        .rev()
        .find(|index| timeline_event_is_visible(data, rewind_event_id, *index))
        .unwrap_or_else(|| data.timeline.len().saturating_sub(1))
}

fn nearest_visible_timeline_event(
    data: &WorkbenchData,
    rewind_event_id: &str,
    selected_event: usize,
) -> usize {
    if timeline_event_is_visible(data, rewind_event_id, selected_event) {
        return selected_event;
    }
    (selected_event.saturating_add(1)..data.timeline.len())
        .find(|index| timeline_event_is_visible(data, rewind_event_id, *index))
        .or_else(|| {
            (0..selected_event)
                .rev()
                .find(|index| timeline_event_is_visible(data, rewind_event_id, *index))
        })
        .unwrap_or_else(|| selected_event.min(data.timeline.len().saturating_sub(1)))
}

fn next_visible_timeline_event(
    data: &WorkbenchData,
    rewind_event_id: &str,
    selected_event: usize,
) -> usize {
    (selected_event.saturating_add(1)..data.timeline.len())
        .find(|index| timeline_event_is_visible(data, rewind_event_id, *index))
        .unwrap_or_else(|| nearest_visible_timeline_event(data, rewind_event_id, selected_event))
}

fn previous_visible_timeline_event(
    data: &WorkbenchData,
    rewind_event_id: &str,
    selected_event: usize,
) -> usize {
    (0..selected_event)
        .rev()
        .find(|index| timeline_event_is_visible(data, rewind_event_id, *index))
        .unwrap_or_else(|| nearest_visible_timeline_event(data, rewind_event_id, selected_event))
}

fn session_matches_query(session: &SessionSummary, query: &str) -> bool {
    query.is_empty()
        || session.id.to_ascii_lowercase().contains(query)
        || session.title.to_ascii_lowercase().contains(query)
        || session.cwd.to_ascii_lowercase().contains(query)
        || session.cli.id().contains(query)
        || session
            .branch
            .as_ref()
            .is_some_and(|branch| branch.to_ascii_lowercase().contains(query))
        || session
            .health_reason
            .as_ref()
            .is_some_and(|reason| reason.to_ascii_lowercase().contains(query))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::empty())
    }

    fn new_app(source: CliTool, target: CliTool) -> App {
        App::new(source, target).expect("app")
    }

    fn settle_session_load(app: &mut App) {
        for _ in 0..100 {
            app.poll_background();
            if !app.is_session_load_pending() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        app.poll_background();
        assert!(
            !app.is_session_load_pending(),
            "session load did not finish"
        );
    }

    #[test]
    fn space_updates_rewind_point_from_selected_event() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.selected_event = 0;
        app.handle_key(key(' '));
        assert_eq!(app.rewind_event_id, "evt-001");
        assert!(app.data.capsule.rewind_point.contains("evt-001"));
        assert!(app.data.capsule.target_branch.contains("evt-001"));
    }

    #[test]
    fn timeline_navigation_skips_hidden_tool_events() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.focus = Focus::Timeline;
        app.selected_event = 0;

        app.handle_key(key('j'));
        assert_eq!(app.selected_event, 2);

        app.handle_key(key('k'));
        assert_eq!(app.selected_event, 0);
    }

    #[test]
    fn rewind_selection_from_non_user_event_is_rejected() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        let original_rewind = app.rewind_event_id.clone();
        app.selected_event = 2;

        app.handle_key(key(' '));

        assert_eq!(app.rewind_event_id, original_rewind);
        assert_eq!(app.status_message, "Rewind anchor must be a User turn");
    }

    #[test]
    fn compiler_cycles() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        let first = app.data.capsule.compiler.clone();
        app.handle_key(key('s'));
        assert_ne!(app.data.capsule.compiler, first);
    }

    #[test]
    fn compile_key_runs_selected_compiler() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key('s'));
        let compiler = app.data.capsule.compiler.clone();

        app.handle_key(key('c'));

        assert_eq!(app.compile_status, "COMPILED");
        assert_eq!(app.data.capsule.compiler, compiler);
        assert!(app.status_message.contains("Capsule compiled"));
    }

    #[test]
    fn new_selects_requested_source_session() {
        let app = new_app(CliTool::Hermes, CliTool::Codex);

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
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key('f'));
        assert!(
            app.visible_session_indices()
                .iter()
                .all(|index| app.data.sessions[*index].cli == CliTool::Codex)
        );
    }

    #[test]
    fn current_session_respects_empty_filter_results() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.search_query = "no-match".into();
        app.refresh_visible_sessions();
        app.clamp_selected_session();

        assert!(app.visible_session_indices().is_empty());
        assert!(app.current_session().is_none());
    }

    #[test]
    fn slash_search_filters_while_typing() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('/'));
        app.handle_key(key('5'));

        assert!(app.command_mode);
        assert_eq!(app.search_query, "5");
        assert_eq!(app.visible_session_indices().len(), 1);
        assert_eq!(
            app.current_session().map(|session| session.id.as_str()),
            Some("hermes-cxcp-502")
        );
        assert!(app.is_session_load_pending());
        assert_eq!(app.rewind_event_id, "evt-091");
        settle_session_load(&mut app);
        assert_eq!(app.data.source, CliTool::Hermes);
        assert_eq!(app.data.capsule.source_session, "hermes-cxcp-502");
        assert_eq!(app.rewind_event_id, "evt-052");
    }

    #[test]
    fn slash_search_escape_keeps_filter_result() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('/'));
        app.handle_key(key('5'));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));

        assert!(!app.command_mode);
        assert_eq!(app.search_query, "5");
        assert_eq!(app.visible_session_indices().len(), 1);
    }

    #[test]
    fn main_escape_does_not_quit() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));

        assert!(!app.should_quit());
        assert_eq!(app.status_message, "Press q or Ctrl-C to quit");
    }

    #[test]
    fn q_and_ctrl_c_quit_from_main_screen() {
        let mut q_app = new_app(CliTool::Codex, CliTool::Hermes);
        q_app.handle_key(key('q'));
        assert!(q_app.should_quit());

        let mut ctrl_c_app = new_app(CliTool::Codex, CliTool::Hermes);
        ctrl_c_app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(ctrl_c_app.should_quit());
    }

    #[test]
    fn clear_filter_resets_source_and_search() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.apply_session_filter(SessionFilter::Tool(CliTool::Hermes));
        app.handle_key(key('/'));
        app.handle_key(key('5'));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        app.handle_key(key('a'));

        assert_eq!(app.session_filter, SessionFilter::All);
        assert!(app.search_query.is_empty());
        assert_eq!(app.visible_session_indices().len(), 3);
        assert_eq!(
            app.status_message,
            "Filters cleared - loading selected session"
        );
        settle_session_load(&mut app);
    }

    #[test]
    fn source_filter_cycles_in_tui() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key(']'));
        assert_eq!(app.session_filter, SessionFilter::Tool(CliTool::Codex));

        app.handle_key(key(']'));
        assert_eq!(app.session_filter, SessionFilter::Tool(CliTool::Claude));
        assert!(app.is_session_load_pending());
        assert_eq!(app.rewind_event_id, "evt-091");
        settle_session_load(&mut app);
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
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('j'));

        assert_eq!(
            app.current_session().map(|session| session.id.as_str()),
            Some("claude-qc-platform")
        );
        assert!(app.is_session_load_pending());
        assert_eq!(app.data.source, CliTool::Codex);
        assert_eq!(app.data.capsule.source_session, "codex-cxcp-design");
        assert_eq!(app.rewind_event_id, "evt-091");
        assert_eq!(app.compile_status, "LOADING");
        settle_session_load(&mut app);
        assert_eq!(app.data.source, CliTool::Claude);
        assert_eq!(app.data.capsule.source_cli, CliTool::Claude);
        assert_eq!(app.data.capsule.source_session, "claude-qc-platform");
        assert_eq!(app.rewind_event_id, "evt-074");
        assert_eq!(app.selected_event, 4);
        assert!(app.data.timeline[0].detail.contains("QC platform"));
        assert!(app.data.branches[1].label.contains("evt-074"));
        assert!(
            app.status_message
                .starts_with("Session: Claude QC platform trace repair (5 events, ")
        );
    }

    #[test]
    fn rapid_session_moves_ignore_stale_background_loads() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('j'));
        app.handle_key(key('k'));

        assert!(app.is_session_load_pending());
        assert_eq!(
            app.current_session().map(|session| session.id.as_str()),
            Some("codex-cxcp-design")
        );
        settle_session_load(&mut app);
        assert_eq!(app.data.source, CliTool::Codex);
        assert_eq!(app.data.capsule.source_session, "codex-cxcp-design");
        assert_eq!(app.rewind_event_id, "evt-091");
    }

    #[test]
    fn launch_waits_for_selected_session_details() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('j'));
        app.handle_key(key('H'));

        assert!(app.is_session_load_pending());
        assert!(!app.show_launch);
        assert_eq!(
            app.status_message,
            "Launch waits for selected session to load"
        );
        settle_session_load(&mut app);
        app.handle_key(key('H'));
        assert!(app.show_launch);
        assert_eq!(app.status_message, "Choose target CLI");
    }

    #[test]
    fn target_cycles_inside_launch_picker() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key('t'));
        assert!(app.show_launch);
        assert_eq!(app.status_message, "Choose target CLI");

        app.handle_key(key('j'));
        assert_eq!(app.pending_target, CliTool::Codex);
        assert_eq!(app.data.target, CliTool::Hermes);
        assert_eq!(app.status_message, "Target: Codex");

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(app.show_launch);
        assert!(app.launch_review);
        assert_eq!(app.data.target, CliTool::Codex);
        assert!(app.status_message.starts_with("Review launch: Codex"));
        assert_eq!(app.data.source, CliTool::Codex);
        assert_eq!(app.data.capsule.source_cli, CliTool::Codex);
        assert_eq!(app.data.capsule.source_session, "codex-cxcp-design");
        assert!(app.data.capsule.target_branch.contains("codex"));
        assert!(app.data.branches[2].label.contains("codex"));
    }

    #[test]
    fn uppercase_h_remains_launch_picker_compatibility_alias() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('H'));

        assert!(app.show_launch);
        assert_eq!(app.status_message, "Choose target CLI");
    }

    #[test]
    fn target_change_preserves_selected_rewind_point() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.selected_event = 0;
        app.handle_key(key(' '));
        app.handle_key(key('H'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert_eq!(app.rewind_event_id, "evt-001");
        assert!(app.data.capsule.rewind_point.contains("evt-001"));
        assert!(app.data.capsule.target_branch.contains("evt-001"));
    }

    #[test]
    fn launch_picker_cancel_discards_pending_target() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key('H'));
        app.handle_key(key('j'));
        app.handle_key(key('q'));

        assert!(!app.show_launch);
        assert_eq!(app.pending_target, CliTool::Codex);
        assert_eq!(app.data.target, CliTool::Hermes);
        assert_eq!(app.status_message, "Launch cancelled");
    }

    #[test]
    fn launch_validation_warns_for_same_cli_handoff() {
        let app = new_app(CliTool::Codex, CliTool::Codex);

        let validation = app.validate_launch_for_target(CliTool::Codex);
        let report = app
            .launch_verification_for_target(CliTool::Codex)
            .expect("launch verification");

        assert_eq!(validation.state, LaunchValidationState::Warning);
        assert!(validation.summary().contains("Same-CLI handoff"));
        assert!(report.checks.iter().any(|check| {
            check.name == "target_support" && check.detail.contains("Same-CLI handoff")
        }));
    }

    #[test]
    fn target_picker_blocks_failed_same_cli_resume_path() {
        let mut app = new_app(CliTool::Hermes, CliTool::Hermes);

        app.handle_key(key('H'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(app.show_launch);
        assert!(!app.launch_review);
        assert_eq!(app.data.target, CliTool::Hermes);
        assert!(app.status_message.starts_with("Target blocked:"));
        assert!(app.status_message.contains("raw resume is known failed"));
    }

    #[test]
    fn blocked_target_cannot_copy_launch_command() {
        let mut app = new_app(CliTool::Hermes, CliTool::Hermes);

        app.handle_key(key('H'));
        app.handle_key(key('y'));

        assert!(app.take_clipboard_text().is_none());
        assert!(app.status_message.starts_with("Target blocked:"));
    }

    #[test]
    fn launch_picker_requires_visible_session() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.search_query = "no-match".into();
        app.refresh_visible_sessions();
        app.clamp_selected_session();

        app.handle_key(key('H'));

        assert!(!app.show_launch);
        assert_eq!(app.status_message, "No session selected");
    }

    #[test]
    fn verify_toggle_reports_status() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('v'));

        assert!(app.verify_passed);
        assert!(app.status_message.starts_with("Verify: PASS ("));
        assert!(app.status_message.ends_with(" checks)"));
    }

    #[test]
    fn launch_copy_queues_clipboard_text() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key('H'));

        app.handle_key(key('y'));
        assert!(app.take_clipboard_text().is_none());
        assert_eq!(app.status_message, "Confirm target first with enter");

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(app.launch_review);
        app.handle_key(key('y'));

        let copied = app.take_clipboard_text().expect("clipboard text");
        assert!(copied.starts_with("moonbox launch --execute --target"));
        assert!(copied.contains("--session codex-cxcp-design"));
        assert!(!copied.contains("--capsule"));
        assert_eq!(app.status_message, "Copied launch command");
    }

    #[test]
    fn launch_review_enter_queues_target_handoff_without_executing_in_tests() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key('H'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(app.should_quit());
        let Some(TuiExitAction::TargetHandoff(plan)) = app.take_exit_action() else {
            panic!("expected target handoff action");
        };
        assert_eq!(plan.source_session.id, "codex-cxcp-design");
        assert_eq!(plan.target_cli, CliTool::Hermes);
        assert!(plan.dry_run);
    }

    #[test]
    fn x_shortcut_opens_target_handoff_picker() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('x'));

        assert!(app.show_launch);
        assert!(!app.launch_review);
        assert_eq!(app.status_message, "Choose target CLI");
    }

    #[test]
    fn main_enter_queues_original_resume_without_opening_handoff_picker() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(app.should_quit());
        assert!(!app.show_launch);
        let Some(TuiExitAction::OriginalResume(plan)) = app.take_exit_action() else {
            panic!("expected original resume action");
        };
        assert_eq!(plan.source_session.id, "codex-cxcp-design");
        assert_eq!(plan.command.display, "codex resume codex-cxcp-design");
    }

    #[test]
    fn original_copy_and_enter_queue_distinct_actions() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key('o'));
        app.handle_key(key('y'));

        assert_eq!(
            app.take_clipboard_text().as_deref(),
            Some("moonbox open --execute --session codex-cxcp-design")
        );
        assert_eq!(app.status_message, "Copied original command");

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(app.should_quit());
        let Some(TuiExitAction::OriginalResume(plan)) = app.take_exit_action() else {
            panic!("expected original resume action");
        };
        assert_eq!(plan.source_session.id, "codex-cxcp-design");
        assert_eq!(plan.command.display, "codex resume codex-cxcp-design");
        assert!(plan.dry_run);
    }

    #[test]
    fn overlay_navigation_scrolls_modal_without_moving_timeline() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.focus = Focus::Timeline;
        app.selected_event = 3;
        app.handle_key(key('?'));
        app.handle_key(key('j'));

        assert_eq!(app.selected_event, 3);
        assert_eq!(app.modal_scroll, 1);
    }

    #[test]
    fn doctor_overlay_reports_and_copies_json_without_moving_selection() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.focus = Focus::Timeline;
        app.selected_event = 3;

        app.handle_key(key('D'));

        assert!(app.show_doctor);
        assert_eq!(app.selected_event, 3);
        assert!(app.status_message.starts_with("Doctor: "));
        assert!(
            app.doctor_report
                .checks
                .iter()
                .any(|check| check.name == "session_discovery")
        );

        app.handle_key(key('y'));
        let copied = app.take_clipboard_text().expect("doctor json");
        let json: serde_json::Value = serde_json::from_str(&copied).expect("valid json");
        assert_eq!(json["version"], 1);
        assert!(json["checks"].as_array().is_some_and(|checks| {
            checks
                .iter()
                .any(|check| check["name"] == "session_discovery")
        }));
        assert_eq!(app.status_message, "Copied doctor report");
    }

    #[test]
    fn command_mode_opens_doctor_overlay() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key(':'));
        for ch in "doctor".chars() {
            app.handle_key(key(ch));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(app.show_doctor);
        assert!(!app.command_mode);
        assert!(app.status_message.starts_with("Doctor: "));
    }

    #[test]
    fn capsule_panel_scrolls_with_vim_navigation() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.focus = Focus::Capsule;

        app.handle_key(key('j'));
        app.handle_key(key('j'));
        app.handle_key(key('k'));

        assert_eq!(app.capsule_scroll, 1);
    }
}
