use serde::Serialize;

use super::{
    adapter::{SourceAdapter, collect_sessions},
    compiler::{
        CapsuleCompiler, DEFAULT_COMPILER_ID, FixtureCapsuleCompiler, default_rewind_event_id,
    },
    error::CoreError,
    fixture::FixtureSourceAdapter,
    model::{
        CanonicalTimeline, CapsuleCompileRequest, CliTool, RedactionReport, SessionSummary,
        TargetLaunchCommand, VerificationCheck, VerificationReport, VerificationStatus,
        WorkCapsule,
    },
    redaction, verifier,
};

const TOKEN_BUDGET: usize = 100_000;
const OVERSIZED_CAPSULE_BYTES: usize = 150 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct ReplayEvalReport {
    pub version: u16,
    pub fixture_only: bool,
    pub compiler: String,
    pub source_count: usize,
    pub target_count: usize,
    pub matrix_case_count: usize,
    pub synthetic_case_count: usize,
    pub case_count: usize,
    pub coverage_count: usize,
    pub pipeline_passed: bool,
    pub status_counts: ReplayEvalStatusCounts,
    pub coverage: Vec<ReplayEvalCoverage>,
    pub cases: Vec<ReplayEvalCase>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ReplayEvalStatusCounts {
    pub pass: usize,
    pub warn: usize,
    pub fail: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayEvalCaseKind {
    Matrix,
    Synthetic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayEvalScenario {
    SuccessfulHandoff,
    SameCliHandoffWarning,
    SourceHealthWarning,
    FailedRawResume,
    TargetMismatch,
    OversizedCapsule,
    MissingToolPreflight,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReplayEvalCoverage {
    pub scenario: ReplayEvalScenario,
    pub expected_status: VerificationStatus,
    pub covered: bool,
    pub case_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplayEvalCase {
    pub case_kind: ReplayEvalCaseKind,
    pub scenario: ReplayEvalScenario,
    pub source_cli: CliTool,
    pub target_cli: CliTool,
    pub capsule_target_cli: CliTool,
    pub source_session: String,
    pub rewind_event_id: String,
    pub timeline_events: usize,
    pub handoff_label: String,
    pub ready: bool,
    pub status: VerificationStatus,
    pub check_count: usize,
    pub warnings: Vec<String>,
    pub failures: Vec<String>,
}

pub fn evaluate_fixture_replay() -> Result<ReplayEvalReport, CoreError> {
    let adapters = CliTool::ALL.map(FixtureSourceAdapter::new);
    let adapter_refs = adapters
        .iter()
        .map(|adapter| adapter as &dyn SourceAdapter)
        .collect::<Vec<_>>();
    let sessions = collect_sessions(&adapter_refs)?;

    let mut cases = Vec::new();
    for session in &sessions {
        let adapter = adapter_for(&adapters, session.cli)?;
        let (timeline, rewind_event_id, capsule) =
            compile_fixture_capsule(adapter, session, session.cli)?;

        for target in CliTool::ALL {
            let (_, _, capsule) = if target == session.cli {
                (timeline.clone(), rewind_event_id.clone(), capsule.clone())
            } else {
                compile_fixture_capsule(adapter, session, target)?
            };
            let report = verifier::verify_capsule(&capsule, session, &timeline.events, target);
            let scenario = matrix_scenario(session, target, &report);
            cases.push(replay_case(
                ReplayCaseInput {
                    case_kind: ReplayEvalCaseKind::Matrix,
                    scenario,
                    requested_target: target,
                    rewind_event_id: rewind_event_id.clone(),
                    timeline_events: timeline.events.len(),
                    capsule,
                },
                session,
                &report,
            ));
        }
    }

    cases.extend(synthetic_cases(&adapters, &sessions)?);
    let coverage = expected_coverage(&cases);
    let matrix_case_count = cases
        .iter()
        .filter(|case| case.case_kind == ReplayEvalCaseKind::Matrix)
        .count();
    let synthetic_case_count = cases
        .iter()
        .filter(|case| case.case_kind == ReplayEvalCaseKind::Synthetic)
        .count();
    let pipeline_passed = !cases.is_empty() && coverage.iter().all(|scenario| scenario.covered);

    Ok(ReplayEvalReport {
        version: 1,
        fixture_only: true,
        compiler: DEFAULT_COMPILER_ID.into(),
        source_count: sessions.len(),
        target_count: CliTool::ALL.len(),
        case_count: cases.len(),
        matrix_case_count,
        synthetic_case_count,
        coverage_count: coverage.len(),
        pipeline_passed,
        status_counts: ReplayEvalStatusCounts::from_cases(&cases),
        coverage,
        cases,
    })
}

impl ReplayEvalStatusCounts {
    fn from_cases(cases: &[ReplayEvalCase]) -> Self {
        let mut counts = Self::default();
        for case in cases {
            match case.status {
                VerificationStatus::Pass => counts.pass += 1,
                VerificationStatus::Warn => counts.warn += 1,
                VerificationStatus::Fail => counts.fail += 1,
            }
        }
        counts
    }
}

fn synthetic_cases(
    adapters: &[FixtureSourceAdapter],
    sessions: &[SessionSummary],
) -> Result<Vec<ReplayEvalCase>, CoreError> {
    let session = sessions
        .iter()
        .find(|session| session.id == "codex-cxcp-design")
        .or_else(|| sessions.first())
        .ok_or_else(|| CoreError::ReplayEval {
            reason: "fixture replay corpus has no sessions".into(),
        })?;
    let adapter = adapter_for(adapters, session.cli)?;
    let (timeline, rewind_event_id, healthy_capsule) =
        compile_fixture_capsule(adapter, session, CliTool::Hermes)?;

    let mut cases = Vec::new();

    let target_mismatch = healthy_capsule.clone();
    let report =
        verifier::verify_capsule(&target_mismatch, session, &timeline.events, CliTool::Codex);
    cases.push(replay_case(
        ReplayCaseInput {
            case_kind: ReplayEvalCaseKind::Synthetic,
            scenario: ReplayEvalScenario::TargetMismatch,
            requested_target: CliTool::Codex,
            rewind_event_id: rewind_event_id.clone(),
            timeline_events: timeline.events.len(),
            capsule: target_mismatch,
        },
        session,
        &report,
    ));

    let mut oversized = healthy_capsule.clone();
    oversized.evidence.push("x".repeat(OVERSIZED_CAPSULE_BYTES));
    let report = verifier::verify_capsule(&oversized, session, &timeline.events, CliTool::Hermes);
    cases.push(replay_case(
        ReplayCaseInput {
            case_kind: ReplayEvalCaseKind::Synthetic,
            scenario: ReplayEvalScenario::OversizedCapsule,
            requested_target: CliTool::Hermes,
            rewind_event_id: rewind_event_id.clone(),
            timeline_events: timeline.events.len(),
            capsule: oversized,
        },
        session,
        &report,
    ));

    let mut report =
        verifier::verify_capsule(&healthy_capsule, session, &timeline.events, CliTool::Hermes);
    report = with_extra_check(
        report,
        verifier::execution_command_check(&TargetLaunchCommand {
            program: "/definitely/missing/moonbox-target-binary".into(),
            args: Vec::new(),
            cwd: None,
            display: "/definitely/missing/moonbox-target-binary".into(),
        }),
    );
    cases.push(replay_case(
        ReplayCaseInput {
            case_kind: ReplayEvalCaseKind::Synthetic,
            scenario: ReplayEvalScenario::MissingToolPreflight,
            requested_target: CliTool::Hermes,
            rewind_event_id,
            timeline_events: timeline.events.len(),
            capsule: healthy_capsule,
        },
        session,
        &report,
    ));

    Ok(cases)
}

fn expected_coverage(cases: &[ReplayEvalCase]) -> Vec<ReplayEvalCoverage> {
    [
        (
            ReplayEvalScenario::SuccessfulHandoff,
            VerificationStatus::Warn,
        ),
        (
            ReplayEvalScenario::FailedRawResume,
            VerificationStatus::Fail,
        ),
        (ReplayEvalScenario::TargetMismatch, VerificationStatus::Fail),
        (
            ReplayEvalScenario::OversizedCapsule,
            VerificationStatus::Fail,
        ),
        (
            ReplayEvalScenario::MissingToolPreflight,
            VerificationStatus::Fail,
        ),
    ]
    .into_iter()
    .map(|(scenario, expected_status)| {
        let case_count = cases
            .iter()
            .filter(|case| case.scenario == scenario && case.status == expected_status)
            .count();
        ReplayEvalCoverage {
            scenario,
            expected_status,
            covered: case_count > 0,
            case_count,
        }
    })
    .collect()
}

fn compile_fixture_capsule(
    adapter: &FixtureSourceAdapter,
    session: &SessionSummary,
    target: CliTool,
) -> Result<(CanonicalTimeline, String, WorkCapsule), CoreError> {
    let timeline = adapter.load_timeline(&session.id)?;
    let rewind_event_id = rewind_event_id_for_session(session, &timeline.events);
    let request = redaction::redact_compile_request(CapsuleCompileRequest {
        version: 1,
        source_cli: session.cli,
        target_cli: target,
        source_session: session.clone(),
        rewind_event_id: rewind_event_id.clone(),
        token_budget: TOKEN_BUDGET,
        compiler: DEFAULT_COMPILER_ID.into(),
        timeline: timeline.clone(),
        redaction: RedactionReport::default(),
    });
    let output = FixtureCapsuleCompiler.compile(&request)?;
    Ok((timeline, rewind_event_id, output.capsule))
}

fn matrix_scenario(
    session: &SessionSummary,
    target: CliTool,
    report: &VerificationReport,
) -> ReplayEvalScenario {
    if report
        .checks
        .iter()
        .any(|check| check.detail.contains("raw resume is known failed"))
    {
        ReplayEvalScenario::FailedRawResume
    } else if report.ready
        && session.status == super::model::SessionStatus::Healthy
        && session.cli != target
    {
        ReplayEvalScenario::SuccessfulHandoff
    } else if session.cli == target {
        ReplayEvalScenario::SameCliHandoffWarning
    } else {
        ReplayEvalScenario::SourceHealthWarning
    }
}

fn with_extra_check(
    mut report: VerificationReport,
    check: VerificationCheck,
) -> VerificationReport {
    report.checks.push(check);
    report.status = overall_status(&report.checks);
    report.ready = report.status != VerificationStatus::Fail;
    report
}

fn overall_status(checks: &[VerificationCheck]) -> VerificationStatus {
    if checks
        .iter()
        .any(|check| check.status == VerificationStatus::Fail)
    {
        VerificationStatus::Fail
    } else if checks
        .iter()
        .any(|check| check.status == VerificationStatus::Warn)
    {
        VerificationStatus::Warn
    } else {
        VerificationStatus::Pass
    }
}

struct ReplayCaseInput {
    case_kind: ReplayEvalCaseKind,
    scenario: ReplayEvalScenario,
    requested_target: CliTool,
    rewind_event_id: String,
    timeline_events: usize,
    capsule: WorkCapsule,
}

fn replay_case(
    input: ReplayCaseInput,
    session: &SessionSummary,
    report: &VerificationReport,
) -> ReplayEvalCase {
    ReplayEvalCase {
        case_kind: input.case_kind,
        scenario: input.scenario,
        source_cli: session.cli,
        target_cli: input.requested_target,
        capsule_target_cli: input.capsule.target_cli,
        source_session: session.id.clone(),
        rewind_event_id: input.rewind_event_id,
        timeline_events: input.timeline_events,
        handoff_label: input.capsule.handoff_label,
        ready: report.ready,
        status: report.status,
        check_count: report.checks.len(),
        warnings: check_details(&report.checks, VerificationStatus::Warn),
        failures: check_details(&report.checks, VerificationStatus::Fail),
    }
}

fn check_details(checks: &[VerificationCheck], status: VerificationStatus) -> Vec<String> {
    checks
        .iter()
        .filter(|check| check.status == status)
        .map(|check| format!("{}: {}", check.name, check.detail))
        .collect()
}

fn adapter_for(
    adapters: &[FixtureSourceAdapter],
    tool: CliTool,
) -> Result<&FixtureSourceAdapter, CoreError> {
    adapters
        .iter()
        .find(|adapter| adapter.tool() == tool)
        .ok_or_else(|| CoreError::ReplayEval {
            reason: format!("missing fixture adapter for {tool}"),
        })
}

fn rewind_event_id_for_session(
    session: &SessionSummary,
    events: &[super::model::TimelineEvent],
) -> String {
    let preferred = default_rewind_event_id(&session.id);
    if events.iter().any(|event| event.id == preferred) {
        return preferred.into();
    }
    events
        .last()
        .map(|event| event.id.clone())
        .unwrap_or_else(|| preferred.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_eval_uses_only_embedded_fixture_sessions() {
        let report = evaluate_fixture_replay().expect("report");
        let session_ids = report
            .cases
            .iter()
            .map(|case| case.source_session.as_str())
            .collect::<std::collections::BTreeSet<_>>();

        assert!(report.fixture_only);
        assert_eq!(
            session_ids,
            ["claude-qc-platform", "codex-cxcp-design", "hermes-cxcp-502"]
                .into_iter()
                .collect()
        );
    }

    #[test]
    fn replay_eval_covers_every_source_target_pair() {
        let report = evaluate_fixture_replay().expect("report");

        assert_eq!(report.source_count, CliTool::ALL.len());
        assert_eq!(report.target_count, CliTool::ALL.len());
        assert_eq!(
            report.matrix_case_count,
            CliTool::ALL.len() * CliTool::ALL.len()
        );
        for source in CliTool::ALL {
            for target in CliTool::ALL {
                assert!(
                    report
                        .cases
                        .iter()
                        .filter(|case| case.case_kind == ReplayEvalCaseKind::Matrix)
                        .any(|case| case.source_cli == source && case.target_cli == target)
                );
            }
        }
    }

    #[test]
    fn replay_eval_records_expected_verifier_signals() {
        let report = evaluate_fixture_replay().expect("report");
        let failed_same_cli = report
            .cases
            .iter()
            .find(|case| case.source_cli == CliTool::Hermes && case.target_cli == CliTool::Hermes)
            .expect("hermes same-cli case");

        assert!(report.pipeline_passed);
        assert!(report.status_counts.warn > 0);
        assert!(report.status_counts.fail > 0);
        assert_eq!(failed_same_cli.status, VerificationStatus::Fail);
        assert!(!failed_same_cli.ready);
        assert!(
            failed_same_cli
                .failures
                .iter()
                .any(|failure| failure.contains("raw resume is known failed"))
        );
    }

    #[test]
    fn replay_eval_covers_safe_synthetic_regressions() {
        let report = evaluate_fixture_replay().expect("report");

        assert_eq!(report.synthetic_case_count, 3);
        assert_eq!(report.case_count, report.matrix_case_count + 3);
        assert_eq!(report.coverage_count, 5);
        assert!(report.coverage.iter().all(|coverage| coverage.covered));

        let target_mismatch = report
            .cases
            .iter()
            .find(|case| case.scenario == ReplayEvalScenario::TargetMismatch)
            .expect("target mismatch case");
        assert_eq!(target_mismatch.status, VerificationStatus::Fail);
        assert_eq!(target_mismatch.target_cli, CliTool::Codex);
        assert_eq!(target_mismatch.capsule_target_cli, CliTool::Hermes);
        assert!(
            target_mismatch
                .failures
                .iter()
                .any(|failure| failure.contains("capsule target Hermes vs requested Codex"))
        );

        let oversized = report
            .cases
            .iter()
            .find(|case| case.scenario == ReplayEvalScenario::OversizedCapsule)
            .expect("oversized case");
        assert_eq!(oversized.status, VerificationStatus::Fail);
        assert!(
            oversized
                .failures
                .iter()
                .any(|failure| failure.starts_with("capsule_size:"))
        );

        let missing_tool = report
            .cases
            .iter()
            .find(|case| case.scenario == ReplayEvalScenario::MissingToolPreflight)
            .expect("missing tool case");
        assert_eq!(missing_tool.status, VerificationStatus::Fail);
        assert_eq!(missing_tool.check_count, 24);
        assert!(
            missing_tool
                .failures
                .iter()
                .any(|failure| failure.starts_with("target_command:"))
        );
    }
}
