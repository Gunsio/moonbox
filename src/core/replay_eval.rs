use serde::Serialize;

use super::{
    adapter::{SourceAdapter, collect_sessions},
    compiler::{
        CapsuleCompiler, DEFAULT_COMPILER_ID, FixtureCapsuleCompiler, default_rewind_event_id,
    },
    error::CoreError,
    fixture::FixtureSourceAdapter,
    model::{
        CapsuleCompileRequest, CliTool, SessionSummary, VerificationCheck, VerificationReport,
        VerificationStatus,
    },
    verifier,
};

const TOKEN_BUDGET: usize = 100_000;

#[derive(Debug, Clone, Serialize)]
pub struct ReplayEvalReport {
    pub version: u16,
    pub fixture_only: bool,
    pub compiler: String,
    pub source_count: usize,
    pub target_count: usize,
    pub case_count: usize,
    pub pipeline_passed: bool,
    pub status_counts: ReplayEvalStatusCounts,
    pub cases: Vec<ReplayEvalCase>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ReplayEvalStatusCounts {
    pub pass: usize,
    pub warn: usize,
    pub fail: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplayEvalCase {
    pub source_cli: CliTool,
    pub target_cli: CliTool,
    pub source_session: String,
    pub rewind_event_id: String,
    pub timeline_events: usize,
    pub target_branch: String,
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
        let adapter = adapter_for(&adapters, session.cli);
        let timeline = adapter.load_timeline(&session.id)?;
        let rewind_event_id = rewind_event_id_for_session(session, &timeline.events);

        for target in CliTool::ALL {
            let request = CapsuleCompileRequest {
                version: 1,
                source_cli: session.cli,
                target_cli: target,
                source_session: session.clone(),
                rewind_event_id: rewind_event_id.clone(),
                token_budget: TOKEN_BUDGET,
                compiler: DEFAULT_COMPILER_ID.into(),
                timeline: timeline.clone(),
            };
            let output = FixtureCapsuleCompiler.compile(&request)?;
            let report =
                verifier::verify_capsule(&output.capsule, session, &timeline.events, target);
            cases.push(replay_case(
                session,
                target,
                &rewind_event_id,
                timeline.events.len(),
                output.capsule.target_branch,
                &report,
            ));
        }
    }

    Ok(ReplayEvalReport {
        version: 1,
        fixture_only: true,
        compiler: DEFAULT_COMPILER_ID.into(),
        source_count: sessions.len(),
        target_count: CliTool::ALL.len(),
        case_count: cases.len(),
        pipeline_passed: !cases.is_empty(),
        status_counts: ReplayEvalStatusCounts::from_cases(&cases),
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

fn replay_case(
    session: &SessionSummary,
    target: CliTool,
    rewind_event_id: &str,
    timeline_events: usize,
    target_branch: String,
    report: &VerificationReport,
) -> ReplayEvalCase {
    ReplayEvalCase {
        source_cli: session.cli,
        target_cli: target,
        source_session: session.id.clone(),
        rewind_event_id: rewind_event_id.into(),
        timeline_events,
        target_branch,
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

fn adapter_for(adapters: &[FixtureSourceAdapter], tool: CliTool) -> &FixtureSourceAdapter {
    adapters
        .iter()
        .find(|adapter| adapter.tool() == tool)
        .expect("fixture adapter exists for every CliTool")
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
        assert_eq!(report.case_count, CliTool::ALL.len() * CliTool::ALL.len());
        for source in CliTool::ALL {
            for target in CliTool::ALL {
                assert!(
                    report
                        .cases
                        .iter()
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
        assert!(report.status_counts.pass > 0);
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
}
