use std::{
    collections::BTreeSet,
    env,
    path::{Path, PathBuf},
};

use super::{
    compiler,
    model::{
        CliTool, ContinuationProtocol, LaunchValidation, SessionStatus, SessionSummary,
        SourceProvenance, TargetLaunchCommand, TimelineEvent, VerificationCheck,
        VerificationReport, VerificationStatus, WorkCapsule,
    },
};

const SUPPORTED_CAPSULE_VERSION: u16 = 1;
const TOKEN_BUDGET_WARN_THRESHOLD: usize = 100_000;
const CAPSULE_SIZE_WARN_BYTES: usize = 64 * 1024;
const CAPSULE_SIZE_FAIL_BYTES: usize = 128 * 1024;

pub fn verify_capsule(
    capsule: &WorkCapsule,
    session: &SessionSummary,
    timeline: &[TimelineEvent],
    requested_target: CliTool,
) -> VerificationReport {
    verify_capsule_with_continuation(
        capsule,
        session,
        timeline,
        requested_target,
        &ContinuationProtocol::default(),
    )
}

pub fn verify_capsule_with_continuation(
    capsule: &WorkCapsule,
    session: &SessionSummary,
    timeline: &[TimelineEvent],
    requested_target: CliTool,
    continuation: &ContinuationProtocol,
) -> VerificationReport {
    let rewind_id = rewind_event_id(capsule);
    let mut checks = Vec::new();

    checks.push(check(
        "capsule_version",
        if capsule.version == SUPPORTED_CAPSULE_VERSION {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Fail
        },
        format!(
            "capsule version {} vs supported {}",
            capsule.version, SUPPORTED_CAPSULE_VERSION
        ),
    ));

    let missing_fields = missing_required_fields(capsule);
    checks.push(check(
        "capsule_required_fields",
        if missing_fields.is_empty() {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Fail
        },
        if missing_fields.is_empty() {
            "required capsule fields are populated".into()
        } else {
            format!("missing required field(s): {}", missing_fields.join(", "))
        },
    ));

    checks.push(check(
        "compiler_mode",
        compiler_mode_status(capsule, session),
        compiler_mode_detail(capsule, session),
    ));

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
        "target_cli",
        if capsule.target_cli == requested_target {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Fail
        },
        format!(
            "capsule target {} vs requested {}",
            capsule.target_cli, requested_target
        ),
    ));

    checks.push(continuation_level_check(continuation));
    checks.push(package_import_check(continuation));
    checks.push(workspace_restore_check(continuation));

    checks.push(check(
        "rewind_exists",
        if timeline.iter().any(|event| event.id == rewind_id) {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Fail
        },
        format!("rewind {rewind_id} in selected timeline"),
    ));

    checks.push(semantic_source_map_check(
        capsule, session, timeline, rewind_id,
    ));

    checks.push(semantic_compiler_coverage_check(
        capsule, timeline, rewind_id,
    ));

    checks.push(todo_timeline_consistency_check(
        capsule, timeline, rewind_id,
    ));

    checks.push(file_references_check(capsule));

    checks.push(diff_applicability_check(capsule, timeline, rewind_id));

    checks.push(check(
        "handoff_label",
        if capsule.handoff_label.contains(capsule.target_cli.id())
            && capsule.handoff_label.contains(rewind_id)
        {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Fail
        },
        format!("handoff label {}", capsule.handoff_label),
    ));

    checks.push(check(
        "handoff_label_namespace",
        if capsule.handoff_label.starts_with("moonbox/") {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Warn
        },
        format!("handoff label namespace {}", capsule.handoff_label),
    ));

    checks.push(check(
        "handoff_context",
        handoff_context_status(capsule),
        handoff_context_detail(capsule),
    ));

    checks.push(check(
        "risk_context",
        if capsule.risks.is_empty() {
            VerificationStatus::Warn
        } else {
            VerificationStatus::Pass
        },
        if capsule.risks.is_empty() {
            "risk list is empty; target may miss known hazards".into()
        } else {
            format!("{} risk(s) captured", capsule.risks.len())
        },
    ));

    checks.push(check(
        "redaction_policy",
        if capsule.redaction.enabled {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Warn
        },
        if capsule.redaction.enabled {
            format!(
                "redaction {}: {} secret(s), {} path(s), {} event(s) removed",
                capsule.redaction.policy,
                capsule.redaction.secrets_redacted,
                capsule.redaction.paths_redacted,
                capsule.redaction.events_removed
            )
        } else {
            "redaction policy disabled or missing; inspect sensitive content before handoff".into()
        },
    ));

    checks.push(check(
        "capsule_size",
        capsule_size_status(capsule),
        capsule_size_detail(capsule),
    ));

    checks.push(check(
        "token_budget",
        match session.token_count {
            Some(count) if count > TOKEN_BUDGET_WARN_THRESHOLD => VerificationStatus::Warn,
            _ => VerificationStatus::Pass,
        },
        format!(
            "{} / {} tokens",
            session
                .token_count
                .map(|count| count.to_string())
                .unwrap_or_else(|| "unknown".into()),
            TOKEN_BUDGET_WARN_THRESHOLD
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
        target_support_status(session, requested_target),
        if session.status == SessionStatus::Failed && session.cli == requested_target {
            format!(
                "{} raw resume is known failed for this session",
                requested_target
            )
        } else if session.cli == requested_target {
            format!("Same-CLI handoff to {requested_target}; prefer original resume for no handoff")
        } else {
            format!("{requested_target} handoff dry-run supported")
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

pub fn validation_from_report(report: &VerificationReport) -> LaunchValidation {
    let blockers = report
        .checks
        .iter()
        .filter(|check| check.status == VerificationStatus::Fail)
        .map(|check| check.detail.clone())
        .collect::<Vec<_>>();
    if !blockers.is_empty() {
        return LaunchValidation::blocked(blockers);
    }

    let warning_checks = report
        .checks
        .iter()
        .filter(|check| check.status == VerificationStatus::Warn)
        .collect::<Vec<_>>();
    if !warning_checks.is_empty() {
        LaunchValidation::warning(validation_warning_reasons(&warning_checks))
    } else {
        LaunchValidation::ready()
    }
}

pub fn execution_command_check(command: &TargetLaunchCommand) -> VerificationCheck {
    let available = command_available(&command.program);
    check(
        "target_command",
        if available {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Fail
        },
        if available {
            format!("target command {} is executable", command.program)
        } else {
            format!(
                "target command {} was not found on disk or PATH",
                command.program
            )
        },
    )
}

pub fn execution_command_blocker(command: &TargetLaunchCommand) -> Option<String> {
    let check = execution_command_check(command);
    (check.status == VerificationStatus::Fail).then_some(check.detail)
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
    } else if session.cli == target {
        VerificationStatus::Warn
    } else {
        VerificationStatus::Pass
    }
}

fn semantic_source_map_check(
    capsule: &WorkCapsule,
    session: &SessionSummary,
    timeline: &[TimelineEvent],
    rewind_id: &str,
) -> VerificationCheck {
    let Some(source_map) = &capsule.raw_source_map else {
        return check(
            "semantic_source_map",
            VerificationStatus::Warn,
            "raw_source_map missing; semantic coverage checks are limited",
        );
    };

    let expected_events = semantic_source_events(capsule, timeline, rewind_id);
    let mut mismatches = Vec::new();
    if source_map.version != SUPPORTED_CAPSULE_VERSION {
        mismatches.push(format!("version {}", source_map.version));
    }
    if source_map.source_cli != session.cli {
        mismatches.push(format!(
            "source_cli {} vs selected {}",
            source_map.source_cli, session.cli
        ));
    }
    if source_map.source_session != session.id {
        mismatches.push(format!(
            "source_session {} vs selected {}",
            source_map.source_session, session.id
        ));
    }
    if source_map.rewind_event_id != rewind_id {
        mismatches.push(format!(
            "rewind {} vs selected {}",
            source_map.rewind_event_id, rewind_id
        ));
    }
    if source_map.source_event_count != expected_events.len() {
        mismatches.push(format!(
            "event_count {} vs expected {}",
            source_map.source_event_count,
            expected_events.len()
        ));
    }
    if source_map.generated_by != capsule.compiler {
        mismatches.push(format!(
            "generated_by {} vs compiler {}",
            source_map.generated_by, capsule.compiler
        ));
    }

    if mismatches.is_empty() {
        check(
            "semantic_source_map",
            VerificationStatus::Pass,
            format!(
                "raw source map matches selected session and {} event(s) through rewind",
                expected_events.len()
            ),
        )
    } else {
        check(
            "semantic_source_map",
            VerificationStatus::Fail,
            format!("raw source map mismatch: {}", mismatches.join("; ")),
        )
    }
}

fn validation_warning_reasons(checks: &[&VerificationCheck]) -> Vec<String> {
    const PRIORITY: [&str; 8] = [
        "target_support",
        "workspace_restore",
        "package_import",
        "continuation_level",
        "source_health",
        "semantic_compiler_coverage",
        "semantic_diff_applicability",
        "semantic_file_refs",
    ];
    let mut reasons = Vec::new();
    for name in PRIORITY {
        if let Some(check) = checks.iter().find(|check| check.name == name) {
            reasons.push(validation_warning_summary(check));
        }
        if reasons.len() >= 2 {
            break;
        }
    }
    if reasons.is_empty() {
        reasons.extend(
            checks
                .iter()
                .take(2)
                .map(|check| truncate(&check.detail, 96)),
        );
    }
    let remaining = checks.len().saturating_sub(reasons.len());
    if remaining > 0 {
        reasons.push(format!(
            "{remaining} additional warning(s); open readiness for details"
        ));
    }
    reasons
}

fn validation_warning_summary(check: &VerificationCheck) -> String {
    match check.name.as_str() {
        "continuation_level" => "requested continuation level is not target input yet".into(),
        "package_import" => "native Capsule import is not supported for this target".into(),
        "workspace_restore" => "workspace restore is preview-only or unavailable".into(),
        "semantic_compiler_coverage" => {
            "compiler coverage has uncovered critical source refs".into()
        }
        "semantic_diff_applicability" => "diff evidence is not patch-applicable".into(),
        "semantic_file_refs" => "file references could not all be verified".into(),
        _ => truncate(&check.detail, 112),
    }
}

fn continuation_level_check(continuation: &ContinuationProtocol) -> VerificationCheck {
    if continuation.requested_level == continuation.target_input_level {
        return check(
            "continuation_level",
            VerificationStatus::Pass,
            format!("target input level is {}", continuation.target_input_level),
        );
    }

    check(
        "continuation_level",
        VerificationStatus::Warn,
        format!(
            "requested {} but target input is {}; unsupported requested capabilities must be resolved before launch",
            continuation.requested_level, continuation.target_input_level
        ),
    )
}

fn package_import_check(continuation: &ContinuationProtocol) -> VerificationCheck {
    let plan = &continuation.package_import;
    if !plan.requested {
        return check(
            "package_import",
            VerificationStatus::Pass,
            plan.reason.clone(),
        );
    }
    check(
        "package_import",
        if plan.supported {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Fail
        },
        plan.reason.clone(),
    )
}

fn workspace_restore_check(continuation: &ContinuationProtocol) -> VerificationCheck {
    let plan = &continuation.workspace_restore;
    if !plan.requested {
        return check(
            "workspace_restore",
            VerificationStatus::Pass,
            plan.reason.clone(),
        );
    }
    check(
        "workspace_restore",
        if plan.supported {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Fail
        },
        plan.reason.clone(),
    )
}

fn semantic_compiler_coverage_check(
    capsule: &WorkCapsule,
    timeline: &[TimelineEvent],
    rewind_id: &str,
) -> VerificationCheck {
    if capsule.raw_refs.is_empty() {
        return check(
            "semantic_compiler_coverage",
            VerificationStatus::Warn,
            "raw_refs missing; cannot verify compiler coverage of source events",
        );
    }

    let expected_events = semantic_source_events(capsule, timeline, rewind_id);
    let expected_ids = expected_events
        .iter()
        .map(|event| event.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    let mut duplicates = BTreeSet::new();
    for raw_ref in &capsule.raw_refs {
        if !seen.insert(raw_ref.source_event_id.as_str()) {
            duplicates.insert(raw_ref.source_event_id.as_str());
        }
    }
    let raw_ids = seen;
    let missing = expected_ids
        .difference(&raw_ids)
        .copied()
        .collect::<Vec<_>>();
    let extra = raw_ids
        .difference(&expected_ids)
        .copied()
        .collect::<Vec<_>>();

    let covered_ref_count = capsule
        .raw_refs
        .iter()
        .filter(|raw_ref| raw_ref.covered)
        .count();
    let uncovered_ref_count = capsule.raw_refs.len().saturating_sub(covered_ref_count);
    let mut failures = Vec::new();
    if !duplicates.is_empty() {
        failures.push(format!(
            "duplicate refs {}",
            duplicates.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    if !missing.is_empty() {
        failures.push(format!("missing refs {}", summarize_ids(&missing)));
    }
    if !extra.is_empty() {
        failures.push(format!("unknown refs {}", summarize_ids(&extra)));
    }
    if capsule.coverage.raw_ref_count != capsule.raw_refs.len()
        || capsule.coverage.covered_ref_count != covered_ref_count
        || capsule.coverage.uncovered_ref_count != uncovered_ref_count
    {
        failures.push(format!(
            "coverage counts {} / {} / {} vs refs {} / {} / {}",
            capsule.coverage.raw_ref_count,
            capsule.coverage.covered_ref_count,
            capsule.coverage.uncovered_ref_count,
            capsule.raw_refs.len(),
            covered_ref_count,
            uncovered_ref_count
        ));
    }
    if !failures.is_empty() {
        return check(
            "semantic_compiler_coverage",
            VerificationStatus::Fail,
            failures.join("; "),
        );
    }

    let uncovered_critical = capsule
        .raw_refs
        .iter()
        .filter(|raw_ref| !raw_ref.covered && critical_source_kind(raw_ref.kind))
        .map(|raw_ref| raw_ref.source_event_id.as_str())
        .collect::<Vec<_>>();
    if !uncovered_critical.is_empty() {
        return check(
            "semantic_compiler_coverage",
            VerificationStatus::Warn,
            format!(
                "{} critical source ref(s) are not covered by capsule summary: {}; coverage {} / {}",
                uncovered_critical.len(),
                summarize_ids(&uncovered_critical),
                covered_ref_count,
                capsule.raw_refs.len()
            ),
        );
    }

    check(
        "semantic_compiler_coverage",
        VerificationStatus::Pass,
        format!(
            "critical source refs covered; coverage {} / {}",
            covered_ref_count,
            capsule.raw_refs.len()
        ),
    )
}

fn todo_timeline_consistency_check(
    capsule: &WorkCapsule,
    timeline: &[TimelineEvent],
    rewind_id: &str,
) -> VerificationCheck {
    let timeline_ids = timeline
        .iter()
        .map(|event| event.id.as_str())
        .collect::<BTreeSet<_>>();
    let mentioned = capsule
        .todo
        .iter()
        .flat_map(|item| event_ids_in_text(&item.text))
        .collect::<BTreeSet<_>>();
    let unknown = mentioned
        .iter()
        .filter(|event_id| !timeline_ids.contains(event_id.as_str()))
        .map(String::as_str)
        .collect::<Vec<_>>();

    if !unknown.is_empty() {
        return check(
            "semantic_todo_timeline",
            VerificationStatus::Fail,
            format!(
                "todo references unknown timeline event(s): {}",
                summarize_ids(&unknown)
            ),
        );
    }
    if mentioned.is_empty() {
        return check(
            "semantic_todo_timeline",
            VerificationStatus::Warn,
            "todo items do not reference timeline event ids",
        );
    }
    if !mentioned.contains(rewind_id) {
        return check(
            "semantic_todo_timeline",
            VerificationStatus::Warn,
            format!("todo references timeline event(s) but not selected rewind {rewind_id}"),
        );
    }

    check(
        "semantic_todo_timeline",
        VerificationStatus::Pass,
        format!(
            "todo references {} known timeline event id(s), including selected rewind {}",
            mentioned.len(),
            rewind_id
        ),
    )
}

fn file_references_check(capsule: &WorkCapsule) -> VerificationCheck {
    let text = capsule_semantic_text(capsule);
    let refs = text
        .iter()
        .flat_map(|value| file_refs_in_text(value))
        .collect::<BTreeSet<_>>();
    if refs.is_empty() {
        let redacted_paths = text
            .iter()
            .filter(|value| value.contains("<path:redacted>"))
            .count();
        return check(
            "semantic_file_refs",
            VerificationStatus::Warn,
            if redacted_paths > 0 {
                format!("{redacted_paths} redacted path reference(s) cannot be checked")
            } else {
                "no verifiable file references found in capsule summary fields".into()
            },
        );
    }

    let missing = refs
        .iter()
        .filter(|path| !file_ref_exists(path))
        .map(String::as_str)
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return check(
            "semantic_file_refs",
            VerificationStatus::Fail,
            format!("missing file reference(s): {}", summarize_ids(&missing)),
        );
    }

    check(
        "semantic_file_refs",
        VerificationStatus::Pass,
        format!("{} file reference(s) exist locally", refs.len()),
    )
}

fn diff_applicability_check(
    capsule: &WorkCapsule,
    timeline: &[TimelineEvent],
    rewind_id: &str,
) -> VerificationCheck {
    let diff_events = semantic_source_events(capsule, timeline, rewind_id)
        .into_iter()
        .filter(|event| event.kind == super::model::TimelineKind::GitDiff)
        .collect::<Vec<_>>();
    if diff_events.is_empty() {
        return check(
            "semantic_diff_applicability",
            VerificationStatus::Pass,
            "no git diff events through selected rewind",
        );
    }

    let mut summary_only = Vec::new();
    let mut malformed = Vec::new();
    let mut patch_like = 0usize;
    for event in diff_events {
        if is_unified_diff(&event.detail) {
            patch_like += 1;
        } else if event.detail.contains("diff --git") {
            malformed.push(event.id.as_str());
        } else {
            summary_only.push(event.id.as_str());
        }
    }

    if !malformed.is_empty() {
        return check(
            "semantic_diff_applicability",
            VerificationStatus::Fail,
            format!(
                "malformed unified diff event(s): {}",
                summarize_ids(&malformed)
            ),
        );
    }
    if !summary_only.is_empty() {
        return check(
            "semantic_diff_applicability",
            VerificationStatus::Warn,
            format!(
                "git diff event(s) are summary-only, not patch-applicable: {}",
                summarize_ids(&summary_only)
            ),
        );
    }

    check(
        "semantic_diff_applicability",
        VerificationStatus::Pass,
        format!("{patch_like} unified diff event(s) have file headers and hunks"),
    )
}

fn semantic_source_events<'a>(
    capsule: &WorkCapsule,
    timeline: &'a [TimelineEvent],
    rewind_id: &str,
) -> Vec<&'a TimelineEvent> {
    let events = timeline_events_through_rewind(timeline, rewind_id);
    if capsule.redaction.enabled && !capsule.redaction.event_allowlist.is_empty() {
        events
            .into_iter()
            .filter(|event| {
                capsule.redaction.event_allowlist.contains(&event.kind) || event.id == rewind_id
            })
            .collect()
    } else {
        events
    }
}

fn timeline_events_through_rewind<'a>(
    timeline: &'a [TimelineEvent],
    rewind_id: &str,
) -> Vec<&'a TimelineEvent> {
    let mut selected = Vec::new();
    for event in timeline {
        selected.push(event);
        if event.id == rewind_id {
            break;
        }
    }
    selected
}

fn critical_source_kind(kind: super::model::TimelineKind) -> bool {
    matches!(
        kind,
        super::model::TimelineKind::User
            | super::model::TimelineKind::Tool
            | super::model::TimelineKind::Error
            | super::model::TimelineKind::GitDiff
            | super::model::TimelineKind::RewindPoint
    )
}

fn event_ids_in_text(value: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut offset = 0usize;
    while let Some(index) = value[offset..].find("evt-") {
        let start = offset + index;
        let id = value[start..]
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '-')
            .collect::<String>();
        if id.len() > "evt-".len() {
            ids.push(id);
        }
        offset = start + "evt-".len();
    }
    ids
}

fn capsule_semantic_text(capsule: &WorkCapsule) -> Vec<&str> {
    let mut text = vec![
        capsule.rewind_point.as_str(),
        capsule.goal.as_str(),
        capsule.state.as_str(),
        capsule.handoff_label.as_str(),
    ];
    text.extend(capsule.decisions.iter().map(String::as_str));
    text.extend(capsule.todo.iter().map(|item| item.text.as_str()));
    text.extend(capsule.evidence.iter().map(String::as_str));
    text.extend(capsule.risks.iter().map(String::as_str));
    text
}

fn file_refs_in_text(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .filter_map(normalize_file_ref)
        .collect()
}

fn normalize_file_ref(token: &str) -> Option<String> {
    let token = token.trim_matches(|character: char| {
        matches!(
            character,
            '"' | '\'' | '`' | ',' | ';' | ':' | ')' | ']' | '}' | '(' | '[' | '{'
        )
    });
    let token = token.strip_prefix("file:").unwrap_or(token);
    if token.is_empty()
        || token.contains("://")
        || token.contains("<path:redacted>")
        || token.starts_with("moonbox/")
    {
        return None;
    }
    if looks_like_file_ref(token) {
        Some(token.to_owned())
    } else {
        None
    }
}

fn looks_like_file_ref(value: &str) -> bool {
    value.starts_with("./")
        || value.starts_with("../")
        || value.starts_with("~/")
        || value.starts_with('/')
        || value.starts_with("src/")
        || value.starts_with("tests/")
        || value.starts_with("fixtures/")
        || matches!(
            value,
            "README.md" | "CHANGELOG.md" | "Cargo.toml" | "Cargo.lock"
        )
        || [
            ".rs", ".md", ".toml", ".json", ".jsonl", ".yaml", ".yml", ".lock", ".sh", ".sql",
            ".txt", ".diff", ".patch",
        ]
        .iter()
        .any(|extension| value.ends_with(extension))
}

fn file_ref_exists(value: &str) -> bool {
    let path = if let Some(rest) = value.strip_prefix("~/") {
        env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(rest))
            .unwrap_or_else(|| PathBuf::from(value))
    } else {
        PathBuf::from(value)
    };
    if path.is_absolute() {
        path.exists()
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(path).exists())
            .unwrap_or(false)
    }
}

fn is_unified_diff(value: &str) -> bool {
    value.contains("diff --git")
        && value.contains("\n--- ")
        && value.contains("\n+++ ")
        && value.contains("\n@@")
}

fn summarize_ids(values: &[&str]) -> String {
    const LIMIT: usize = 5;
    let mut summary = values
        .iter()
        .take(LIMIT)
        .copied()
        .collect::<Vec<_>>()
        .join(", ");
    if values.len() > LIMIT {
        summary.push_str(&format!(", +{} more", values.len() - LIMIT));
    }
    summary
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        value.into()
    }
}

fn missing_required_fields(capsule: &WorkCapsule) -> Vec<&'static str> {
    [
        (capsule.source_session.trim().is_empty(), "source_session"),
        (capsule.rewind_point.trim().is_empty(), "rewind_point"),
        (capsule.compiler.trim().is_empty(), "compiler"),
        (capsule.handoff_label.trim().is_empty(), "handoff_label"),
        (capsule.goal.trim().is_empty(), "goal"),
        (capsule.state.trim().is_empty(), "state"),
    ]
    .into_iter()
    .filter_map(|(missing, field)| missing.then_some(field))
    .collect()
}

fn compiler_mode_status(capsule: &WorkCapsule, session: &SessionSummary) -> VerificationStatus {
    if compiler::compiler_is_builtin(&capsule.compiler)
        && session.source_provenance != SourceProvenance::Fixture
    {
        VerificationStatus::Warn
    } else {
        VerificationStatus::Pass
    }
}

fn compiler_mode_detail(capsule: &WorkCapsule, session: &SessionSummary) -> String {
    if compiler::compiler_is_builtin(&capsule.compiler) {
        if session.source_provenance == SourceProvenance::Fixture {
            format!(
                "{} built-in draft compiler for fixture replay",
                capsule.compiler
            )
        } else {
            format!(
                "{} is a built-in draft compiler; configure an external skill for production handoff",
                capsule.compiler
            )
        }
    } else {
        format!("external compiler {} selected", capsule.compiler)
    }
}

fn handoff_context_status(capsule: &WorkCapsule) -> VerificationStatus {
    if capsule.decisions.is_empty()
        || capsule.todo.is_empty()
        || capsule.evidence.is_empty()
        || !capsule.todo.iter().any(|item| !item.done)
    {
        VerificationStatus::Fail
    } else {
        VerificationStatus::Pass
    }
}

fn handoff_context_detail(capsule: &WorkCapsule) -> String {
    let mut missing = Vec::new();
    if capsule.decisions.is_empty() {
        missing.push("decisions");
    }
    if capsule.todo.is_empty() {
        missing.push("todo");
    }
    if capsule.evidence.is_empty() {
        missing.push("evidence");
    }
    if !capsule.todo.iter().any(|item| !item.done) {
        missing.push("open_todo");
    }
    if missing.is_empty() {
        format!(
            "{} decision(s), {} todo item(s), {} evidence item(s)",
            capsule.decisions.len(),
            capsule.todo.len(),
            capsule.evidence.len()
        )
    } else {
        format!("missing handoff context: {}", missing.join(", "))
    }
}

fn capsule_size_status(capsule: &WorkCapsule) -> VerificationStatus {
    match capsule_size_bytes(capsule) {
        bytes if bytes > CAPSULE_SIZE_FAIL_BYTES => VerificationStatus::Fail,
        bytes if bytes > CAPSULE_SIZE_WARN_BYTES => VerificationStatus::Warn,
        _ => VerificationStatus::Pass,
    }
}

fn capsule_size_detail(capsule: &WorkCapsule) -> String {
    format!(
        "{} / {} bytes",
        capsule_size_bytes(capsule),
        CAPSULE_SIZE_WARN_BYTES
    )
}

fn capsule_size_bytes(capsule: &WorkCapsule) -> usize {
    serde_json::to_vec(capsule)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX)
}

fn command_available(command: &str) -> bool {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return command_is_executable(path);
    }
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| command_is_executable(&dir.join(command))))
        .unwrap_or(false)
}

fn command_is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
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
    use crate::core::{data, model::CliTool};
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(unix)]
    static SCRIPT_COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn healthy_cross_cli_capsule_warns_on_semantic_evidence_gaps_but_stays_ready() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");

        let report = verify_capsule(&data.capsule, session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Warn);
        assert!(report.ready);
        assert!(report.checks.iter().any(|check| {
            check.name == "semantic_source_map" && check.status == VerificationStatus::Pass
        }));
        assert!(report.checks.iter().any(|check| {
            check.name == "semantic_compiler_coverage"
                && check.status == VerificationStatus::Warn
                && check.detail.contains("critical source ref")
        }));
        assert!(report.checks.iter().any(|check| {
            check.name == "semantic_todo_timeline" && check.status == VerificationStatus::Pass
        }));
    }

    #[test]
    fn builtin_compiler_warns_for_real_source_handoff() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let mut session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session")
            .clone();
        session.source_provenance = SourceProvenance::Real;

        let report = verify_capsule(&data.capsule, &session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Warn);
        assert!(report.ready);
        assert!(report.checks.iter().any(|check| {
            check.name == "compiler_mode" && check.status == VerificationStatus::Warn
        }));
    }

    #[test]
    fn external_compiler_passes_compiler_mode_for_real_source() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let mut session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session")
            .clone();
        session.source_provenance = SourceProvenance::Real;
        let mut capsule = data.capsule.clone();
        capsule.compiler = "production-skill".into();
        capsule.state = "compiled".into();

        let report = verify_capsule(&capsule, &session, &data.timeline, CliTool::Hermes);

        assert!(report.checks.iter().any(|check| {
            check.name == "compiler_mode" && check.status == VerificationStatus::Pass
        }));
    }

    #[test]
    fn failed_same_cli_capsule_fails_target_support() {
        let data = data::workbench_data(CliTool::Hermes, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");

        let report = verify_capsule(&data.capsule, session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Fail);
        assert!(!report.ready);
        assert!(report.checks.iter().any(
            |check| check.name == "target_support" && check.status == VerificationStatus::Fail
        ));
    }

    #[test]
    fn healthy_same_cli_capsule_warns_target_support() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Codex).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");

        let report = verify_capsule(&data.capsule, session, &data.timeline, CliTool::Codex);

        assert_eq!(report.status, VerificationStatus::Warn);
        assert!(report.ready);
        assert!(report.checks.iter().any(
            |check| check.name == "target_support" && check.status == VerificationStatus::Warn
        ));
    }

    #[test]
    fn mismatched_requested_target_fails() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");

        let report = verify_capsule(&data.capsule, session, &data.timeline, CliTool::Codex);

        assert_eq!(report.status, VerificationStatus::Fail);
        assert!(!report.ready);
        assert!(report
            .checks
            .iter()
            .any(|check| check.name == "target_cli" && check.status == VerificationStatus::Fail));
    }

    #[test]
    fn wrong_capsule_version_fails() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");
        let mut capsule = data.capsule.clone();
        capsule.version = 99;

        let report = verify_capsule(&capsule, session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Fail);
        assert!(report.checks.iter().any(|check| {
            check.name == "capsule_version" && check.status == VerificationStatus::Fail
        }));
    }

    #[test]
    fn missing_required_capsule_fields_fail() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");
        let mut capsule = data.capsule.clone();
        capsule.goal.clear();
        capsule.compiler.clear();

        let report = verify_capsule(&capsule, session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Fail);
        assert!(report.checks.iter().any(|check| {
            check.name == "capsule_required_fields"
                && check.status == VerificationStatus::Fail
                && check.detail.contains("goal")
                && check.detail.contains("compiler")
        }));
    }

    #[test]
    fn missing_handoff_context_fails() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");
        let mut capsule = data.capsule.clone();
        capsule.decisions.clear();
        capsule.todo.clear();
        capsule.evidence.clear();

        let report = verify_capsule(&capsule, session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Fail);
        assert!(report.checks.iter().any(|check| {
            check.name == "handoff_context" && check.status == VerificationStatus::Fail
        }));
    }

    #[test]
    fn all_done_todo_fails_handoff_context() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");
        let mut capsule = data.capsule.clone();
        for item in &mut capsule.todo {
            item.done = true;
        }

        let report = verify_capsule(&capsule, session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Fail);
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.name == "handoff_context" && check.detail.contains("open_todo"))
        );
    }

    #[test]
    fn empty_risk_context_warns_but_stays_ready() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");
        let mut capsule = data.capsule.clone();
        capsule.risks.clear();

        let report = verify_capsule(&capsule, session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Warn);
        assert!(report.ready);
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.name == "risk_context"
                    && check.status == VerificationStatus::Warn)
        );
    }

    #[test]
    fn handoff_label_without_rewind_fails() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");
        let mut capsule = data.capsule.clone();
        capsule.handoff_label = "moonbox/hermes-rewind-other".into();

        let report = verify_capsule(&capsule, session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Fail);
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.name == "handoff_label"
                    && check.status == VerificationStatus::Fail)
        );
    }

    #[test]
    fn semantic_source_map_mismatch_fails() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");
        let mut capsule = data.capsule.clone();
        capsule
            .raw_source_map
            .as_mut()
            .expect("raw source map")
            .source_event_count = 999;

        let report = verify_capsule(&capsule, session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Fail);
        assert!(report.checks.iter().any(|check| {
            check.name == "semantic_source_map"
                && check.status == VerificationStatus::Fail
                && check.detail.contains("event_count 999")
        }));
    }

    #[test]
    fn semantic_todo_unknown_timeline_event_fails() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");
        let mut capsule = data.capsule.clone();
        capsule.todo[0].text = "Selected rewind point evt-999".into();

        let report = verify_capsule(&capsule, session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Fail);
        assert!(report.checks.iter().any(|check| {
            check.name == "semantic_todo_timeline"
                && check.status == VerificationStatus::Fail
                && check.detail.contains("evt-999")
        }));
    }

    #[test]
    fn semantic_missing_file_reference_fails() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");
        let mut capsule = data.capsule.clone();
        capsule
            .evidence
            .push("file:fixtures/adapters/missing-m59-file.md".into());

        let report = verify_capsule(&capsule, session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Fail);
        assert!(report.checks.iter().any(|check| {
            check.name == "semantic_file_refs"
                && check.status == VerificationStatus::Fail
                && check.detail.contains("missing-m59-file.md")
        }));
    }

    #[test]
    fn oversized_capsule_fails() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");
        let mut capsule = data.capsule.clone();
        capsule.evidence.push("x".repeat(CAPSULE_SIZE_FAIL_BYTES));

        let report = verify_capsule(&capsule, session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Fail);
        assert!(report.checks.iter().any(|check| {
            check.name == "capsule_size" && check.status == VerificationStatus::Fail
        }));
    }

    #[test]
    fn execution_command_check_detects_missing_binary() {
        let command = TargetLaunchCommand {
            program: format!("/tmp/moonbox-missing-target-command-{}", std::process::id()),
            args: Vec::new(),
            cwd: None,
            display: "missing".into(),
        };

        let check = execution_command_check(&command);

        assert_eq!(check.status, VerificationStatus::Fail);
        assert!(check.detail.contains("not found"));
    }

    #[cfg(unix)]
    #[test]
    fn execution_command_check_accepts_executable_path() {
        let script = executable_script(
            "target-command",
            r#"#!/bin/sh
exit 0
"#,
        );
        let command = TargetLaunchCommand {
            program: script.to_string_lossy().into_owned(),
            args: Vec::new(),
            cwd: None,
            display: script.to_string_lossy().into_owned(),
        };

        let check = execution_command_check(&command);

        assert_eq!(check.status, VerificationStatus::Pass);
    }

    #[cfg(unix)]
    fn executable_script(name: &str, contents: &str) -> PathBuf {
        let unique = SCRIPT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "moonbox-verifier-{name}-{}-{nanos}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("script dir");
        let path = dir.join("target.sh");
        fs::write(&path, contents).expect("script");
        let mut permissions = fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("permissions");
        path
    }
}
