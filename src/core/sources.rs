use super::{
    adapter::{SourceAdapter, collect_sessions},
    error::CoreError,
    fixture::FixtureSourceAdapter,
    model::{CanonicalTimeline, CliTool, SessionSummary},
};

#[cfg(not(test))]
use super::claude::ClaudeSourceAdapter;
#[cfg(not(test))]
use super::codex::CodexSourceAdapter;
#[cfg(not(test))]
use super::hermes::HermesSourceAdapter;

pub const SESSION_MODE_ENV: &str = "MOONBOX_SESSION_MODE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    Auto,
    Fixture,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionModeState {
    pub mode: SessionMode,
    pub raw: Option<String>,
    pub valid: bool,
}

impl SessionMode {
    pub fn id(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Fixture => "fixture",
        }
    }
}

pub fn session_mode_state() -> SessionModeState {
    parse_session_mode(std::env::var(SESSION_MODE_ENV).ok().as_deref())
}

pub fn session_mode() -> SessionMode {
    session_mode_state().mode
}

pub fn list_sessions() -> Result<Vec<SessionSummary>, CoreError> {
    let adapters = runtime_adapters();
    let adapter_refs = adapters
        .iter()
        .map(|adapter| adapter.as_ref())
        .collect::<Vec<_>>();
    collect_sessions(&adapter_refs).map_err(CoreError::from)
}

pub fn find_session(session_id: &str) -> Result<Option<SessionSummary>, CoreError> {
    let adapters = runtime_adapters();
    let preferred_tool = preferred_tool_for_session_id(session_id);

    if let Some(preferred_tool) = preferred_tool {
        for adapter in adapters
            .iter()
            .filter(|adapter| adapter.tool() == preferred_tool)
        {
            if let Some(session) = adapter.find_session(session_id)? {
                return Ok(Some(session));
            }
        }
    }

    let adapter_refs = adapters
        .iter()
        .map(|adapter| adapter.as_ref())
        .collect::<Vec<_>>();
    if let Some(session) = collect_sessions(&adapter_refs)?
        .into_iter()
        .find(|session| session.id == session_id)
    {
        return Ok(Some(session));
    }

    for adapter in adapters
        .iter()
        .filter(|adapter| Some(adapter.tool()) != preferred_tool)
    {
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
    if session_mode() == SessionMode::Fixture {
        return fixture_adapters();
    }

    let mut adapters: Vec<Box<dyn SourceAdapter>> = Vec::new();
    if let Some(codex) =
        CodexSourceAdapter::from_default_home().filter(|adapter| adapter.has_session_store())
    {
        adapters.push(Box::new(codex));
    } else {
        adapters.push(Box::new(FixtureSourceAdapter::new(CliTool::Codex)));
    }

    if let Some(claude) =
        ClaudeSourceAdapter::from_default_home().filter(|adapter| adapter.has_session_store())
    {
        adapters.push(Box::new(claude));
    } else {
        adapters.push(Box::new(FixtureSourceAdapter::new(CliTool::Claude)));
    }

    if let Some(hermes) =
        HermesSourceAdapter::from_default_home().filter(|adapter| adapter.has_session_store())
    {
        adapters.push(Box::new(hermes));
    } else {
        adapters.push(Box::new(FixtureSourceAdapter::new(CliTool::Hermes)));
    }
    adapters
}

fn parse_session_mode(raw: Option<&str>) -> SessionModeState {
    let Some(raw) = raw.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return SessionModeState {
            mode: SessionMode::Auto,
            raw: None,
            valid: true,
        };
    };
    let normalized = raw.to_ascii_lowercase();
    match normalized.as_str() {
        "auto" | "real" => SessionModeState {
            mode: SessionMode::Auto,
            raw: Some(raw.into()),
            valid: true,
        },
        "fixture" | "fixtures" | "demo" => SessionModeState {
            mode: SessionMode::Fixture,
            raw: Some(raw.into()),
            valid: true,
        },
        _ => SessionModeState {
            mode: SessionMode::Auto,
            raw: Some(raw.into()),
            valid: false,
        },
    }
}

fn preferred_tool_for_session_id(session_id: &str) -> Option<CliTool> {
    if session_id.starts_with("codex-") || session_id.starts_with("rollout-") {
        return Some(CliTool::Codex);
    }
    if session_id.starts_with("claude-") || looks_like_uuid(session_id) {
        return Some(CliTool::Claude);
    }
    if session_id.starts_with("hermes-")
        || session_id.starts_with("cron_")
        || looks_like_hermes_timestamp_id(session_id)
    {
        return Some(CliTool::Hermes);
    }
    None
}

fn looks_like_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for index in [8, 13, 18, 23] {
        if bytes[index] != b'-' {
            return false;
        }
    }
    bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| matches!(index, 8 | 13 | 18 | 23) || byte.is_ascii_hexdigit())
}

fn looks_like_hermes_timestamp_id(value: &str) -> bool {
    let Some(prefix) = value.get(..9) else {
        return false;
    };
    let (date, separator) = prefix.split_at(8);
    separator == "_" && date.bytes().all(|byte| byte.is_ascii_digit())
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

    #[test]
    fn session_id_heuristics_prioritize_expensive_lookup() {
        assert_eq!(
            preferred_tool_for_session_id("20260605_142114_9609f348"),
            Some(CliTool::Hermes)
        );
        assert_eq!(
            preferred_tool_for_session_id("cron_2ceb18b7c4db_20260605_190259"),
            Some(CliTool::Hermes)
        );
        assert_eq!(
            preferred_tool_for_session_id("09606d04-f303-418a-ae24-8921389bbe54"),
            Some(CliTool::Claude)
        );
        assert_eq!(
            preferred_tool_for_session_id("codex-cxcp-design"),
            Some(CliTool::Codex)
        );
    }

    #[test]
    fn session_mode_parser_accepts_fixture_aliases_and_warns_on_invalid_values() {
        assert_eq!(parse_session_mode(None).mode, SessionMode::Auto);
        assert_eq!(parse_session_mode(Some("auto")).mode, SessionMode::Auto);
        assert_eq!(parse_session_mode(Some("real")).mode, SessionMode::Auto);
        assert_eq!(
            parse_session_mode(Some("fixture")).mode,
            SessionMode::Fixture
        );
        assert_eq!(
            parse_session_mode(Some("fixtures")).mode,
            SessionMode::Fixture
        );
        assert_eq!(parse_session_mode(Some("demo")).mode, SessionMode::Fixture);

        let invalid = parse_session_mode(Some("recent"));
        assert_eq!(invalid.mode, SessionMode::Auto);
        assert!(!invalid.valid);
    }
}
