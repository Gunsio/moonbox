use super::model::{
    CliTool, SessionStatus, SessionSummary, TimelineEvent, VerificationCheck, VerificationReport,
    VerificationStatus, WorkCapsule,
};

pub fn verify_capsule(
    capsule: &WorkCapsule,
    session: &SessionSummary,
    timeline: &[TimelineEvent],
) -> VerificationReport {
    let rewind_id = rewind_event_id(capsule);
    let mut checks = Vec::new();

    checks.push(check(
        "capsule_source",
        if capsule.source_session == session.id && capsule.source_cli == session.cli {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Fail
        },
        format!(
            "capsule source {} / {} vs selected {} / {}",
            capsule.source_cli, capsule.source_session, session.cli, session.id
        ),
    ));

    checks.push(check(
        "rewind_exists",
        if timeline.iter().any(|event| event.id == rewind_id) {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Fail
        },
        format!("rewind {rewind_id} in selected timeline"),
    ));

    checks.push(check(
        "target_branch",
        if capsule.target_branch.contains(capsule.target_cli.id())
            && capsule.target_branch.contains(rewind_id)
        {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Fail
        },
        format!("target branch {}", capsule.target_branch),
    ));

    checks.push(check(
        "token_budget",
        match session.token_count {
            Some(count) if count > 100_000 => VerificationStatus::Warn,
            _ => VerificationStatus::Pass,
        },
        format!(
            "{} / 100000 tokens",
            session
                .token_count
                .map(|count| count.to_string())
                .unwrap_or_else(|| "unknown".into())
        ),
    ));

    checks.push(check(
        "source_health",
        match session.status {
            SessionStatus::Healthy => VerificationStatus::Pass,
            SessionStatus::Warning | SessionStatus::Failed => VerificationStatus::Warn,
        },
        session
            .health_reason
            .clone()
            .unwrap_or_else(|| "no health reason".into()),
    ));

    checks.push(check(
        "target_support",
        target_support_status(session, capsule.target_cli),
        if session.status == SessionStatus::Failed && session.cli == capsule.target_cli {
            format!(
                "{} raw resume is known failed for this session",
                capsule.target_cli
            )
        } else {
            format!("{} handoff dry-run supported", capsule.target_cli)
        },
    ));

    let status = overall_status(&checks);
    VerificationReport {
        version: 1,
        status,
        ready: status != VerificationStatus::Fail,
        checks,
    }
}

fn rewind_event_id(capsule: &WorkCapsule) -> &str {
    capsule
        .rewind_point
        .split_whitespace()
        .next()
        .unwrap_or_default()
}

fn target_support_status(session: &SessionSummary, target: CliTool) -> VerificationStatus {
    if session.status == SessionStatus::Failed && session.cli == target {
        VerificationStatus::Fail
    } else {
        VerificationStatus::Pass
    }
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

fn check(
    name: impl Into<String>,
    status: VerificationStatus,
    detail: impl Into<String>,
) -> VerificationCheck {
    VerificationCheck {
        name: name.into(),
        status,
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{demo, model::CliTool};

    #[test]
    fn healthy_cross_cli_capsule_passes() {
        let data = demo::demo_data(CliTool::Codex, CliTool::Hermes);
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");

        let report = verify_capsule(&data.capsule, session, &data.timeline);

        assert_eq!(report.status, VerificationStatus::Pass);
        assert!(report.ready);
    }

    #[test]
    fn failed_same_cli_capsule_fails_target_support() {
        let data = demo::demo_data(CliTool::Hermes, CliTool::Hermes);
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");

        let report = verify_capsule(&data.capsule, session, &data.timeline);

        assert_eq!(report.status, VerificationStatus::Fail);
        assert!(!report.ready);
        assert!(report.checks.iter().any(
            |check| check.name == "target_support" && check.status == VerificationStatus::Fail
        ));
    }
}
