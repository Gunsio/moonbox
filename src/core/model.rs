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
    pub parse_skip_count: usize,
    #[serde(default)]
    pub provider_metadata: Option<ProviderSessionMetadata>,
}

pub fn unknown_runtime_reason(tool: CliTool) -> String {
    format!("{tool} source adapter does not expose live runtime activity yet")
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProviderSessionMetadata {
    #[serde(default)]
    pub source: Option<String>,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompilerPresetKind {
    Builtin,
    Environment,
    Config,
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
}
