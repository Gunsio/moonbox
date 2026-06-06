use super::{
    adapter::{SourceAdapter, collect_sessions},
    error::CoreError,
    fixture::FixtureSourceAdapter,
    model::{CanonicalTimeline, CliTool, SessionSummary},
};

#[cfg(not(test))]
use super::codex::CodexSourceAdapter;

pub fn list_sessions() -> Result<Vec<SessionSummary>, CoreError> {
    let adapters = runtime_adapters();
    let adapter_refs = adapters
        .iter()
        .map(|adapter| adapter.as_ref())
        .collect::<Vec<_>>();
    collect_sessions(&adapter_refs).map_err(CoreError::from)
}

pub fn find_session(session_id: &str) -> Result<Option<SessionSummary>, CoreError> {
    for adapter in runtime_adapters() {
        if let Some(session) = adapter.find_session(session_id)? {
            return Ok(Some(session));
        }
    }
    Ok(None)
}

pub fn load_timeline(session: &SessionSummary) -> Result<CanonicalTimeline, CoreError> {
    for adapter in runtime_adapters() {
        if adapter.tool() != session.cli {
            continue;
        }
        return adapter.load_timeline(&session.id).map_err(CoreError::from);
    }

    FixtureSourceAdapter::new(session.cli)
        .load_timeline(&session.id)
        .map_err(CoreError::from)
}

#[cfg(test)]
fn fixture_adapters() -> Vec<Box<dyn SourceAdapter>> {
    CliTool::ALL
        .into_iter()
        .map(|tool| Box::new(FixtureSourceAdapter::new(tool)) as Box<dyn SourceAdapter>)
        .collect()
}

#[cfg(test)]
fn runtime_adapters() -> Vec<Box<dyn SourceAdapter>> {
    fixture_adapters()
}

#[cfg(not(test))]
fn runtime_adapters() -> Vec<Box<dyn SourceAdapter>> {
    let mut adapters: Vec<Box<dyn SourceAdapter>> = Vec::new();
    if let Some(codex) =
        CodexSourceAdapter::from_default_home().filter(|adapter| adapter.has_session_store())
    {
        adapters.push(Box::new(codex));
    } else {
        adapters.push(Box::new(FixtureSourceAdapter::new(CliTool::Codex)));
    }

    adapters.push(Box::new(FixtureSourceAdapter::new(CliTool::Claude)));
    adapters.push(Box::new(FixtureSourceAdapter::new(CliTool::Hermes)));
    adapters
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_uses_stable_fixture_data() {
        let sessions = list_sessions().expect("sessions");

        assert_eq!(sessions.len(), 3);
        assert_eq!(sessions[0].id, "codex-cxcp-design");
    }

    #[test]
    fn test_registry_loads_fixture_timeline() {
        let session = list_sessions()
            .expect("sessions")
            .into_iter()
            .find(|session| session.id == "codex-cxcp-design")
            .expect("codex session");

        let timeline = load_timeline(&session).expect("timeline");

        assert_eq!(timeline.source_session, session.id);
        assert!(!timeline.events.is_empty());
    }

    #[test]
    fn test_registry_finds_fixture_session() {
        let session = find_session("codex-cxcp-design")
            .expect("find result")
            .expect("session");

        assert_eq!(session.cli, CliTool::Codex);
    }
}
