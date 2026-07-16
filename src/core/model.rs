use std::fmt::{Display, Formatter};

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum CliTool {
    Codex,
    Claude,
    Hermes,
}

impl CliTool {
    pub const ALL: [Self; 3] = [Self::Codex, Self::Claude, Self::Hermes];

    pub fn id(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Hermes => "hermes",
        }
    }

    pub fn next(self) -> Self {
        let current = Self::ALL.iter().position(|tool| *tool == self).unwrap_or(0);
        Self::ALL[(current + 1) % Self::ALL.len()]
    }

    pub fn previous(self) -> Self {
        let current = Self::ALL.iter().position(|tool| *tool == self).unwrap_or(0);
        Self::ALL[(current + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

impl Display for CliTool {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            CliTool::Codex => f.write_str("Codex"),
            CliTool::Claude => f.write_str("Claude"),
            CliTool::Hermes => f.write_str("Hermes"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Healthy,
    Warning,
    Failed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceProvenance {
    Real,
    #[default]
    Fixture,
    Missing,
}

impl Display for SourceProvenance {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Real => f.write_str("real"),
            Self::Fixture => f.write_str("fixture"),
            Self::Missing => f.write_str("missing"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionRuntimeStatus {
    Active,
    Inactive,
    #[default]
    Unknown,
}

impl Display for SessionRuntimeStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => f.write_str("active"),
            Self::Inactive => f.write_str("inactive"),
            Self::Unknown => f.write_str("unknown"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub cli: CliTool,
    pub title: String,
    pub cwd: String,
    pub updated_at: String,
    pub updated: String,
    #[serde(default)]
    pub runtime_status: SessionRuntimeStatus,
    #[serde(default)]
    pub runtime_reason: Option<String>,
    pub status: SessionStatus,
    pub branch: Option<String>,
    pub token_count: Option<usize>,
    pub health_reason: Option<String>,
    pub event_count: usize,
    pub resume_command: String,
    #[serde(default)]
    pub source_provenance: SourceProvenance,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub source_size_bytes: Option<u64>,
    #[serde(default)]
    pub parse_skip_count: usize,
    #[serde(default)]
    pub provider_metadata: Option<ProviderSessionMetadata>,
    #[serde(default)]
    pub context_health: Option<ContextHealth>,
    #[serde(default)]
    pub anatomy: Option<SessionAnatomy>,
}

pub fn unknown_runtime_reason(tool: CliTool) -> String {
    format!("{tool} source adapter does not expose live runtime activity yet")
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionAnatomyStatus {
    #[default]
    Missing,
    Ready,
    Partial,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionAnatomy {
    #[serde(default)]
    pub status: SessionAnatomyStatus,
    #[serde(default)]
    pub scan_scope: String,
    #[serde(default)]
    pub source_size_bytes: Option<u64>,
    #[serde(default)]
    pub analyzed_bytes: u64,
    #[serde(default)]
    pub sampled: bool,
    #[serde(default)]
    pub total_lines: Option<usize>,
    #[serde(default)]
    pub malformed_lines: usize,
    #[serde(default)]
    pub value_signals: Vec<AnatomySignal>,
    #[serde(default)]
    pub size_profile: Vec<AnatomyMetric>,
    #[serde(default)]
    pub event_profile: Vec<AnatomyMetric>,
    #[serde(default)]
    pub content_profile: Vec<AnatomyMetric>,
    #[serde(default)]
    pub compact: Option<CompactFrontier>,
    #[serde(default)]
    pub token_profile: Option<TokenBreakdown>,
    #[serde(default)]
    pub sidecars: Vec<SessionSidecarSummary>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnatomySignal {
    #[serde(default)]
    pub rank: u8,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnatomyMetric {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub count: usize,
    #[serde(default)]
    pub bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactFrontier {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub line_number: Option<usize>,
    #[serde(default)]
    pub tail_lines: usize,
    #[serde(default)]
    pub tail_bytes: u64,
    #[serde(default)]
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSidecarSummary {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub file_count: usize,
    #[serde(default)]
    pub bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProviderSessionMetadata {
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub thread_source: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub session_key: Option<String>,
    #[serde(default)]
    pub parent_session_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub model_config: Option<Value>,
    #[serde(default)]
    pub system_prompt_snapshot: Option<String>,
    #[serde(default)]
    pub origin: Option<Value>,
    #[serde(default)]
    pub handoff: Option<ProviderHandoffMetadata>,
    #[serde(default)]
    pub token_breakdown: Option<TokenBreakdown>,
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default)]
    pub search: Option<ProviderSearchMetadata>,
    #[serde(default)]
    pub continuation_points: Vec<ProviderContinuationPoint>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceConfidence {
    Exact,
    Derived,
    Estimated,
    #[default]
    Unknown,
}

impl Display for EvidenceConfidence {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exact => f.write_str("exact"),
            Self::Derived => f.write_str("derived"),
            Self::Estimated => f.write_str("estimated"),
            Self::Unknown => f.write_str("unknown"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextHealth {
    #[serde(default)]
    pub used_tokens: Option<usize>,
    #[serde(default)]
    pub window_tokens: Option<usize>,
    #[serde(default)]
    pub quality_cliff_tokens: Option<usize>,
    #[serde(default)]
    pub compact_layers: usize,
    #[serde(default)]
    pub handoff_markers: usize,
    #[serde(default)]
    pub confidence: EvidenceConfidence,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderHandoffMetadata {
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenBreakdown {
    #[serde(default)]
    pub input: usize,
    #[serde(default)]
    pub output: usize,
    #[serde(default)]
    pub cache_read: usize,
    #[serde(default)]
    pub cache_write: usize,
    #[serde(default)]
    pub reasoning: usize,
    #[serde(default)]
    pub total: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSearchMetadata {
    #[serde(default)]
    pub backend: String,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub matched_message_count: usize,
    #[serde(default)]
    pub continuation_point_count: usize,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderContinuationPoint {
    #[serde(default)]
    pub message_id: String,
    #[serde(default)]
    pub event_id: Option<String>,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub snippet: String,
    #[serde(default)]
    pub bookend_before: Option<String>,
    #[serde(default)]
    pub bookend_after: Option<String>,
    #[serde(default)]
    pub scroll_context: ProviderScrollContext,
    #[serde(default)]
    pub score: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderScrollContext {
    #[serde(default)]
    pub message_index: usize,
    #[serde(default)]
    pub total_messages: usize,
    #[serde(default)]
    pub before_message_id: Option<String>,
    #[serde(default)]
    pub after_message_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineKind {
    User,
    Assistant,
    Tool,
    Compact,
    Error,
    GitDiff,
    RewindPoint,
}

impl TimelineKind {
    pub const ALL: [Self; 7] = [
        Self::User,
        Self::Assistant,
        Self::Tool,
        Self::Compact,
        Self::Error,
        Self::GitDiff,
        Self::RewindPoint,
    ];
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub id: String,
    pub time: String,
    pub kind: TimelineKind,
    pub title: String,
    pub detail: String,
    #[serde(default)]
    pub metadata: TimelineEventMetadata,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TimelineEventMetadata {
    #[serde(default)]
    pub raw_refs: Vec<TimelineEventRawRef>,
    #[serde(default)]
    pub message_ids: Vec<String>,
    #[serde(default)]
    pub provider_item_ids: Vec<String>,
    #[serde(default)]
    pub tool_calls: Vec<TimelineToolCall>,
    #[serde(default)]
    pub tool_results: Vec<TimelineToolResult>,
    #[serde(default)]
    pub approvals: Vec<TimelineApproval>,
    #[serde(default)]
    pub attachments: Vec<TimelineAttachment>,
    #[serde(default)]
    pub file_changes: Vec<TimelineFileChange>,
    #[serde(default)]
    pub runtime: Option<TimelineRuntimeMetadata>,
    #[serde(default)]
    pub system_prompt_snapshot: Option<String>,
    #[serde(default)]
    pub config_snapshot: Option<Value>,
    #[serde(default)]
    pub token_usage: Option<TokenBreakdown>,
    #[serde(default)]
    pub cost: Option<TimelineCostMetadata>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TimelineEventRawRef {
    #[serde(default)]
    pub source_cli: Option<CliTool>,
    #[serde(default)]
    pub source_session: Option<String>,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub line_number: Option<usize>,
    #[serde(default)]
    pub row_id: Option<String>,
    #[serde(default)]
    pub record_type: Option<String>,
    #[serde(default)]
    pub provider_kind: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub digest: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TimelineToolCall {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<Value>,
    #[serde(default)]
    pub raw: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TimelineToolResult {
    #[serde(default)]
    pub call_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub is_error: Option<bool>,
    #[serde(default)]
    pub raw: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TimelineApproval {
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub decision: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub raw: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TimelineAttachment {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub raw: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TimelineFileChange {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub operation: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub diff: Option<String>,
    #[serde(default)]
    pub raw: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineRuntimeMetadata {
    #[serde(default)]
    pub status: SessionRuntimeStatus,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub api_duration_ms: Option<u64>,
    #[serde(default)]
    pub turn_count: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TimelineCostMetadata {
    #[serde(default)]
    pub total_cost_usd: Option<f64>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub billing_source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawSourceMap {
    pub version: u16,
    pub source_cli: CliTool,
    pub source_session: String,
    pub rewind_event_id: String,
    pub source_event_count: usize,
    pub generated_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawSourceRef {
    pub source_event_id: String,
    pub kind: TimelineKind,
    pub digest: String,
    pub excerpt: String,
    pub covered: bool,
    #[serde(default)]
    pub message_ids: Vec<String>,
    #[serde(default)]
    pub provider_item_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapsuleCoverage {
    pub raw_ref_count: usize,
    pub covered_ref_count: usize,
    pub uncovered_ref_count: usize,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedactionReport {
    pub version: u16,
    pub enabled: bool,
    pub policy: String,
    pub secret_scan: bool,
    pub path_redaction: bool,
    pub event_allowlist: Vec<TimelineKind>,
    pub file_allowlist: Vec<String>,
    pub secrets_redacted: usize,
    pub paths_redacted: usize,
    pub events_removed: usize,
    pub prompt_injection_warnings: usize,
    pub external_compiler_disclosure: String,
    pub warnings: Vec<String>,
}

impl Default for RedactionReport {
    fn default() -> Self {
        Self {
            version: 1,
            enabled: false,
            policy: "disabled".into(),
            secret_scan: false,
            path_redaction: false,
            event_allowlist: Vec::new(),
            file_allowlist: Vec::new(),
            secrets_redacted: 0,
            paths_redacted: 0,
            events_removed: 0,
            prompt_injection_warnings: 0,
            external_compiler_disclosure: "redaction policy not applied".into(),
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkCapsule {
    pub version: u16,
    pub source_cli: CliTool,
    pub target_cli: CliTool,
    pub source_session: String,
    pub rewind_point: String,
    pub compiler: String,
    #[serde(alias = "target_branch")]
    pub handoff_label: String,
    pub goal: String,
    pub state: String,
    pub decisions: Vec<String>,
    pub todo: Vec<ChecklistItem>,
    pub evidence: Vec<String>,
    pub risks: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff_artifact: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff_artifact_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff_runner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff_skill: Option<String>,
    #[serde(default)]
    pub raw_source_map: Option<RawSourceMap>,
    #[serde(default)]
    pub raw_refs: Vec<RawSourceRef>,
    #[serde(default)]
    pub coverage: CapsuleCoverage,
    #[serde(default)]
    pub redaction: RedactionReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecklistItem {
    pub done: bool,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchNode {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkbenchData {
    pub source: CliTool,
    pub target: CliTool,
    pub source_adapters: Vec<SourceAdapterReport>,
    pub sessions: Vec<SessionSummary>,
    pub timeline: Vec<TimelineEvent>,
    pub capsule: WorkCapsule,
    pub branches: Vec<BranchNode>,
    pub compilers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalTimeline {
    pub version: u16,
    pub source_cli: CliTool,
    pub source_session: String,
    pub events: Vec<TimelineEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapsuleCompileRequest {
    pub version: u16,
    pub source_cli: CliTool,
    pub target_cli: CliTool,
    pub source_session: SessionSummary,
    pub rewind_event_id: String,
    pub token_budget: usize,
    pub compiler: String,
    pub timeline: CanonicalTimeline,
    #[serde(default)]
    pub redaction: RedactionReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapsuleCompileOutput {
    pub version: u16,
    pub capsule: WorkCapsule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Pass,
    Warn,
    Fail,
}

impl Display for VerificationStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            VerificationStatus::Pass => f.write_str("PASS"),
            VerificationStatus::Warn => f.write_str("WARN"),
            VerificationStatus::Fail => f.write_str("FAIL"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationCheck {
    pub name: String,
    pub status: VerificationStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationReport {
    pub version: u16,
    pub status: VerificationStatus,
    pub ready: bool,
    pub checks: Vec<VerificationCheck>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum ContinuationLevel {
    #[default]
    PromptOnly,
    PackageImport,
    WorkspaceRestore,
}

impl Display for ContinuationLevel {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PromptOnly => f.write_str("prompt-only"),
            Self::PackageImport => f.write_str("package-import"),
            Self::WorkspaceRestore => f.write_str("workspace-restore"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceRestoreMode {
    #[default]
    None,
    Branch,
    Worktree,
}

impl Display for WorkspaceRestoreMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => f.write_str("none"),
            Self::Branch => f.write_str("branch"),
            Self::Worktree => f.write_str("worktree"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContinuationOptions {
    pub requested_level: ContinuationLevel,
    pub workspace_restore: WorkspaceRestoreMode,
}

impl ContinuationOptions {
    pub fn new(
        requested_level: Option<ContinuationLevel>,
        workspace_restore: Option<WorkspaceRestoreMode>,
    ) -> Self {
        let mut options = Self {
            requested_level: requested_level.unwrap_or_default(),
            workspace_restore: workspace_restore.unwrap_or_default(),
        };
        if options.workspace_restore != WorkspaceRestoreMode::None {
            options.requested_level = ContinuationLevel::WorkspaceRestore;
        }
        if options.requested_level == ContinuationLevel::WorkspaceRestore
            && options.workspace_restore == WorkspaceRestoreMode::None
        {
            options.workspace_restore = WorkspaceRestoreMode::Worktree;
        }
        if options.requested_level != ContinuationLevel::WorkspaceRestore {
            options.workspace_restore = WorkspaceRestoreMode::None;
        }
        options
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuationProtocol {
    pub version: u16,
    pub requested_level: ContinuationLevel,
    pub target_input_level: ContinuationLevel,
    pub package_import: PackageImportPlan,
    pub workspace_restore: WorkspaceRestorePlan,
    pub notes: Vec<String>,
}

impl Default for ContinuationProtocol {
    fn default() -> Self {
        Self {
            version: 1,
            requested_level: ContinuationLevel::PromptOnly,
            target_input_level: ContinuationLevel::PromptOnly,
            package_import: PackageImportPlan::default(),
            workspace_restore: WorkspaceRestorePlan::default(),
            notes: vec!["Target receives a prompt-only Capsule summary.".into()],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageImportPlan {
    pub requested: bool,
    pub supported: bool,
    pub target_native_import: bool,
    pub capsule_path: Option<String>,
    pub command: Option<String>,
    pub reason: String,
    pub warnings: Vec<String>,
}

impl Default for PackageImportPlan {
    fn default() -> Self {
        Self {
            requested: false,
            supported: false,
            target_native_import: false,
            capsule_path: None,
            command: None,
            reason: "native continuation package import not requested".into(),
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRestorePlan {
    pub requested: bool,
    pub mode: WorkspaceRestoreMode,
    pub supported: bool,
    pub reversible: bool,
    pub preview_only: bool,
    pub source_cwd: Option<String>,
    pub target_cwd: Option<String>,
    pub branch: Option<String>,
    pub worktree_path: Option<String>,
    pub commands: Vec<String>,
    pub cleanup_commands: Vec<String>,
    pub reason: String,
    pub warnings: Vec<String>,
}

impl Default for WorkspaceRestorePlan {
    fn default() -> Self {
        Self {
            requested: false,
            mode: WorkspaceRestoreMode::None,
            supported: true,
            reversible: true,
            preview_only: false,
            source_cwd: None,
            target_cwd: None,
            branch: None,
            worktree_path: None,
            commands: Vec::new(),
            cleanup_commands: Vec::new(),
            reason: "workspace restore not requested".into(),
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub version: u16,
    pub status: VerificationStatus,
    pub ready: bool,
    pub source_adapters: Vec<SourceAdapterReport>,
    pub checks: Vec<VerificationCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceAdapterReport {
    pub cli: CliTool,
    pub provenance: SourceProvenance,
    pub active: bool,
    pub store_path: Option<String>,
    pub session_count: usize,
    pub skipped_record_count: usize,
    pub last_indexed_at: Option<String>,
    pub filter_status: String,
    pub reason: String,
    #[serde(default)]
    pub fidelity: SourceFidelity,
    #[serde(default)]
    pub capabilities: SourceCapabilities,
    #[serde(default)]
    pub list_limit: Option<usize>,
    #[serde(default)]
    pub scan_entry_limit: Option<usize>,
    #[serde(default)]
    pub summary_line_limit: Option<usize>,
    #[serde(default)]
    pub scan_entry_count: usize,
    #[serde(default)]
    pub scan_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFidelity {
    pub status: SourceFidelityStatus,
    pub primary_surface: String,
    #[serde(default)]
    pub fallback_surface: Option<String>,
    pub detail: String,
}

impl Default for SourceFidelity {
    fn default() -> Self {
        Self {
            status: SourceFidelityStatus::Missing,
            primary_surface: "none".into(),
            fallback_surface: None,
            detail: "source surface is missing".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceFidelityStatus {
    FullFidelity,
    Partial,
    Fallback,
    #[default]
    Missing,
}

impl Display for SourceFidelityStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FullFidelity => f.write_str("full-fidelity"),
            Self::Partial => f.write_str("partial"),
            Self::Fallback => f.write_str("fallback"),
            Self::Missing => f.write_str("missing"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceCapabilityStatus {
    Available,
    Planned,
    Unavailable,
    #[default]
    Unknown,
}

impl Display for SourceCapabilityStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Available => f.write_str("available"),
            Self::Planned => f.write_str("planned"),
            Self::Unavailable => f.write_str("unavailable"),
            Self::Unknown => f.write_str("unknown"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCapability {
    pub status: SourceCapabilityStatus,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCapabilities {
    pub version: u16,
    pub local_store: SourceCapability,
    pub rich_local_rpc: SourceCapability,
    pub cloud_metadata: SourceCapability,
    pub deep_link: SourceCapability,
    pub export_search: SourceCapability,
    pub remote_control: SourceCapability,
    pub fork_resume: SourceCapability,
    pub native_handoff: SourceCapability,
}

impl Default for SourceCapabilities {
    fn default() -> Self {
        Self {
            version: 1,
            local_store: SourceCapability::default(),
            rich_local_rpc: SourceCapability::default(),
            cloud_metadata: SourceCapability::default(),
            deep_link: SourceCapability::default(),
            export_search: SourceCapability::default(),
            remote_control: SourceCapability::default(),
            fork_resume: SourceCapability::default(),
            native_handoff: SourceCapability::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchValidationState {
    Ready,
    Warning,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

    pub fn warning(reasons: Vec<String>) -> Self {
        Self {
            state: LaunchValidationState::Warning,
            reasons,
        }
    }

    pub fn blocked(reasons: Vec<String>) -> Self {
        Self {
            state: LaunchValidationState::Blocked,
            reasons,
        }
    }

    pub fn summary(&self) -> String {
        self.reasons.join("; ")
    }

    pub fn is_blocked(&self) -> bool {
        self.state == LaunchValidationState::Blocked
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionAction {
    OriginalResume,
    FullAccessResume,
    NativeFork,
    NewSession,
    TargetHandoff,
    AppDeepLink,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchPlan {
    pub version: u16,
    pub action: SessionAction,
    pub dry_run: bool,
    pub source_session: SessionSummary,
    pub target_cli: CliTool,
    pub compiler: String,
    #[serde(alias = "target_branch")]
    pub handoff_label: String,
    #[serde(default)]
    pub rewind_point: String,
    pub capsule_path: Option<String>,
    pub command: String,
    pub target_command: TargetLaunchCommand,
    pub verification: VerificationReport,
    #[serde(default)]
    pub continuation: ContinuationProtocol,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetLaunchCommand {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub display: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchExecutionStatus {
    Success,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchExecution {
    pub version: u16,
    pub status: LaunchExecutionStatus,
    pub exit_code: Option<i32>,
    pub plan: LaunchPlan,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_ledger: Option<LaunchLedgerLink>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_ledger_warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OriginalSessionPlan {
    pub version: u16,
    pub action: SessionAction,
    pub dry_run: bool,
    pub source_session: SessionSummary,
    pub command: TargetLaunchCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppOpenPlan {
    pub version: u16,
    pub action: SessionAction,
    pub dry_run: bool,
    pub source_session: SessionSummary,
    pub supported: bool,
    pub deep_link: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OriginalSessionExecution {
    pub version: u16,
    pub status: LaunchExecutionStatus,
    pub exit_code: Option<i32>,
    pub plan: OriginalSessionPlan,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_ledger: Option<LaunchLedgerLink>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_ledger_warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchLedgerLink {
    pub id: i64,
    pub status: String,
    pub launched_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompilerPresetKind {
    Builtin,
    Environment,
    Config,
    Agent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompilerPresetStatus {
    Ready,
    Warning,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilerPresetInfo {
    pub id: String,
    pub kind: CompilerPresetKind,
    pub status: CompilerPresetStatus,
    pub score: u8,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub timeout_ms: Option<u64>,
    pub reason: String,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub github_stars: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_event_accepts_legacy_five_field_json() {
        let event: TimelineEvent = serde_json::from_str(
            r#"{
                "id": "evt-001",
                "time": "10:00",
                "kind": "user",
                "title": "User",
                "detail": "continue"
            }"#,
        )
        .expect("legacy event");

        assert_eq!(event.id, "evt-001");
        assert_eq!(event.kind, TimelineKind::User);
        assert!(event.metadata.raw_refs.is_empty());
        assert!(event.metadata.message_ids.is_empty());
        assert!(event.metadata.tool_calls.is_empty());
        assert!(event.metadata.token_usage.is_none());
    }

    #[test]
    fn work_capsule_accepts_legacy_target_branch_field() {
        let capsule: WorkCapsule = serde_json::from_str(
            r#"{
                "version": 1,
                "source_cli": "codex",
                "target_cli": "hermes",
                "source_session": "codex-legacy",
                "rewind_point": "evt-001 / user",
                "compiler": "engineering-handoff",
                "target_branch": "moonbox/hermes-rewind-evt-001",
                "goal": "continue safely",
                "state": "compiled",
                "decisions": [],
                "todo": [],
                "evidence": [],
                "risks": []
            }"#,
        )
        .expect("legacy capsule");

        assert_eq!(capsule.handoff_label, "moonbox/hermes-rewind-evt-001");
        assert!(capsule.raw_source_map.is_none());
        assert!(capsule.raw_refs.is_empty());
        assert_eq!(capsule.coverage.raw_ref_count, 0);
        assert!(!capsule.redaction.enabled);
        let json = serde_json::to_value(capsule).expect("capsule json");
        assert_eq!(json["handoff_label"], "moonbox/hermes-rewind-evt-001");
        assert!(json.get("target_branch").is_none());
    }

    #[test]
    fn source_adapter_report_accepts_legacy_json_without_fidelity() {
        let report: SourceAdapterReport = serde_json::from_str(
            r#"{
                "cli": "codex",
                "provenance": "real",
                "active": true,
                "store_path": "/tmp/moonbox/codex",
                "session_count": 1,
                "skipped_record_count": 0,
                "last_indexed_at": "2026-06-09T10:00:00Z",
                "filter_status": "included_real_store",
                "reason": "legacy report"
            }"#,
        )
        .expect("legacy source report");

        assert_eq!(report.fidelity.status, SourceFidelityStatus::Missing);
        assert_eq!(report.fidelity.primary_surface, "none");
        assert_eq!(report.capabilities.version, 1);
        assert_eq!(report.scan_entry_count, 0);
        assert!(!report.scan_truncated);
    }
}
