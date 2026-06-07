use super::{
    adapter::{SourceAdapter, collect_sessions, report_from_sessions},
    compiler::{
        CapsuleCompiler, DEFAULT_COMPILER_ID, FixtureCapsuleCompiler,
        compile_with_configured_runner, compiler_catalog, default_compiler_id,
        default_rewind_event_id,
    },
    error::CoreError,
    fixture::FixtureSourceAdapter,
    model::{
        BranchNode, CanonicalTimeline, CapsuleCompileOutput, CapsuleCompileRequest, CliTool,
        SessionStatus, SessionSummary, SourceAdapterReport, SourceProvenance, TimelineEvent,
        TimelineKind, WorkCapsule, WorkbenchData,
    },
    sources,
};

pub fn workbench_data(source: CliTool, target: CliTool) -> Result<WorkbenchData, CoreError> {
    let inventory = sources::source_inventory()?;
    let sessions = inventory.sessions;
    let source_adapters = inventory.adapter_reports;
    let source_session = sessions
        .iter()
        .find(|session| session.cli == source)
        .cloned()
        .unwrap_or_else(|| fallback_session(source));
    let source_session_id = source_session.id.clone();
    let timeline = preview_timeline_for_workbench(&source_session)?;

    let rewind_event_id = rewind_event_id_for_timeline(&source_session_id, &timeline);
    let compiler = default_compiler_id();
    let capsule = workbench_capsule_for_session(
        &source_session,
        target,
        &timeline,
        &rewind_event_id,
        &compiler,
    )?;
    Ok(build_workbench_data(
        source,
        target,
        source_adapters,
        sessions,
        timeline.events,
        capsule,
        &source_session_id,
    ))
}

pub fn workbench_data_for_session(
    session_id: &str,
    target: CliTool,
) -> Result<Option<WorkbenchData>, CoreError> {
    let inventory = sources::source_inventory()?;
    let sessions = inventory.sessions;
    let source_adapters = inventory.adapter_reports;
    let source_session = if let Some(session) = sessions
        .iter()
        .find(|session| session.id == session_id)
        .cloned()
    {
        Some(session)
    } else {
        find_session(session_id)?
    };
    let Some(source_session) = source_session else {
        return Ok(None);
    };
    workbench_data_from_session_snapshot(source_session, sessions, source_adapters, target)
        .map(Some)
}

pub fn workbench_data_from_session_snapshot(
    source_session: SessionSummary,
    sessions: Vec<SessionSummary>,
    source_adapters: Vec<SourceAdapterReport>,
    target: CliTool,
) -> Result<WorkbenchData, CoreError> {
    let sessions = include_source_session(sessions, &source_session);
    let source_session_id = source_session.id.clone();
    let timeline = preview_timeline_for_workbench(&source_session)?;
    let rewind_event_id = rewind_event_id_for_timeline(&source_session_id, &timeline);
    let compiler = default_compiler_id();
    let capsule = workbench_capsule_for_session(
        &source_session,
        target,
        &timeline,
        &rewind_event_id,
        &compiler,
    )?;
    Ok(build_workbench_data(
        source_session.cli,
        target,
        source_adapters,
        sessions,
        timeline.events,
        capsule,
        &source_session_id,
    ))
}

pub fn fixture_workbench_data(
    source: CliTool,
    target: CliTool,
) -> Result<WorkbenchData, CoreError> {
    let sessions = fixture_sessions()?;
    let source_adapters = fixture_source_adapter_reports()?;
    let source_session = sessions
        .iter()
        .find(|session| session.cli == source)
        .cloned()
        .unwrap_or_else(|| fallback_session(source));
    let source_session_id = source_session.id.clone();
    let timeline = fixture_timeline_for_session(&source_session)?;
    let rewind_event_id = rewind_event_id_for_timeline(&source_session_id, &timeline);
    let capsule = compile_capsule_for_session_with_fixture_compiler(
        &source_session,
        target,
        &timeline,
        &rewind_event_id,
    )?;
    Ok(build_workbench_data(
        source,
        target,
        source_adapters,
        sessions,
        timeline.events,
        capsule,
        &source_session_id,
    ))
}

fn build_workbench_data(
    source: CliTool,
    target: CliTool,
    source_adapters: Vec<SourceAdapterReport>,
    sessions: Vec<SessionSummary>,
    timeline: Vec<TimelineEvent>,
    capsule: WorkCapsule,
    source_session_id: &str,
) -> WorkbenchData {
    let rewind_id = capsule
        .rewind_point
        .split_whitespace()
        .next()
        .unwrap_or("evt-000")
        .to_string();
    let branches = vec![
        BranchNode {
            id: "root".into(),
            label: format!("original/{source_session_id}"),
            detail: "original session, read-only".into(),
            active: false,
        },
        BranchNode {
            id: rewind_id.clone(),
            label: format!("rewind/{rewind_id}"),
            detail: capsule.rewind_point.clone(),
            active: false,
        },
        BranchNode {
            id: "target".into(),
            label: capsule.target_branch.clone(),
            detail: if capsule.state == "compiled" {
                format!("compiled by {}", capsule.compiler)
            } else {
                format!("{} by {}", capsule.state, capsule.compiler)
            },
            active: true,
        },
    ];

    WorkbenchData {
        source,
        target,
        source_adapters,
        sessions,
        timeline,
        capsule,
        branches,
        compilers: compiler_catalog(),
    }
}

fn include_source_session(
    mut sessions: Vec<SessionSummary>,
    source_session: &SessionSummary,
) -> Vec<SessionSummary> {
    if !sessions
        .iter()
        .any(|session| session.id == source_session.id)
    {
        sessions.insert(0, source_session.clone());
    }
    sessions
}

#[cfg(test)]
pub type FixtureTestSourceAdapter = FixtureSourceAdapter;

pub fn sessions() -> Result<Vec<SessionSummary>, CoreError> {
    sources::list_sessions()
}

pub fn find_session(session_id: &str) -> Result<Option<SessionSummary>, CoreError> {
    sources::find_session(session_id)
}

pub fn canonical_timeline_for_session(
    session: &SessionSummary,
) -> Result<CanonicalTimeline, CoreError> {
    sources::load_timeline(session)
}

pub fn preview_timeline_for_session(
    session: &SessionSummary,
) -> Result<CanonicalTimeline, CoreError> {
    sources::load_timeline_preview(session)
}

fn preview_timeline_for_workbench(
    source_session: &SessionSummary,
) -> Result<CanonicalTimeline, CoreError> {
    if should_skip_timeline_load(source_session) {
        return Ok(empty_timeline(source_session));
    }
    preview_timeline_for_session(source_session)
}

pub fn compile_request(
    source: CliTool,
    target: CliTool,
    rewind_event_id: &str,
) -> Result<CapsuleCompileRequest, CoreError> {
    compile_request_with_compiler(source, target, rewind_event_id, &default_compiler_id())
}

pub fn compile_request_with_compiler(
    source: CliTool,
    target: CliTool,
    rewind_event_id: &str,
    compiler: &str,
) -> Result<CapsuleCompileRequest, CoreError> {
    let source_session = default_source_session(source)?;
    let timeline = timeline_for_compile_request(&source_session)?;
    Ok(CapsuleCompileRequest {
        version: 1,
        source_cli: source,
        target_cli: target,
        source_session,
        rewind_event_id: rewind_event_id.into(),
        token_budget: 100_000,
        compiler: compiler.into(),
        timeline: CanonicalTimeline {
            version: 1,
            source_cli: source,
            source_session: timeline.source_session,
            events: timeline.events,
        },
    })
}

pub fn compile_output(source: CliTool, target: CliTool) -> Result<CapsuleCompileOutput, CoreError> {
    compile_output_with_compiler(source, target, &default_compiler_id())
}

pub fn compile_output_with_compiler(
    source: CliTool,
    target: CliTool,
    compiler: &str,
) -> Result<CapsuleCompileOutput, CoreError> {
    let source_session = default_source_session(source)?;
    let timeline = timeline_for_compile_request(&source_session)?;
    let rewind_event_id = rewind_event_id_for_timeline(&source_session.id, &timeline);
    let request = CapsuleCompileRequest {
        version: 1,
        source_cli: source,
        target_cli: target,
        source_session,
        rewind_event_id,
        token_budget: 100_000,
        compiler: compiler.into(),
        timeline,
    };
    compile_with_configured_runner(&request).map_err(CoreError::from)
}

pub fn compile_capsule_for_session_id(
    session_id: &str,
    target: CliTool,
    rewind_event_id: &str,
    compiler: &str,
) -> Result<Option<WorkCapsule>, CoreError> {
    let Some(source_session) = find_session(session_id)? else {
        return Ok(None);
    };
    let timeline = canonical_timeline_for_session(&source_session)?;
    compile_capsule_for_session(
        &source_session,
        target,
        &timeline,
        rewind_event_id,
        compiler,
    )
    .map(Some)
}

pub fn launch_artifacts_for_session_id(
    session_id: &str,
    target: CliTool,
) -> Result<Option<(SessionSummary, CanonicalTimeline, WorkCapsule)>, CoreError> {
    let Some(source_session) = find_session(session_id)? else {
        return Ok(None);
    };
    let timeline = canonical_timeline_for_session(&source_session)?;
    let rewind_event_id = rewind_event_id_for_timeline(&source_session.id, &timeline);
    let compiler = default_compiler_id();
    let capsule = compile_capsule_for_session(
        &source_session,
        target,
        &timeline,
        &rewind_event_id,
        &compiler,
    )?;
    Ok(Some((source_session, timeline, capsule)))
}

fn compile_capsule_for_session(
    session: &SessionSummary,
    target: CliTool,
    timeline: &CanonicalTimeline,
    rewind_event_id: &str,
    compiler: &str,
) -> Result<WorkCapsule, CoreError> {
    let request = CapsuleCompileRequest {
        version: 1,
        source_cli: session.cli,
        target_cli: target,
        source_session: session.clone(),
        rewind_event_id: rewind_event_id.into(),
        token_budget: 100_000,
        compiler: compiler.into(),
        timeline: timeline.clone(),
    };
    Ok(compile_with_configured_runner(&request)?.capsule)
}

fn workbench_capsule_for_session(
    session: &SessionSummary,
    target: CliTool,
    timeline: &CanonicalTimeline,
    rewind_event_id: &str,
    compiler: &str,
) -> Result<WorkCapsule, CoreError> {
    if timeline
        .events
        .iter()
        .any(|event| event.id == rewind_event_id)
    {
        return compile_capsule_for_session(session, target, timeline, rewind_event_id, compiler);
    }
    Ok(pending_work_capsule(
        session,
        target,
        rewind_event_id,
        compiler,
    ))
}

fn pending_work_capsule(
    session: &SessionSummary,
    target: CliTool,
    rewind_event_id: &str,
    compiler: &str,
) -> WorkCapsule {
    let rewind_event_id = if rewind_event_id.trim().is_empty() {
        "pending"
    } else {
        rewind_event_id
    };
    WorkCapsule {
        version: 1,
        source_cli: session.cli,
        target_cli: target,
        source_session: session.id.clone(),
        rewind_point: format!("{rewind_event_id} / choose a rewind point"),
        compiler: compiler.into(),
        target_branch: format!("moonbox/{}-rewind-{rewind_event_id}", target.id()),
        goal: format!("Resume or hand off {}", session.title),
        state: "pending_rewind".into(),
        decisions: vec!["Timeline has no loaded rewind event yet.".into()],
        todo: vec![super::model::ChecklistItem {
            done: false,
            text: "Load session details and select a rewind point.".into(),
        }],
        evidence: vec![format!("session: {} ({})", session.id, session.cli.id())],
        risks: vec![
            "Launch and verify remain blocked until a real rewind event is selected.".into(),
        ],
    }
}

fn compile_capsule_for_session_with_fixture_compiler(
    session: &SessionSummary,
    target: CliTool,
    timeline: &CanonicalTimeline,
    rewind_event_id: &str,
) -> Result<WorkCapsule, CoreError> {
    let request = CapsuleCompileRequest {
        version: 1,
        source_cli: session.cli,
        target_cli: target,
        source_session: session.clone(),
        rewind_event_id: rewind_event_id.into(),
        token_budget: 100_000,
        compiler: DEFAULT_COMPILER_ID.into(),
        timeline: timeline.clone(),
    };
    Ok(FixtureCapsuleCompiler.compile(&request)?.capsule)
}

fn fixture_sessions() -> Result<Vec<SessionSummary>, CoreError> {
    let adapters = CliTool::ALL
        .into_iter()
        .map(FixtureSourceAdapter::new)
        .collect::<Vec<_>>();
    let adapter_refs = adapters
        .iter()
        .map(|adapter| adapter as &dyn SourceAdapter)
        .collect::<Vec<_>>();
    collect_sessions(&adapter_refs).map_err(CoreError::from)
}

fn fixture_source_adapter_reports() -> Result<Vec<SourceAdapterReport>, CoreError> {
    let adapters = CliTool::ALL.map(FixtureSourceAdapter::new);
    let mut reports = Vec::new();
    for adapter in &adapters {
        let sessions = adapter.list_sessions()?;
        reports.push(report_from_sessions(
            adapter.tool(),
            adapter.provenance(),
            true,
            adapter.store_path(),
            "included_fixture_snapshot",
            "fixture workbench snapshot",
            &sessions,
        ));
    }
    Ok(reports)
}

fn fixture_timeline_for_session(session: &SessionSummary) -> Result<CanonicalTimeline, CoreError> {
    FixtureSourceAdapter::new(session.cli)
        .load_timeline(&session.id)
        .map_err(CoreError::from)
}

fn rewind_event_id_for_timeline(session_id: &str, timeline: &CanonicalTimeline) -> String {
    let preferred = default_rewind_event_id(session_id);
    if is_fixture_session_id(session_id)
        && timeline.events.iter().any(|event| event.id == preferred)
    {
        return preferred.into();
    }
    timeline
        .events
        .iter()
        .rev()
        .find(|event| event.kind != TimelineKind::Tool)
        .or_else(|| timeline.events.last())
        .map(|event| event.id.clone())
        .unwrap_or_default()
}

fn is_fixture_session_id(session_id: &str) -> bool {
    matches!(
        session_id,
        "codex-cxcp-design" | "claude-qc-platform" | "hermes-cxcp-502"
    )
}

fn default_source_session(source: CliTool) -> Result<SessionSummary, CoreError> {
    Ok(sessions()?
        .into_iter()
        .find(|session| session.cli == source)
        .unwrap_or_else(|| fallback_session(source)))
}

fn timeline_for_compile_request(
    source_session: &SessionSummary,
) -> Result<CanonicalTimeline, CoreError> {
    if should_skip_timeline_load(source_session) {
        return Ok(empty_timeline(source_session));
    }
    canonical_timeline_for_session(source_session)
}

fn should_skip_timeline_load(source_session: &SessionSummary) -> bool {
    source_session.event_count == 0 && source_session.source_path.is_none()
}

fn empty_timeline(source_session: &SessionSummary) -> CanonicalTimeline {
    CanonicalTimeline {
        version: 1,
        source_cli: source_session.cli,
        source_session: source_session.id.clone(),
        events: Vec::new(),
    }
}

fn fallback_session(source: CliTool) -> SessionSummary {
    SessionSummary {
        id: format!("{}-session", source.id()),
        cli: source,
        title: "Synthetic session".into(),
        cwd: "~".into(),
        updated_at: "1970-01-01T00:00:00+00:00".into(),
        updated: "unknown".into(),
        status: SessionStatus::Warning,
        branch: None,
        token_count: None,
        health_reason: Some("synthetic fallback".into()),
        event_count: 0,
        resume_command: fallback_resume_command(source),
        source_provenance: SourceProvenance::Fixture,
        source_path: None,
        parse_skip_count: 0,
    }
}

fn fallback_resume_command(source: CliTool) -> String {
    let id = format!("{}-session", source.id());
    match source {
        CliTool::Codex => format!("codex resume {id}"),
        CliTool::Claude => format!("claude --resume {id}"),
        CliTool::Hermes => format!("hermes --resume {id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::adapter::SourceAdapter;

    #[test]
    fn sessions_are_adapter_collected_and_time_sorted() {
        let sessions = sessions().expect("sessions");

        assert_eq!(sessions.len(), 3);
        assert_eq!(sessions[0].id, "codex-cxcp-design");
        assert_eq!(sessions[1].id, "claude-qc-platform");
        assert_eq!(sessions[2].id, "hermes-cxcp-502");
    }

    #[test]
    fn fixture_adapter_returns_canonical_timeline() {
        let adapter = FixtureTestSourceAdapter::new(CliTool::Codex);
        assert_eq!(adapter.tool(), CliTool::Codex);
        let timeline = adapter
            .load_timeline("codex-cxcp-design")
            .expect("timeline");

        assert_eq!(timeline.version, 1);
        assert_eq!(timeline.source_cli, CliTool::Codex);
        assert_eq!(timeline.source_session, "codex-cxcp-design");
        assert_eq!(timeline.events[0].id, "evt-001");
    }

    #[test]
    fn compile_request_carries_source_session_rewind_and_timeline() {
        let request = compile_request(CliTool::Codex, CliTool::Hermes, "evt-091").expect("request");

        assert_eq!(request.version, 1);
        assert_eq!(request.source_session.id, "codex-cxcp-design");
        assert_eq!(request.rewind_event_id, "evt-091");
        assert_eq!(request.timeline.events.len(), 7);
        assert_eq!(request.token_budget, 100_000);
    }

    #[test]
    fn compile_request_accepts_explicit_compiler() {
        let request = compile_request_with_compiler(
            CliTool::Codex,
            CliTool::Hermes,
            "evt-091",
            "custom-skill",
        )
        .expect("request");

        assert_eq!(request.compiler, "custom-skill");
    }

    #[test]
    fn zero_event_index_session_with_source_path_still_hydrates_timeline() {
        let mut session = sessions()
            .expect("sessions")
            .into_iter()
            .find(|session| session.id == "codex-cxcp-design")
            .expect("codex session");
        session.event_count = 0;
        session.source_path = Some("indexed-by-external-store".into());

        let timeline = preview_timeline_for_workbench(&session).expect("timeline");

        assert!(!timeline.events.is_empty());
    }

    #[test]
    fn empty_timeline_builds_pending_capsule_without_compiling_missing_rewind() {
        let mut session = sessions()
            .expect("sessions")
            .into_iter()
            .find(|session| session.id == "codex-cxcp-design")
            .expect("codex session");
        session.event_count = 0;
        session.source_path = None;
        let timeline = preview_timeline_for_workbench(&session).expect("empty timeline");
        let rewind_event_id = rewind_event_id_for_timeline(&session.id, &timeline);

        let capsule = workbench_capsule_for_session(
            &session,
            CliTool::Hermes,
            &timeline,
            &rewind_event_id,
            DEFAULT_COMPILER_ID,
        )
        .expect("pending capsule");

        assert!(timeline.events.is_empty());
        assert_eq!(capsule.state, "pending_rewind");
        assert!(capsule.rewind_point.starts_with("pending /"));
    }

    #[test]
    fn real_session_default_rewind_prefers_high_signal_event_over_tool() {
        let timeline = CanonicalTimeline {
            version: 1,
            source_cli: CliTool::Codex,
            source_session: "real-codex-session".into(),
            events: vec![
                TimelineEvent {
                    id: "evt-001".into(),
                    time: "10:00".into(),
                    kind: TimelineKind::User,
                    title: "User".into(),
                    detail: "real request".into(),
                },
                TimelineEvent {
                    id: "evt-091".into(),
                    time: "10:01".into(),
                    kind: TimelineKind::Tool,
                    title: "exec_command".into(),
                    detail: "low signal tool call".into(),
                },
            ],
        };

        assert_eq!(
            rewind_event_id_for_timeline("real-codex-session", &timeline),
            "evt-001"
        );
    }
}
