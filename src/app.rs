use std::{
    fmt,
    sync::mpsc::{self, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::{
    compiler, config, continuation, dataspace, doctor,
    error::CoreError,
    launcher,
    model::{
        CliTool, ContinuationOptions, DoctorReport, LaunchPlan, LaunchValidation,
        LaunchValidationState, OriginalSessionPlan, SessionAction, SessionSummary, TimelineKind,
        VerificationReport, WorkCapsule, WorkbenchData,
    },
    verifier, workbench,
};

type SessionLoadResult = Result<WorkbenchData, CoreError>;
type DataSpaceLoadResult = Result<WorkbenchData, CoreError>;

pub const HANDOFF_TRAIL_DURATION_MS: u64 = 720;
const HANDOFF_TRAIL_FRAME_COUNT: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoffTrailPhase {
    Review,
}

impl HandoffTrailPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Review => "Review",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandoffTrailFrame {
    pub phase: HandoffTrailPhase,
    pub step: usize,
    pub elapsed_ms: u64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Copy)]
struct HandoffTrail {
    phase: HandoffTrailPhase,
    started_at: Instant,
}

struct PendingSessionLoad {
    request_id: u64,
    session_id: String,
    target: CliTool,
    started_at: Instant,
    receiver: mpsc::Receiver<SessionLoadResult>,
}

struct PendingDataSpaceLoad {
    request_id: u64,
    index: usize,
    space: dataspace::DataSpaceEntry,
    started_at: Instant,
    receiver: mpsc::Receiver<DataSpaceLoadResult>,
}

impl fmt::Debug for PendingDataSpaceLoad {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingDataSpaceLoad")
            .field("request_id", &self.request_id)
            .field("index", &self.index)
            .field("space", &self.space.label)
            .finish_non_exhaustive()
    }
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

impl Focus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sessions => "Sessions",
            Self::Timeline => "Timeline",
            Self::Capsule => "Details",
            Self::Branches => "Action Path",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionFilter {
    Starred,
    All,
    Tool(CliTool),
}

impl SessionFilter {
    pub fn label(self) -> &'static str {
        match self {
            Self::Starred => "Star",
            Self::All => "All",
            Self::Tool(CliTool::Codex) => "Codex",
            Self::Tool(CliTool::Claude) => "Claude",
            Self::Tool(CliTool::Hermes) => "Hermes",
        }
    }

    fn matches(self, session: &SessionSummary, starred_sessions: &[String]) -> bool {
        match self {
            Self::Starred => starred_sessions
                .iter()
                .any(|key| key == &session_star_key(session)),
            Self::All => true,
            Self::Tool(tool) => session.cli == tool,
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Starred => Self::All,
            Self::All => Self::Tool(CliTool::Codex),
            Self::Tool(CliTool::Codex) => Self::Tool(CliTool::Claude),
            Self::Tool(CliTool::Claude) => Self::Tool(CliTool::Hermes),
            Self::Tool(CliTool::Hermes) => Self::Starred,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Starred => Self::Tool(CliTool::Hermes),
            Self::All => Self::Starred,
            Self::Tool(CliTool::Codex) => Self::All,
            Self::Tool(CliTool::Claude) => Self::Tool(CliTool::Codex),
            Self::Tool(CliTool::Hermes) => Self::Tool(CliTool::Claude),
        }
    }
}

#[derive(Debug, Clone)]
pub enum TuiExitAction {
    OriginalResume(Box<OriginalSessionPlan>),
    TargetHandoff(Box<LaunchPlan>),
}

#[derive(Debug)]
pub struct App {
    pub data: WorkbenchData,
    pub focus: Focus,
    pub zoomed_focus: Option<Focus>,
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
    pub show_skill_picker: bool,
    pub session_filter: SessionFilter,
    pub starred_sessions: Vec<String>,
    pub search_query: String,
    visible_session_indices: Vec<usize>,
    pub data_spaces: Vec<dataspace::DataSpaceEntry>,
    pub selected_data_space: usize,
    pub pending_target: CliTool,
    pub pending_compiler: usize,
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
    data_space_load_request_id: u64,
    pending_data_space_load: Option<PendingDataSpaceLoad>,
    handoff_trail: Option<HandoffTrail>,
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
            zoomed_focus: None,
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
            show_skill_picker: false,
            session_filter: SessionFilter::All,
            starred_sessions: config::load_starred_sessions(),
            search_query: String::new(),
            data_spaces: dataspace::list_data_spaces(),
            selected_data_space: 0,
            pending_target: target,
            pending_compiler: 0,
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
            data_space_load_request_id: 0,
            pending_data_space_load: None,
            handoff_trail: None,
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

    pub fn current_data_space(&self) -> &dataspace::DataSpaceEntry {
        self.data_spaces
            .get(self.selected_data_space)
            .unwrap_or_else(|| &self.data_spaces[0])
    }

    pub fn poll_background(&mut self) -> bool {
        let mut changed = self.prune_handoff_trail();

        if let Some(pending) = self.pending_session_load.take() {
            match pending.receiver.try_recv() {
                Ok(result) => {
                    self.apply_session_load_result(pending, result);
                    changed = true;
                }
                Err(TryRecvError::Empty) => {
                    self.pending_session_load = Some(pending);
                }
                Err(TryRecvError::Disconnected) => {
                    self.compile_status = "FAILED";
                    self.set_status(format!("Session load failed: {}", pending.session_id));
                    changed = true;
                }
            }
        }

        self.poll_data_space_background() || changed
    }

    fn prune_handoff_trail(&mut self) -> bool {
        if self.handoff_trail_frame().is_some() {
            return false;
        }
        self.handoff_trail.take().is_some()
    }

    pub fn handoff_trail_frame(&self) -> Option<HandoffTrailFrame> {
        let trail = self.handoff_trail?;
        let elapsed = trail.started_at.elapsed();
        let duration = Duration::from_millis(HANDOFF_TRAIL_DURATION_MS);
        let elapsed_ms = elapsed.as_millis();
        if elapsed >= duration {
            return None;
        }
        let step = ((elapsed_ms * HANDOFF_TRAIL_FRAME_COUNT as u128)
            / u128::from(HANDOFF_TRAIL_DURATION_MS))
        .min((HANDOFF_TRAIL_FRAME_COUNT - 1) as u128) as usize;
        Some(HandoffTrailFrame {
            phase: trail.phase,
            step,
            elapsed_ms: elapsed_ms as u64,
            duration_ms: HANDOFF_TRAIL_DURATION_MS,
        })
    }

    pub(crate) fn start_handoff_trail_for_review(&mut self) {
        self.start_handoff_trail(HandoffTrailPhase::Review);
    }

    fn start_handoff_trail(&mut self, phase: HandoffTrailPhase) {
        self.handoff_trail = Some(HandoffTrail {
            phase,
            started_at: Instant::now(),
        });
    }

    fn clear_handoff_trail(&mut self) {
        self.handoff_trail = None;
    }

    #[cfg(test)]
    fn set_handoff_trail_elapsed_for_test(&mut self, elapsed: Duration) {
        self.handoff_trail = Some(HandoffTrail {
            phase: HandoffTrailPhase::Review,
            started_at: Instant::now() - elapsed,
        });
    }

    fn poll_data_space_background(&mut self) -> bool {
        let Some(pending) = self.pending_data_space_load.take() else {
            return false;
        };

        match pending.receiver.try_recv() {
            Ok(result) => {
                self.apply_data_space_load_result(pending, result);
                true
            }
            Err(TryRecvError::Empty) => {
                self.pending_data_space_load = Some(pending);
                false
            }
            Err(TryRecvError::Disconnected) => {
                self.compile_status = "FAILED";
                self.set_status("Data space load failed: worker disconnected");
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
            KeyCode::Char('{') => self.cycle_data_space(false),
            KeyCode::Char('}') => self.cycle_data_space(true),
            KeyCode::Char('f') => self.cycle_session_filter(true),
            KeyCode::Char('a') => self.clear_session_filters(),
            KeyCode::Char('s') | KeyCode::Char('*') => self.toggle_starred_session(),
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
            KeyCode::Char('c') => self.review_capsule(),
            KeyCode::Char('v') => self.toggle_verify(),
            KeyCode::Char('S') => self.open_skill_picker(),
            KeyCode::Char('+') | KeyCode::Char('=') => self.zoom_current_panel(),
            KeyCode::Char('-') => self.restore_zoom(),
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
                    "review" | "compile" | "c" => self.review_capsule(),
                    "verify" | "v" => self.mark_verify_passed(),
                    "help" | "?" => self.open_help(),
                    "doctor" | "diag" | "health" => self.open_doctor(),
                    "filter" | "filter next" => self.cycle_session_filter(true),
                    "filter prev" | "filter previous" => self.cycle_session_filter(false),
                    "filter star" | "filter starred" | "starred" => {
                        self.apply_session_filter(SessionFilter::Starred)
                    }
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
                    "star" | "s" | "*" => self.toggle_starred_session(),
                    "skill" | "compiler" => self.open_skill_picker(),
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
                    self.clear_handoff_trail();
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
                self.clear_handoff_trail();
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
        if self.show_skill_picker {
            self.handle_skill_picker_key(key);
            return;
        }
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
        if self.show_skill_picker {
            self.show_skill_picker = false;
            self.modal_scroll = 0;
            self.pending_compiler = self.selected_compiler;
            self.set_status("Skill picker closed");
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
            self.clear_handoff_trail();
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
        if self.zoomed_focus.is_some() {
            self.zoomed_focus = Some(self.focus);
        }
        self.pending_g = false;
    }

    fn prev_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Sessions => Focus::Branches,
            Focus::Timeline => Focus::Sessions,
            Focus::Capsule => Focus::Timeline,
            Focus::Branches => Focus::Capsule,
        };
        if self.zoomed_focus.is_some() {
            self.zoomed_focus = Some(self.focus);
        }
        self.pending_g = false;
    }

    fn zoom_current_panel(&mut self) {
        self.zoomed_focus = Some(self.focus);
        self.set_status(format!("Zoomed {}", self.focus.label()));
        self.pending_g = false;
    }

    fn restore_zoom(&mut self) {
        if self.zoomed_focus.take().is_some() {
            self.set_status("Zoom restored");
        } else {
            self.set_status("No panel zoom active");
        }
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

    fn open_skill_picker(&mut self) {
        self.pending_compiler = self
            .selected_compiler
            .min(self.data.compilers.len().saturating_sub(1));
        self.show_skill_picker = true;
        self.modal_scroll = 0;
        self.set_status("Choose compiler skill");
        self.pending_g = false;
    }

    fn handle_skill_picker_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.back_or_quit(),
            KeyCode::Enter => self.confirm_skill_picker(),
            KeyCode::Char('j') | KeyCode::Char('l') | KeyCode::Down | KeyCode::Right => {
                self.move_skill_picker(true)
            }
            KeyCode::Char('k') | KeyCode::Char('h') | KeyCode::Up | KeyCode::Left => {
                self.move_skill_picker(false)
            }
            KeyCode::Char('y') => self.copy_pending_skill_reference(),
            _ => {}
        }
    }

    fn move_skill_picker(&mut self, forward: bool) {
        if self.data.compilers.is_empty() {
            self.pending_compiler = 0;
            self.set_status("No compiler skills configured");
            return;
        }
        if forward {
            self.pending_compiler = (self.pending_compiler + 1) % self.data.compilers.len();
        } else {
            self.pending_compiler = if self.pending_compiler == 0 {
                self.data.compilers.len() - 1
            } else {
                self.pending_compiler - 1
            };
        }
        self.set_status(format!(
            "Skill candidate: {}",
            self.data.compilers[self.pending_compiler]
        ));
    }

    fn confirm_skill_picker(&mut self) {
        if self.data.compilers.is_empty() {
            self.show_skill_picker = false;
            self.set_status("No compiler skills configured");
            return;
        }
        self.selected_compiler = self.pending_compiler.min(self.data.compilers.len() - 1);
        self.data.capsule.compiler = self.data.compilers[self.selected_compiler].clone();
        self.show_skill_picker = false;
        self.modal_scroll = 0;
        self.set_status(format!("Skill: {}", self.data.capsule.compiler));
        self.pending_g = false;
    }

    fn copy_pending_skill_reference(&mut self) {
        let Some(skill) = self.data.compilers.get(self.pending_compiler) else {
            self.set_status("No compiler skill selected");
            return;
        };
        let info = compiler::compiler_catalog_entries()
            .into_iter()
            .find(|entry| entry.id == *skill);
        let copied = info
            .and_then(|entry| entry.homepage.or(entry.command))
            .unwrap_or_else(|| skill.clone());
        self.clipboard_text = Some(copied);
        self.set_status(format!("Copied skill reference: {skill}"));
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
        self.exit_action = Some(TuiExitAction::OriginalResume(Box::new(
            OriginalSessionPlan {
                version: 1,
                action: SessionAction::OriginalResume,
                dry_run: true,
                source_session: session.clone(),
                command,
            },
        )));
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

    pub fn is_session_starred(&self, session: &SessionSummary) -> bool {
        let key = session_star_key(session);
        self.starred_sessions.iter().any(|item| item == &key)
    }

    fn refresh_visible_sessions(&mut self) {
        let query = self.search_query.trim().to_ascii_lowercase();
        self.visible_session_indices = self
            .data
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, session)| self.session_filter.matches(session, &self.starred_sessions))
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

    fn cycle_data_space(&mut self, forward: bool) {
        if self.data_spaces.len() <= 1 {
            self.set_status("Data space: Local only");
            self.pending_g = false;
            return;
        }
        let len = self.data_spaces.len();
        let next = if forward {
            (self.selected_data_space + 1) % len
        } else if self.selected_data_space == 0 {
            len - 1
        } else {
            self.selected_data_space - 1
        };
        self.load_data_space(next);
    }

    fn load_data_space(&mut self, index: usize) {
        let Some(space) = self.data_spaces.get(index).cloned() else {
            self.set_status("Data space not found");
            self.pending_g = false;
            return;
        };
        self.data_space_load_request_id = self.data_space_load_request_id.wrapping_add(1);
        let request_id = self.data_space_load_request_id;
        let source = self.data.source;
        let target = self.data.target;
        let worker_space = space.clone();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = workbench::load_workbench_for_data_space(&worker_space, source, target);
            let _ = sender.send(result);
        });
        self.pending_session_load = None;
        self.pending_data_space_load = Some(PendingDataSpaceLoad {
            request_id,
            index,
            space: space.clone(),
            started_at: Instant::now(),
            receiver,
        });
        self.compile_status = "LOADING";
        self.verify_passed = false;
        self.set_status(format!("Loading data space: {}", space.label));
        self.pending_g = false;
    }

    fn toggle_starred_session(&mut self) {
        let Some(session) = self.current_session() else {
            self.set_status("No session selected");
            self.pending_g = false;
            return;
        };
        let key = session_star_key(session);
        let starred =
            if let Some(index) = self.starred_sessions.iter().position(|item| item == &key) {
                self.starred_sessions.remove(index);
                false
            } else {
                self.starred_sessions.push(key);
                self.starred_sessions.sort();
                self.starred_sessions.dedup();
                true
            };
        if let Err(error) = config::save_starred_sessions(&self.starred_sessions) {
            self.set_status(format!("Star save failed: {error}"));
        } else if starred {
            self.set_status("Session starred");
        } else {
            self.set_status("Session unstarred");
        }
        self.refresh_visible_sessions();
        self.clamp_selected_session();
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
        self.show_help || self.show_open_original || self.show_doctor || self.show_skill_picker
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
        self.clear_handoff_trail();
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
        self.start_handoff_trail_for_review();
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

    fn review_capsule(&mut self) {
        if self.compile_capsule_for_review() {
            self.pending_target = self.data.target;
            self.show_launch = true;
            self.launch_review = true;
            self.modal_scroll = 0;
            self.set_status("Capsule refreshed");
        }
        self.pending_g = false;
    }

    fn compile_capsule_for_review(&mut self) -> bool {
        if !self.ensure_session_details_ready("Review") {
            return false;
        }
        let compiler = self.data.compilers[self.selected_compiler].clone();
        let Some(session_id) = self.current_session().map(|session| session.id.clone()) else {
            self.compile_status = "FAILED";
            self.set_status("Review failed: no session selected");
            return false;
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
                true
            }
            Ok(None) => {
                self.compile_status = "FAILED";
                self.set_status("Review failed: session not found");
                false
            }
            Err(error) => {
                self.compile_status = "FAILED";
                self.set_status(format!("Review failed: {error}"));
                false
            }
        }
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
        if !self.current_data_space().is_local() {
            self.apply_readonly_remote_session_snapshot(session);
            return;
        }
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

    fn apply_readonly_remote_session_snapshot(&mut self, session: SessionSummary) {
        let selected_compiler = self.selected_compiler;
        let selected_session = self.selected_session;
        let space = self.current_data_space().clone();
        let sessions = self.data.sessions.clone();
        self.data = workbench::load_readonly_workbench_from_session_snapshot(
            &space,
            session.clone(),
            sessions,
            self.data.target,
        );
        self.refresh_visible_sessions();
        self.selected_session = selected_session.min(self.data.sessions.len().saturating_sub(1));
        self.clamp_selected_session();
        self.selected_event = 0;
        self.selected_compiler = selected_compiler.min(self.data.compilers.len().saturating_sub(1));
        self.data.capsule.compiler = self.data.compilers[self.selected_compiler].clone();
        self.rewind_event_id = initial_rewind_event_id(&self.data);
        self.capsule_scroll = 0;
        self.compile_status = "ACTIVE";
        self.verify_passed = false;
        self.set_status(format!(
            "Remote session summary: {} {}",
            session.cli, session.title
        ));
    }

    fn apply_data_space_load_result(
        &mut self,
        pending: PendingDataSpaceLoad,
        result: DataSpaceLoadResult,
    ) {
        if self.data_space_load_request_id != pending.request_id {
            return;
        }
        let elapsed = pending.started_at.elapsed();
        match result {
            Ok(data) => {
                let selected_compiler = self.selected_compiler;
                self.data = data;
                self.selected_data_space = pending.index;
                self.refresh_visible_sessions();
                self.clamp_selected_session();
                self.selected_event =
                    rewind_event_index(&self.data, &initial_rewind_event_id(&self.data));
                self.rewind_event_id = initial_rewind_event_id(&self.data);
                self.selected_compiler =
                    selected_compiler.min(self.data.compilers.len().saturating_sub(1));
                self.data.capsule.compiler = self.data.compilers[self.selected_compiler].clone();
                self.capsule_scroll = 0;
                self.compile_status = "ACTIVE";
                self.verify_passed = pending.space.is_local();
                self.set_status(format!(
                    "Data space: {} ({} sessions, {} ms)",
                    pending.space.label,
                    self.data.sessions.len(),
                    elapsed.as_millis()
                ));
            }
            Err(error) => {
                self.compile_status = "FAILED";
                self.verify_passed = false;
                self.set_status(format!(
                    "Data space failed: {} ({} ms)",
                    error,
                    elapsed.as_millis()
                ));
            }
        }
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
        let continuation = continuation::build_continuation_protocol(
            &session,
            self.pending_target,
            &capsule,
            None,
            ContinuationOptions::default(),
        );
        let target_command = match launcher::target_command_with_continuation(
            self.pending_target,
            &session,
            &capsule,
            &continuation,
        ) {
            Ok(command) => command,
            Err(error) => {
                self.set_status(format!("Target failed: {error}"));
                return;
            }
        };
        let verification = verifier::verify_capsule_with_continuation(
            &capsule,
            &session,
            &self.data.timeline,
            self.pending_target,
            &continuation,
        );
        let command = target_command.display.clone();
        let compiler = capsule.compiler.clone();
        let handoff_label = capsule.handoff_label;
        self.exit_action = Some(TuiExitAction::TargetHandoff(Box::new(LaunchPlan {
            version: 1,
            action: SessionAction::TargetHandoff,
            dry_run: true,
            source_session: session,
            target_cli: self.pending_target,
            compiler,
            handoff_label,
            capsule_path: None,
            command,
            target_command,
            verification,
            continuation,
        })));
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

    pub fn launch_handoff_label(&self) -> String {
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

    pub fn target_command_preview(&self) -> Option<launcher::TargetInputPreview> {
        let session = self.current_session()?;
        let capsule = self.launch_capsule_for_target(self.pending_target);
        let continuation = continuation::build_continuation_protocol(
            session,
            self.pending_target,
            &capsule,
            None,
            ContinuationOptions::default(),
        );
        let command = launcher::target_command_with_continuation(
            self.pending_target,
            session,
            &capsule,
            &continuation,
        )
        .ok()?;
        Some(launcher::TargetInputPreview {
            program: command.program,
            args: command.args,
            cwd: command.cwd,
            prompt: launcher::target_prompt_preview_with_continuation(
                session,
                &capsule,
                &continuation,
            ),
        })
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
        let continuation = continuation::build_continuation_protocol(
            session,
            target,
            &capsule,
            None,
            ContinuationOptions::default(),
        );
        Some(verifier::verify_capsule_with_continuation(
            &capsule,
            session,
            &self.data.timeline,
            target,
            &continuation,
        ))
    }

    pub(crate) fn launch_capsule_for_target(&self, target: CliTool) -> WorkCapsule {
        let mut capsule = self.data.capsule.clone();
        capsule.target_cli = target;
        capsule.handoff_label = format!("moonbox/{}-rewind-{}", target.id(), self.rewind_event_id);
        capsule
    }

    fn apply_rewind_event(&mut self, id: String, title: String) {
        self.rewind_event_id = id.clone();
        self.data.capsule.rewind_point = format!("{id} / {title}");
        self.data.capsule.handoff_label = format!("moonbox/{}-rewind-{id}", self.data.target.id());
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

fn session_star_key(session: &SessionSummary) -> String {
    format!("{}:{}", session.cli.id(), session.id)
}

fn first_visible_timeline_event(data: &WorkbenchData, rewind_event_id: &str) -> usize {
    visible_timeline_group_heads(data, rewind_event_id)
        .first()
        .copied()
        .unwrap_or(0)
}

fn last_visible_timeline_event(data: &WorkbenchData, rewind_event_id: &str) -> usize {
    visible_timeline_group_heads(data, rewind_event_id)
        .last()
        .copied()
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
    let group_heads = visible_timeline_group_heads(data, rewind_event_id);
    let current = selected_visible_timeline_group_position(
        data,
        rewind_event_id,
        selected_event,
        &group_heads,
    );
    group_heads
        .get(current.saturating_add(1))
        .copied()
        .unwrap_or_else(|| nearest_visible_timeline_event(data, rewind_event_id, selected_event))
}

fn previous_visible_timeline_event(
    data: &WorkbenchData,
    rewind_event_id: &str,
    selected_event: usize,
) -> usize {
    let group_heads = visible_timeline_group_heads(data, rewind_event_id);
    let current = selected_visible_timeline_group_position(
        data,
        rewind_event_id,
        selected_event,
        &group_heads,
    );
    current
        .checked_sub(1)
        .and_then(|index| group_heads.get(index))
        .copied()
        .unwrap_or_else(|| nearest_visible_timeline_event(data, rewind_event_id, selected_event))
}

fn visible_timeline_group_heads(data: &WorkbenchData, rewind_event_id: &str) -> Vec<usize> {
    let mut heads = Vec::new();
    let mut previous_kind = None;
    for (index, event) in data.timeline.iter().enumerate() {
        if !timeline_event_is_visible(data, rewind_event_id, index) {
            continue;
        }
        let continues_ai_group =
            event.kind == TimelineKind::Assistant && previous_kind == Some(TimelineKind::Assistant);
        if !continues_ai_group {
            heads.push(index);
        }
        previous_kind = Some(event.kind);
    }
    heads
}

fn selected_visible_timeline_group_position(
    data: &WorkbenchData,
    rewind_event_id: &str,
    selected_event: usize,
    group_heads: &[usize],
) -> usize {
    if group_heads.is_empty() {
        return 0;
    }
    let visible_event = nearest_visible_timeline_event(data, rewind_event_id, selected_event);
    group_heads
        .iter()
        .enumerate()
        .rev()
        .find(|(_, head)| **head <= visible_event)
        .map(|(position, _)| position)
        .unwrap_or(0)
}

fn session_matches_query(session: &SessionSummary, query: &str) -> bool {
    query.is_empty()
        || session.id.to_ascii_lowercase().contains(query)
        || session.title.to_ascii_lowercase().contains(query)
        || session.cwd.to_ascii_lowercase().contains(query)
        || session
            .source_path
            .as_ref()
            .is_some_and(|path| path.to_ascii_lowercase().contains(query))
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
        let mut app = App::new(source, target).expect("app");
        app.starred_sessions.clear();
        app.refresh_visible_sessions();
        app
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
        assert!(app.data.capsule.handoff_label.contains("evt-001"));
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
    fn timeline_navigation_moves_by_visible_ai_groups() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.focus = Focus::Timeline;
        app.data.timeline = vec![
            crate::core::model::TimelineEvent {
                id: "evt-001".into(),
                time: "10:00".into(),
                kind: TimelineKind::User,
                title: "User".into(),
                detail: "分析下 cxcp".into(),
                metadata: Default::default(),
            },
            crate::core::model::TimelineEvent {
                id: "evt-002".into(),
                time: "10:01".into(),
                kind: TimelineKind::Assistant,
                title: "Assistant".into(),
                detail: "先定位项目。".into(),
                metadata: Default::default(),
            },
            crate::core::model::TimelineEvent {
                id: "evt-003".into(),
                time: "10:02".into(),
                kind: TimelineKind::Assistant,
                title: "Assistant".into(),
                detail: "继续分析缓存。".into(),
                metadata: Default::default(),
            },
            crate::core::model::TimelineEvent {
                id: "evt-004".into(),
                time: "10:03".into(),
                kind: TimelineKind::User,
                title: "User".into(),
                detail: "下一步".into(),
                metadata: Default::default(),
            },
        ];
        app.selected_event = 0;
        app.rewind_event_id = "evt-001".into();

        app.handle_key(key('j'));
        assert_eq!(app.selected_event, 1);

        app.handle_key(key('j'));
        assert_eq!(app.selected_event, 3);

        app.selected_event = 2;
        app.handle_key(key('j'));
        assert_eq!(app.selected_event, 3);

        app.handle_key(key('k'));
        assert_eq!(app.selected_event, 1);
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
    fn skill_picker_applies_selected_compiler() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        let first = app.data.capsule.compiler.clone();
        app.handle_key(key('S'));
        assert!(app.show_skill_picker);
        assert_eq!(app.data.capsule.compiler, first);
        app.handle_key(key('j'));
        app.handle_key(KeyEvent::from(KeyCode::Enter));
        assert_ne!(app.data.capsule.compiler, first);
        assert!(!app.show_skill_picker);
    }

    #[test]
    fn zoom_shortcuts_expand_restore_and_follow_focus() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.focus = Focus::Timeline;
        app.selected_event = 2;

        app.handle_key(key('+'));

        assert_eq!(app.zoomed_focus, Some(Focus::Timeline));
        assert_eq!(app.selected_event, 2);
        assert_eq!(app.status_message, "Zoomed Timeline");

        app.handle_key(KeyEvent::from(KeyCode::Tab));

        assert_eq!(app.focus, Focus::Capsule);
        assert_eq!(app.zoomed_focus, Some(Focus::Capsule));

        app.handle_key(key('-'));

        assert_eq!(app.zoomed_focus, None);
        assert_eq!(app.status_message, "Zoom restored");
    }

    #[test]
    fn data_space_shortcut_loads_selected_inventory() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.data_spaces = vec![
            dataspace::DataSpaceEntry::local(),
            dataspace::DataSpaceEntry {
                id: "local-devbox".into(),
                label: "Devbox".into(),
                kind: dataspace::DataSpaceKind::Local,
                detail: "fixture local data space".into(),
            },
        ];

        app.handle_key(key('}'));
        for _ in 0..20 {
            if app.poll_background() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        assert_eq!(app.selected_data_space, 1);
        assert!(app.status_message.contains("Data space: Devbox"));
        assert!(!app.should_quit());
    }

    #[test]
    fn review_key_refreshes_capsule_and_opens_handoff_review() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key('S'));
        app.handle_key(key('j'));
        app.handle_key(KeyEvent::from(KeyCode::Enter));
        let compiler = app.data.capsule.compiler.clone();

        app.handle_key(key('c'));

        assert_eq!(app.compile_status, "COMPILED");
        assert_eq!(app.data.capsule.compiler, compiler);
        assert!(app.show_launch);
        assert!(app.launch_review);
        assert_eq!(app.pending_target, app.data.target);
        assert_eq!(app.status_message, "Capsule refreshed");
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
    fn session_filter_cycles_starred_before_all() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('['));

        assert_eq!(app.session_filter, SessionFilter::Starred);
        assert_eq!(app.status_message, "Filter: Star");

        app.handle_key(key(']'));

        assert_eq!(app.session_filter, SessionFilter::All);
    }

    #[test]
    fn star_shortcut_toggles_current_session_and_filter() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        let session_id = app.current_session().expect("session").id.clone();

        app.handle_key(key('s'));

        assert_eq!(app.status_message, "Session starred");
        assert!(
            app.starred_sessions
                .iter()
                .any(|key| key.ends_with(session_id.as_str()))
        );

        app.apply_session_filter(SessionFilter::Starred);

        assert_eq!(app.visible_session_indices().len(), 1);
        assert_eq!(
            app.current_session().map(|session| session.id.as_str()),
            Some(session_id.as_str())
        );

        app.handle_key(key('*'));

        assert_eq!(app.status_message, "Session unstarred");
        assert!(app.visible_session_indices().is_empty());
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
    fn slash_search_matches_source_path() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        let session = app.data.sessions.get_mut(1).expect("fixture session");
        session.source_path = Some("/tmp/moonbox/raw-title-source.jsonl".into());

        app.handle_key(key('/'));
        for ch in "raw-title-source".chars() {
            app.handle_key(key(ch));
        }

        assert_eq!(app.visible_session_indices().len(), 1);
        assert_eq!(
            app.current_session()
                .and_then(|session| session.source_path.as_deref()),
            Some("/tmp/moonbox/raw-title-source.jsonl")
        );
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
        assert!(app.handoff_trail_frame().is_some());
        assert!(app.status_message.starts_with("Review launch: Codex"));
        assert_eq!(app.data.source, CliTool::Codex);
        assert_eq!(app.data.capsule.source_cli, CliTool::Codex);
        assert_eq!(app.data.capsule.source_session, "codex-cxcp-design");
        assert!(app.data.capsule.handoff_label.contains("codex"));
        assert!(app.data.branches[2].label.contains("codex"));
    }

    #[test]
    fn handoff_trail_starts_for_review_and_expires_under_800ms() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('x'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        let frame = app.handoff_trail_frame().expect("handoff trail frame");
        assert_eq!(frame.phase, HandoffTrailPhase::Review);
        assert!(frame.duration_ms <= 800);

        app.set_handoff_trail_elapsed_for_test(Duration::from_millis(
            HANDOFF_TRAIL_DURATION_MS + 1,
        ));
        assert!(app.handoff_trail_frame().is_none());
        assert!(app.poll_background());
        assert!(app.handoff_trail.is_none());
    }

    #[test]
    fn closing_launch_review_clears_handoff_trail() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('x'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(app.handoff_trail_frame().is_some());

        app.handle_key(key('q'));

        assert!(!app.show_launch);
        assert!(!app.launch_review);
        assert!(app.handoff_trail_frame().is_none());
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
        assert!(app.data.capsule.handoff_label.contains("evt-001"));
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
        assert!(app.status_message.starts_with("Verify: WARN ("));
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
