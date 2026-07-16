use serde::Deserialize;

use super::{
    adapter::{AdapterError, SourceAdapter},
    local_jsonl::timeline_preview_truncated_event,
    model::{
        CanonicalTimeline, CliTool, SessionRuntimeStatus, SessionSummary, SourceProvenance,
        unknown_runtime_reason,
    },
};

#[derive(Debug, Deserialize)]
struct SessionsFixture {
    sessions: Vec<SessionSummary>,
}

#[derive(Debug, Clone, Copy)]
struct FixtureSet {
    tool: CliTool,
    sessions_path: &'static str,
    sessions_json: &'static str,
    timelines: &'static [TimelineFixture],
}

#[derive(Debug, Clone, Copy)]
struct TimelineFixture {
    path: &'static str,
    json: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct FixtureSourceAdapter {
    fixture: FixtureSet,
}

impl FixtureSourceAdapter {
    pub fn new(tool: CliTool) -> Self {
        Self {
            fixture: fixture_for_tool(tool),
        }
    }

    fn parse_sessions(&self) -> Result<Vec<SessionSummary>, AdapterError> {
        serde_json::from_str::<SessionsFixture>(self.fixture.sessions_json)
            .map(|fixture| {
                fixture
                    .sessions
                    .into_iter()
                    .map(|mut session| {
                        session.source_provenance = SourceProvenance::Fixture;
                        session.source_path = Some(self.fixture.sessions_path.into());
                        session.parse_skip_count = 0;
                        session.runtime_status = SessionRuntimeStatus::Unknown;
                        session.runtime_reason = Some(unknown_runtime_reason(session.cli));
                        session
                    })
                    .collect()
            })
            .map_err(|error| AdapterError::InvalidFixture {
                tool: self.fixture.tool,
                path: self.fixture.sessions_path.into(),
                reason: error.to_string(),
            })
    }

    fn parse_timeline(&self, fixture: TimelineFixture) -> Result<CanonicalTimeline, AdapterError> {
        serde_json::from_str::<CanonicalTimeline>(fixture.json).map_err(|error| {
            AdapterError::InvalidFixture {
                tool: self.fixture.tool,
                path: fixture.path.into(),
                reason: error.to_string(),
            }
        })
    }
}

impl SourceAdapter for FixtureSourceAdapter {
    fn tool(&self) -> CliTool {
        self.fixture.tool
    }

    fn provenance(&self) -> SourceProvenance {
        SourceProvenance::Fixture
    }

    fn store_path(&self) -> Option<String> {
        Some(self.fixture.sessions_path.into())
    }

    fn list_sessions(&self) -> Result<Vec<SessionSummary>, AdapterError> {
        self.parse_sessions()
    }

    fn load_timeline(&self, session_id: &str) -> Result<CanonicalTimeline, AdapterError> {
        for fixture in self.fixture.timelines {
            let timeline = self.parse_timeline(*fixture)?;
            if timeline.source_session == session_id {
                return Ok(timeline);
            }
        }
        Err(AdapterError::SessionNotFound {
            tool: self.fixture.tool,
            session_id: session_id.into(),
        })
    }

    fn load_timeline_limited(
        &self,
        session: &SessionSummary,
        event_limit: Option<usize>,
    ) -> Result<CanonicalTimeline, AdapterError> {
        let mut timeline = self.load_timeline(&session.id)?;
        let Some(limit) = event_limit else {
            return Ok(timeline);
        };
        if limit > 0 && timeline.events.len() >= limit {
            timeline.events.truncate(limit);
            timeline.events.push(timeline_preview_truncated_event(
                timeline.events.len() + 1,
                limit,
            ));
        }
        Ok(timeline)
    }
}

const CODEX_TIMELINES: [TimelineFixture; 1] = [TimelineFixture {
    path: "fixtures/adapters/codex/timeline-codex-cxcp-design.json",
    json: include_str!("../../fixtures/adapters/codex/timeline-codex-cxcp-design.json"),
}];

const CLAUDE_TIMELINES: [TimelineFixture; 1] = [TimelineFixture {
    path: "fixtures/adapters/claude/timeline-claude-qc-platform.json",
    json: include_str!("../../fixtures/adapters/claude/timeline-claude-qc-platform.json"),
}];

const HERMES_TIMELINES: [TimelineFixture; 1] = [TimelineFixture {
    path: "fixtures/adapters/hermes/timeline-hermes-cxcp-502.json",
    json: include_str!("../../fixtures/adapters/hermes/timeline-hermes-cxcp-502.json"),
}];

fn fixture_for_tool(tool: CliTool) -> FixtureSet {
    match tool {
        CliTool::Codex => FixtureSet {
            tool,
            sessions_path: "fixtures/adapters/codex/sessions.json",
            sessions_json: include_str!("../../fixtures/adapters/codex/sessions.json"),
            timelines: &CODEX_TIMELINES,
        },
        CliTool::Claude => FixtureSet {
            tool,
            sessions_path: "fixtures/adapters/claude/sessions.json",
            sessions_json: include_str!("../../fixtures/adapters/claude/sessions.json"),
            timelines: &CLAUDE_TIMELINES,
        },
        CliTool::Hermes => FixtureSet {
            tool,
            sessions_path: "fixtures/adapters/hermes/sessions.json",
            sessions_json: include_str!("../../fixtures/adapters/hermes/sessions.json"),
            timelines: &HERMES_TIMELINES,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn parses_session_fixture_for_each_source() {
        for tool in CliTool::ALL {
            let adapter = FixtureSourceAdapter::new(tool);
            let sessions = adapter.list_sessions().expect("sessions");

            assert_eq!(sessions.len(), 1);
            assert_eq!(sessions[0].cli, tool);
            assert!(sessions[0].event_count > 0);
        }
    }

    #[test]
    fn parses_timeline_fixture_for_each_session() {
        for tool in CliTool::ALL {
            let adapter = FixtureSourceAdapter::new(tool);
            let session = adapter.list_sessions().expect("sessions").remove(0);
            let timeline = adapter.load_timeline(&session.id).expect("timeline");

            assert_eq!(timeline.source_cli, tool);
            assert_eq!(timeline.source_session, session.id);
            assert!(!timeline.events.is_empty());
            assert!(
                timeline
                    .events
                    .iter()
                    .any(|event| event.id.starts_with("evt-"))
            );
        }
    }

    #[test]
    fn fixture_timeline_limit_matches_real_source_preview_contract() {
        let adapter = FixtureSourceAdapter::new(CliTool::Codex);
        let session = adapter.list_sessions().expect("sessions").remove(0);

        let timeline = adapter
            .load_timeline_limited(&session, Some(2))
            .expect("limited timeline");

        assert_eq!(timeline.events.len(), 3);
        assert_eq!(timeline.events[2].title, "Timeline preview truncated");
        assert!(timeline.events[2].detail.contains("press G"));
    }

    #[test]
    fn fixture_adapters_satisfy_source_contract() {
        for tool in CliTool::ALL {
            let adapter = FixtureSourceAdapter::new(tool);
            let (sessions, report) = adapter
                .list_sessions_with_report("included_fixture_snapshot", "fixture contract")
                .expect("sessions and report");

            assert_eq!(report.cli, tool);
            assert_eq!(report.provenance, SourceProvenance::Fixture);
            assert!(report.active);
            assert_eq!(report.session_count, sessions.len());
            assert_eq!(report.scan_entry_count, sessions.len());
            assert!(!report.scan_truncated);
            assert_eq!(report.capabilities.version, 1);
            assert_eq!(
                report.capabilities.local_store.status,
                super::super::model::SourceCapabilityStatus::Available
            );
            assert_eq!(
                report.capabilities.native_handoff.status,
                super::super::model::SourceCapabilityStatus::Unavailable
            );
            assert_eq!(
                report.fidelity.status,
                super::super::model::SourceFidelityStatus::Fallback
            );
            assert_eq!(report.fidelity.primary_surface, "embedded_fixture");
            assert!(
                report
                    .store_path
                    .as_deref()
                    .unwrap_or_default()
                    .contains("fixtures/")
            );

            for session in sessions {
                assert_eq!(session.cli, tool);
                assert_eq!(session.source_provenance, SourceProvenance::Fixture);
                assert!(
                    session
                        .source_path
                        .as_deref()
                        .unwrap_or_default()
                        .contains("fixtures/")
                );
                assert!(!session.id.trim().is_empty());
                assert!(!session.title.trim().is_empty());
                assert!(!session.updated_at.trim().is_empty());
                assert_eq!(session.runtime_status, SessionRuntimeStatus::Unknown);
                assert!(
                    session
                        .runtime_reason
                        .as_deref()
                        .is_some_and(|reason| !reason.trim().is_empty())
                );
                assert_eq!(session.parse_skip_count, 0);

                let timeline = adapter.load_timeline(&session.id).expect("timeline");
                assert_eq!(timeline.version, 1);
                assert_eq!(timeline.source_cli, tool);
                assert_eq!(timeline.source_session, session.id);
                assert!(session.event_count >= timeline.events.len());
                assert!(timeline.events.iter().any(|event| {
                    matches!(
                        event.kind,
                        super::super::model::TimelineKind::User
                            | super::super::model::TimelineKind::RewindPoint
                    )
                }));

                let mut ids = BTreeSet::new();
                for event in &timeline.events {
                    assert!(
                        ids.insert(event.id.as_str()),
                        "duplicate event id {}",
                        event.id
                    );
                    assert!(event.id.starts_with("evt-"));
                    assert!(!event.time.trim().is_empty());
                    assert!(!event.title.trim().is_empty());
                }
            }
        }
    }
}
