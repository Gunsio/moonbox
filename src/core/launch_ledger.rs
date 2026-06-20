use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{
    Connection, OptionalExtension, Row, params,
    types::{FromSqlError, Type},
};
use serde::{Deserialize, Serialize};

use super::{
    capsule_store,
    error::CoreError,
    model::{
        CliTool, LaunchExecution, LaunchExecutionStatus, LaunchLedgerLink, LaunchPlan,
        OriginalSessionExecution, OriginalSessionPlan, SessionAction, TargetLaunchCommand,
    },
};

const LEDGER_SCHEMA_VERSION: u16 = 1;
const DEFAULT_LIST_LIMIT: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchLedgerStatus {
    Success,
    Failed,
    Blocked,
}

impl LaunchLedgerStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
        }
    }
}

impl From<LaunchExecutionStatus> for LaunchLedgerStatus {
    fn from(status: LaunchExecutionStatus) -> Self {
        match status {
            LaunchExecutionStatus::Success => Self::Success,
            LaunchExecutionStatus::Failed => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchRecord {
    pub version: u16,
    pub id: i64,
    pub launched_at: String,
    pub action: SessionAction,
    pub capsule_name: Option<String>,
    pub capsule_ref: Option<String>,
    pub source_cli: CliTool,
    pub source_session: String,
    pub rewind_point: Option<String>,
    pub target_cli: Option<CliTool>,
    pub compiler: Option<String>,
    pub handoff_label: Option<String>,
    pub dry_run: bool,
    pub status: LaunchLedgerStatus,
    pub exit_code: Option<i32>,
    pub error_reason: Option<String>,
    pub command: String,
}

impl LaunchRecord {
    pub fn link(&self) -> LaunchLedgerLink {
        LaunchLedgerLink {
            id: self.id,
            status: self.status.as_str().into(),
            launched_at: self.launched_at.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct LaunchRecordInput {
    action: SessionAction,
    capsule_name: Option<String>,
    capsule_ref: Option<String>,
    source_cli: CliTool,
    source_session: String,
    rewind_point: Option<String>,
    target_cli: Option<CliTool>,
    compiler: Option<String>,
    handoff_label: Option<String>,
    dry_run: bool,
    status: LaunchLedgerStatus,
    exit_code: Option<i32>,
    error_reason: Option<String>,
    command: String,
}

pub fn default_ledger_path() -> Result<PathBuf, CoreError> {
    if let Ok(path) = env::var("MOONBOX_LAUNCH_LEDGER")
        && !path.trim().is_empty()
    {
        return Ok(PathBuf::from(path));
    }

    #[cfg(test)]
    {
        Ok(env::temp_dir().join(format!(
            "moonbox-launch-ledger-test-{}.sqlite",
            std::process::id()
        )))
    }

    #[cfg(not(test))]
    {
        let home = env::var_os("HOME").ok_or_else(|| CoreError::LaunchLedger {
            reason: "HOME is not set and MOONBOX_LAUNCH_LEDGER was not provided".into(),
        })?;
        Ok(PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("moonbox")
            .join("launches.sqlite"))
    }
}

pub fn record_target_execution(
    execution: &LaunchExecution,
    capsule_name: Option<&str>,
) -> Result<LaunchRecord, CoreError> {
    LaunchLedgerStore::open_default()?.insert(target_input_from_execution(execution, capsule_name))
}

pub fn record_target_blocked(
    plan: &LaunchPlan,
    capsule_name: Option<&str>,
    error: &CoreError,
) -> Result<LaunchRecord, CoreError> {
    LaunchLedgerStore::open_default()?.insert(target_input_from_blocked(plan, capsule_name, error))
}

pub fn record_original_execution(
    execution: &OriginalSessionExecution,
) -> Result<LaunchRecord, CoreError> {
    LaunchLedgerStore::open_default()?.insert(original_input_from_execution(execution))
}

pub fn record_original_failed(
    plan: &OriginalSessionPlan,
    error: &CoreError,
) -> Result<LaunchRecord, CoreError> {
    LaunchLedgerStore::open_default()?.insert(original_input_from_failed(plan, error))
}

pub fn list_launches(limit: usize) -> Result<Vec<LaunchRecord>, CoreError> {
    LaunchLedgerStore::open_default()?.list(normalized_limit(limit))
}

pub fn show_launch(id: i64) -> Result<Option<LaunchRecord>, CoreError> {
    LaunchLedgerStore::open_default()?.show(id)
}

pub fn link_launch_to_capsule(id: i64, capsule_name: &str) -> Result<LaunchRecord, CoreError> {
    if capsule_store::show_capsule(capsule_name)?.is_none() {
        return Err(CoreError::LaunchLedger {
            reason: format!("capsule {capsule_name} was not found"),
        });
    }
    LaunchLedgerStore::open_default()?.link_capsule(id, capsule_name)
}

pub fn list_capsule_launches(
    capsule_name: &str,
    limit: usize,
) -> Result<Vec<LaunchRecord>, CoreError> {
    if capsule_store::show_capsule(capsule_name)?.is_none() {
        return Err(CoreError::LaunchLedger {
            reason: format!("capsule {capsule_name} was not found"),
        });
    }
    LaunchLedgerStore::open_default()?.list_for_capsule(capsule_name, normalized_limit(limit))
}

pub struct LaunchLedgerStore {
    connection: Connection,
}

impl LaunchLedgerStore {
    pub fn open_default() -> Result<Self, CoreError> {
        Self::open_path(default_ledger_path()?)
    }

    pub fn open_path(path: impl AsRef<Path>) -> Result<Self, CoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| CoreError::LaunchLedger {
                reason: format!(
                    "cannot create launch ledger directory {}: {error}",
                    parent.display()
                ),
            })?;
        }
        let connection = Connection::open(path).map_err(|error| CoreError::LaunchLedger {
            reason: format!("cannot open launch ledger {}: {error}", path.display()),
        })?;
        let store = Self { connection };
        store.ensure_schema()?;
        Ok(store)
    }

    fn insert(&self, input: LaunchRecordInput) -> Result<LaunchRecord, CoreError> {
        let launched_at = now_timestamp();
        self.connection
            .execute(
                r#"
                insert into launches (
                    launched_at, action, capsule_name, capsule_ref,
                    source_cli, source_session, rewind_point, target_cli,
                    compiler, handoff_label, dry_run, status, exit_code,
                    error_reason, command
                ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                "#,
                params![
                    launched_at,
                    action_to_db(input.action),
                    input.capsule_name,
                    input.capsule_ref,
                    input.source_cli.id(),
                    input.source_session,
                    input.rewind_point,
                    input.target_cli.map(CliTool::id),
                    input.compiler,
                    input.handoff_label,
                    bool_to_db(input.dry_run),
                    input.status.as_str(),
                    input.exit_code,
                    input.error_reason,
                    input.command,
                ],
            )
            .map_err(sql_error)?;
        let id = self.connection.last_insert_rowid();
        self.show(id)?.ok_or_else(|| CoreError::LaunchLedger {
            reason: format!("recorded launch {id} could not be read back"),
        })
    }

    fn list(&self, limit: usize) -> Result<Vec<LaunchRecord>, CoreError> {
        let mut statement = self
            .connection
            .prepare(
                r#"
                select id, launched_at, action, capsule_name, capsule_ref,
                       source_cli, source_session, rewind_point, target_cli,
                       compiler, handoff_label, dry_run, status, exit_code,
                       error_reason, command
                from launches
                order by id desc
                limit ?1
                "#,
            )
            .map_err(sql_error)?;
        let rows = statement
            .query_map(params![usize_to_i64(limit)?], record_from_row)
            .map_err(sql_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(sql_error)
    }

    fn show(&self, id: i64) -> Result<Option<LaunchRecord>, CoreError> {
        self.connection
            .query_row(
                r#"
                select id, launched_at, action, capsule_name, capsule_ref,
                       source_cli, source_session, rewind_point, target_cli,
                       compiler, handoff_label, dry_run, status, exit_code,
                       error_reason, command
                from launches
                where id = ?1
                "#,
                params![id],
                record_from_row,
            )
            .optional()
            .map_err(sql_error)
    }

    fn link_capsule(&self, id: i64, capsule_name: &str) -> Result<LaunchRecord, CoreError> {
        let capsule_ref = format!("store:{capsule_name}");
        let count = self
            .connection
            .execute(
                r#"
                update launches
                set capsule_name = ?1,
                    capsule_ref = ?2
                where id = ?3
                "#,
                params![capsule_name, capsule_ref, id],
            )
            .map_err(sql_error)?;
        if count == 0 {
            return Err(CoreError::LaunchLedger {
                reason: format!("launch {id} was not found"),
            });
        }
        self.show(id)?.ok_or_else(|| CoreError::LaunchLedger {
            reason: format!("linked launch {id} could not be read back"),
        })
    }

    fn list_for_capsule(
        &self,
        capsule_name: &str,
        limit: usize,
    ) -> Result<Vec<LaunchRecord>, CoreError> {
        let mut statement = self
            .connection
            .prepare(
                r#"
                select id, launched_at, action, capsule_name, capsule_ref,
                       source_cli, source_session, rewind_point, target_cli,
                       compiler, handoff_label, dry_run, status, exit_code,
                       error_reason, command
                from launches
                where capsule_name = ?1
                order by id desc
                limit ?2
                "#,
            )
            .map_err(sql_error)?;
        let rows = statement
            .query_map(params![capsule_name, usize_to_i64(limit)?], record_from_row)
            .map_err(sql_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(sql_error)
    }

    fn ensure_schema(&self) -> Result<(), CoreError> {
        self.connection
            .execute_batch(
                r#"
                create table if not exists metadata (
                    key text primary key,
                    value text not null
                );
                create table if not exists launches (
                    id integer primary key autoincrement,
                    launched_at text not null,
                    action text not null,
                    capsule_name text,
                    capsule_ref text,
                    source_cli text not null,
                    source_session text not null,
                    rewind_point text,
                    target_cli text,
                    compiler text,
                    handoff_label text,
                    dry_run integer not null,
                    status text not null,
                    exit_code integer,
                    error_reason text,
                    command text not null
                );
                create index if not exists launches_capsule_name_idx
                    on launches(capsule_name, id desc);
                create index if not exists launches_source_session_idx
                    on launches(source_session, id desc);
                "#,
            )
            .map_err(sql_error)?;
        let version = self
            .connection
            .query_row(
                "select value from metadata where key = 'schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sql_error)?;
        match version.as_deref() {
            Some("1") => Ok(()),
            Some(version) => Err(CoreError::LaunchLedger {
                reason: format!(
                    "unsupported launch ledger schema version {version}; supported {LEDGER_SCHEMA_VERSION}"
                ),
            }),
            None => {
                self.connection
                    .execute(
                        "insert into metadata (key, value) values ('schema_version', ?1)",
                        params![LEDGER_SCHEMA_VERSION.to_string()],
                    )
                    .map_err(sql_error)?;
                Ok(())
            }
        }
    }
}

fn target_input_from_execution(
    execution: &LaunchExecution,
    capsule_name: Option<&str>,
) -> LaunchRecordInput {
    target_input(
        &execution.plan,
        capsule_name,
        false,
        execution.status.into(),
        execution.exit_code,
        None,
    )
}

fn target_input_from_blocked(
    plan: &LaunchPlan,
    capsule_name: Option<&str>,
    error: &CoreError,
) -> LaunchRecordInput {
    target_input(
        plan,
        capsule_name,
        false,
        LaunchLedgerStatus::Blocked,
        None,
        Some(safe_error_reason(error)),
    )
}

fn target_input(
    plan: &LaunchPlan,
    capsule_name: Option<&str>,
    dry_run: bool,
    status: LaunchLedgerStatus,
    exit_code: Option<i32>,
    error_reason: Option<String>,
) -> LaunchRecordInput {
    let capsule_name = capsule_name
        .map(str::to_owned)
        .or_else(|| capsule_name_from_ref(plan.capsule_path.as_deref()));
    LaunchRecordInput {
        action: SessionAction::TargetHandoff,
        capsule_name,
        capsule_ref: Some(
            plan.capsule_path
                .clone()
                .unwrap_or_else(|| "generated".into()),
        ),
        source_cli: plan.source_session.cli,
        source_session: plan.source_session.id.clone(),
        rewind_point: non_empty_option(&plan.rewind_point),
        target_cli: Some(plan.target_cli),
        compiler: Some(plan.compiler.clone()),
        handoff_label: Some(plan.handoff_label.clone()),
        dry_run,
        status,
        exit_code,
        error_reason,
        command: safe_target_command(&plan.target_command),
    }
}

fn original_input_from_execution(execution: &OriginalSessionExecution) -> LaunchRecordInput {
    LaunchRecordInput {
        action: execution.plan.action,
        capsule_name: None,
        capsule_ref: None,
        source_cli: execution.plan.source_session.cli,
        source_session: execution.plan.source_session.id.clone(),
        rewind_point: None,
        target_cli: None,
        compiler: None,
        handoff_label: None,
        dry_run: false,
        status: execution.status.into(),
        exit_code: execution.exit_code,
        error_reason: None,
        command: safe_original_command(&execution.plan.command),
    }
}

fn original_input_from_failed(plan: &OriginalSessionPlan, error: &CoreError) -> LaunchRecordInput {
    LaunchRecordInput {
        action: plan.action,
        capsule_name: None,
        capsule_ref: None,
        source_cli: plan.source_session.cli,
        source_session: plan.source_session.id.clone(),
        rewind_point: None,
        target_cli: None,
        compiler: None,
        handoff_label: None,
        dry_run: false,
        status: LaunchLedgerStatus::Failed,
        exit_code: None,
        error_reason: Some(safe_error_reason(error)),
        command: safe_original_command(&plan.command),
    }
}

fn capsule_name_from_ref(capsule_ref: Option<&str>) -> Option<String> {
    capsule_ref
        .and_then(|value| value.strip_prefix("store:"))
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
}

fn safe_original_command(command: &TargetLaunchCommand) -> String {
    safe_command_display(&command.program, &command.args, false)
}

fn safe_target_command(command: &TargetLaunchCommand) -> String {
    safe_command_display(&command.program, &command.args, true)
}

fn safe_command_display(program: &str, args: &[String], target_handoff: bool) -> String {
    std::iter::once(program.to_owned())
        .chain(safe_args(args, target_handoff))
        .map(|value| shell_quote(&value))
        .collect::<Vec<_>>()
        .join(" ")
}

fn safe_args(args: &[String], target_handoff: bool) -> Vec<String> {
    let mut safe = Vec::with_capacity(args.len());
    let mut redact_path = false;
    let mut redact_prompt = false;
    for arg in args {
        if redact_path {
            safe.push("<cwd>".into());
            redact_path = false;
            continue;
        }
        if redact_prompt {
            safe.push("<handoff-prompt>".into());
            redact_prompt = false;
            continue;
        }
        if target_handoff && matches!(arg.as_str(), "-C" | "--add-dir") {
            safe.push(arg.clone());
            redact_path = true;
            continue;
        }
        if target_handoff && arg == "--query" {
            safe.push(arg.clone());
            redact_prompt = true;
            continue;
        }
        if target_handoff && looks_like_handoff_prompt(arg) {
            safe.push("<handoff-prompt>".into());
            continue;
        }
        safe.push(arg.clone());
    }
    safe
}

fn looks_like_handoff_prompt(value: &str) -> bool {
    value.len() > 160
        || value.contains('\n')
        || value.contains("Work Capsule")
        || value.contains("Moonbox cross-CLI handoff")
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".into();
    }
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'_' | b'-' | b'.' | b'/' | b':' | b'=' | b',' | b'<' | b'>'
            )
    }) {
        return value.into();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn safe_error_reason(error: &CoreError) -> String {
    let reason = match error {
        CoreError::LaunchBlocked { reason } => reason.clone(),
        CoreError::LaunchStart { reason, .. } => format!("command failed to start: {reason}"),
        CoreError::ExecuteRequiresSession { action } => {
            format!("{action} execution requires explicit session")
        }
        CoreError::CapsuleRead { reason, .. } => format!("cannot read capsule: {reason}"),
        CoreError::CapsuleParse { reason, .. } => format!("cannot parse capsule: {reason}"),
        CoreError::CapsuleStore { reason } => format!("capsule store error: {reason}"),
        CoreError::LaunchLedger { reason } => format!("launch ledger error: {reason}"),
        CoreError::LarkExport { reason } => format!("lark export failed: {reason}"),
        CoreError::Hooks { reason } => format!("hooks configuration failed: {reason}"),
        CoreError::Setup { reason } => format!("setup failed: {reason}"),
        CoreError::Adapter(_) => "source adapter error".into(),
        CoreError::Compiler(_) => "compiler error".into(),
        CoreError::ReplayEval { reason } => format!("replay eval failed: {reason}"),
        CoreError::DataSpaceLoad { reason, .. } => format!("data space load failed: {reason}"),
        CoreError::SshConfigRead { reason, .. } => format!("ssh config read failed: {reason}"),
        CoreError::WorkspaceSnapshot { reason } => format!("workspace snapshot failed: {reason}"),
    };
    truncate_for_ledger(&reason)
}

fn truncate_for_ledger(value: &str) -> String {
    const MAX: usize = 240;
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.len() <= MAX {
        return normalized;
    }
    let mut truncated = normalized;
    truncated.truncate(MAX.saturating_sub(3));
    truncated.push_str("...");
    truncated
}

fn non_empty_option(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.into())
    }
}

fn normalized_limit(limit: usize) -> usize {
    if limit == 0 {
        DEFAULT_LIST_LIMIT
    } else {
        limit
    }
}

fn record_from_row(row: &Row<'_>) -> rusqlite::Result<LaunchRecord> {
    Ok(LaunchRecord {
        version: LEDGER_SCHEMA_VERSION,
        id: row.get(0)?,
        launched_at: row.get(1)?,
        action: action_from_db(row.get::<_, String>(2)?)?,
        capsule_name: row.get(3)?,
        capsule_ref: row.get(4)?,
        source_cli: cli_tool_from_db(row.get::<_, String>(5)?)?,
        source_session: row.get(6)?,
        rewind_point: row.get(7)?,
        target_cli: optional_cli_tool_from_db(row.get::<_, Option<String>>(8)?)?,
        compiler: row.get(9)?,
        handoff_label: row.get(10)?,
        dry_run: db_to_bool(row.get::<_, i64>(11)?)?,
        status: status_from_db(row.get::<_, String>(12)?)?,
        exit_code: row.get(13)?,
        error_reason: row.get(14)?,
        command: row.get(15)?,
    })
}

fn action_to_db(action: SessionAction) -> &'static str {
    match action {
        SessionAction::OriginalResume => "original_resume",
        SessionAction::NativeFork => "native_fork",
        SessionAction::NewSession => "new_session",
        SessionAction::TargetHandoff => "target_handoff",
        SessionAction::AppDeepLink => "app_deep_link",
    }
}

fn action_from_db(value: String) -> rusqlite::Result<SessionAction> {
    match value.as_str() {
        "original_resume" => Ok(SessionAction::OriginalResume),
        "native_fork" => Ok(SessionAction::NativeFork),
        "new_session" => Ok(SessionAction::NewSession),
        "target_handoff" => Ok(SessionAction::TargetHandoff),
        "app_deep_link" => Ok(SessionAction::AppDeepLink),
        _ => conversion_error(2, format!("unknown launch action {value}")),
    }
}

fn status_from_db(value: String) -> rusqlite::Result<LaunchLedgerStatus> {
    match value.as_str() {
        "success" => Ok(LaunchLedgerStatus::Success),
        "failed" => Ok(LaunchLedgerStatus::Failed),
        "blocked" => Ok(LaunchLedgerStatus::Blocked),
        _ => conversion_error(12, format!("unknown launch status {value}")),
    }
}

fn cli_tool_from_db(value: String) -> rusqlite::Result<CliTool> {
    match value.as_str() {
        "codex" => Ok(CliTool::Codex),
        "claude" => Ok(CliTool::Claude),
        "hermes" => Ok(CliTool::Hermes),
        _ => conversion_error(5, format!("unknown CLI tool {value}")),
    }
}

fn optional_cli_tool_from_db(value: Option<String>) -> rusqlite::Result<Option<CliTool>> {
    value.map(cli_tool_from_db).transpose()
}

fn bool_to_db(value: bool) -> i64 {
    i64::from(value)
}

fn db_to_bool(value: i64) -> rusqlite::Result<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => conversion_error(11, format!("invalid boolean value {value}")),
    }
}

fn usize_to_i64(value: usize) -> Result<i64, CoreError> {
    i64::try_from(value).map_err(|error| CoreError::LaunchLedger {
        reason: format!("launch list limit is too large: {error}"),
    })
}

fn conversion_error<T>(index: usize, reason: String) -> rusqlite::Result<T> {
    Err(rusqlite::Error::FromSqlConversionFailure(
        index,
        Type::Text,
        Box::new(FromSqlError::Other(Box::<
            dyn std::error::Error + Send + Sync,
        >::from(reason))),
    ))
}

fn now_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("unix:{seconds}")
}

fn sql_error(error: rusqlite::Error) -> CoreError {
    CoreError::LaunchLedger {
        reason: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        data,
        model::{ContinuationProtocol, VerificationReport, VerificationStatus},
    };

    fn store_path(name: &str) -> PathBuf {
        env::temp_dir().join(format!(
            "moonbox-launch-ledger-{name}-{}.sqlite",
            std::process::id()
        ))
    }

    fn fixture_plan() -> LaunchPlan {
        let workbench =
            data::fixture_workbench_data(CliTool::Codex, CliTool::Hermes).expect("fixture data");
        let source_session = workbench
            .sessions
            .iter()
            .find(|session| session.id == workbench.capsule.source_session)
            .expect("source session")
            .clone();
        LaunchPlan {
            version: 1,
            action: SessionAction::TargetHandoff,
            dry_run: true,
            source_session,
            target_cli: CliTool::Hermes,
            compiler: workbench.capsule.compiler.clone(),
            handoff_label: workbench.capsule.handoff_label.clone(),
            rewind_point: workbench.capsule.rewind_point.clone(),
            capsule_path: Some("store:demo".into()),
            command: "hermes chat --query '<prompt>'".into(),
            target_command: TargetLaunchCommand {
                program: "hermes".into(),
                args: vec![
                    "chat".into(),
                    "--query".into(),
                    "Work Capsule\nsecret".into(),
                ],
                cwd: None,
                display: "hermes chat --query '<prompt>'".into(),
            },
            verification: VerificationReport {
                version: 1,
                ready: true,
                status: VerificationStatus::Pass,
                checks: Vec::new(),
            },
            continuation: ContinuationProtocol::default(),
        }
    }

    #[test]
    fn records_target_success_without_prompt_body() {
        let path = store_path("target");
        let _ = fs::remove_file(&path);
        let store = LaunchLedgerStore::open_path(&path).expect("store");
        let plan = fixture_plan();
        let execution = LaunchExecution {
            version: 1,
            status: LaunchExecutionStatus::Success,
            exit_code: Some(0),
            plan,
            launch_ledger: None,
            launch_ledger_warning: None,
        };

        let record = store
            .insert(target_input_from_execution(&execution, Some("demo")))
            .expect("record");

        assert_eq!(record.status, LaunchLedgerStatus::Success);
        assert_eq!(record.capsule_name.as_deref(), Some("demo"));
        assert!(
            record
                .rewind_point
                .as_deref()
                .expect("rewind")
                .starts_with("evt-091")
        );
        assert!(record.command.contains("<handoff-prompt>"));
        assert!(!record.command.contains("Work Capsule"));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn records_blocked_launches_with_safe_reason() {
        let path = store_path("blocked");
        let _ = fs::remove_file(&path);
        let store = LaunchLedgerStore::open_path(&path).expect("store");
        let plan = fixture_plan();
        let error = CoreError::LaunchStart {
            command: "hermes chat --query 'Work Capsule secret'".into(),
            reason: "No such file or directory".into(),
        };

        let record = store
            .insert(target_input_from_blocked(&plan, None, &error))
            .expect("record");

        assert_eq!(record.status, LaunchLedgerStatus::Blocked);
        assert_eq!(
            record.error_reason.as_deref(),
            Some("command failed to start: No such file or directory")
        );
        assert!(
            !record
                .error_reason
                .as_deref()
                .expect("reason")
                .contains("Work Capsule")
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn records_native_fork_as_distinct_original_action() {
        let path = store_path("native-fork");
        let _ = fs::remove_file(&path);
        let store = LaunchLedgerStore::open_path(&path).expect("store");
        let workbench =
            data::fixture_workbench_data(CliTool::Codex, CliTool::Hermes).expect("fixture data");
        let source_session = workbench
            .sessions
            .iter()
            .find(|session| session.cli == CliTool::Codex)
            .expect("codex session")
            .clone();
        let plan = OriginalSessionPlan {
            version: 1,
            action: SessionAction::NativeFork,
            dry_run: true,
            source_session,
            command: TargetLaunchCommand {
                program: "codex".into(),
                args: vec!["fork".into(), "codex-cxcp-design".into()],
                cwd: None,
                display: "codex fork codex-cxcp-design".into(),
            },
        };
        let execution = OriginalSessionExecution {
            version: 1,
            status: LaunchExecutionStatus::Success,
            exit_code: Some(0),
            plan: plan.clone(),
            launch_ledger: None,
            launch_ledger_warning: None,
        };

        let record = store
            .insert(original_input_from_execution(&execution))
            .expect("success record");

        assert_eq!(record.action, SessionAction::NativeFork);
        assert_eq!(record.status, LaunchLedgerStatus::Success);

        let failed_record = store
            .insert(original_input_from_failed(
                &plan,
                &CoreError::LaunchBlocked {
                    reason: "provider does not support fork".into(),
                },
            ))
            .expect("failed record");

        assert_eq!(failed_record.action, SessionAction::NativeFork);
        assert_eq!(failed_record.status, LaunchLedgerStatus::Failed);
        assert_eq!(
            failed_record.error_reason.as_deref(),
            Some("provider does not support fork")
        );
        let _ = fs::remove_file(&path);
    }
}
