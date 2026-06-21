use std::{env, fs, path::PathBuf, sync::OnceLock};

use serde::Deserialize;

use super::{
    config::{self, ContextModelConfig},
    model::CliTool,
};

const DEFAULT_QUALITY_CLIFF_TOKENS: usize = 120_000;
const CLAUDE_GPT_55_WINDOW_TOKENS: usize = 1_000_000;
const CLAUDE_GPT_55_QUALITY_CLIFF_TOKENS: usize = 500_000;
const CLAUDE_LONG_CONTEXT_WINDOW_TOKENS: usize = 1_000_000;
const CLAUDE_LONG_CONTEXT_QUALITY_CLIFF_TOKENS: usize = 500_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelContextResolution {
    pub window_tokens: usize,
    pub quality_cliff_tokens: Option<usize>,
    pub source: String,
}

#[derive(Debug, Default, Deserialize)]
struct CodexModelsCache {
    #[serde(default)]
    models: Vec<CodexModelEntry>,
}

#[derive(Debug, Deserialize)]
struct CodexModelEntry {
    slug: String,
    context_window: Option<usize>,
    effective_context_window_percent: Option<usize>,
}

static CODEX_MODELS_CACHE: OnceLock<Option<CodexModelsCache>> = OnceLock::new();

pub(crate) fn resolve_model_context_window(
    agent: CliTool,
    model: &str,
) -> Option<ModelContextResolution> {
    resolve_context_model_config(agent, model, &config::load_context_model_configs()).or_else(
        || match agent {
            CliTool::Codex => resolve_model_context_window_from_codex_cache(model),
            CliTool::Hermes => resolve_model_context_window_from_codex_cache(model)
                .or_else(|| resolve_anthropic_context_window(model)),
            CliTool::Claude => resolve_claude_context_window(model)
                .or_else(|| resolve_anthropic_context_window(model)),
        },
    )
}

fn resolve_context_model_config(
    agent: CliTool,
    model: &str,
    configs: &[ContextModelConfig],
) -> Option<ModelContextResolution> {
    let config = configs
        .iter()
        .find(|entry| context_model_config_matches(agent, model, entry))?;
    config
        .window_tokens
        .map(|window_tokens| ModelContextResolution {
            window_tokens,
            quality_cliff_tokens: config.quality_cliff_tokens,
            source: context_model_config_source(config),
        })
}

fn resolve_model_context_window_from_codex_cache(model: &str) -> Option<ModelContextResolution> {
    let cache = CODEX_MODELS_CACHE
        .get_or_init(|| codex_models_cache_path().and_then(read_codex_models_cache))
        .as_ref()?;
    resolve_model_context_window_from_cache(model, cache)
}

fn codex_models_cache_path() -> Option<PathBuf> {
    env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")))
        .map(|home| home.join("models_cache.json"))
}

fn read_codex_models_cache(path: PathBuf) -> Option<CodexModelsCache> {
    let json = fs::read_to_string(path).ok()?;
    serde_json::from_str(&json).ok()
}

fn resolve_model_context_window_from_cache(
    model: &str,
    cache: &CodexModelsCache,
) -> Option<ModelContextResolution> {
    let candidates = model_candidates(model);
    let entry = cache.models.iter().find(|entry| {
        let slug = entry.slug.to_ascii_lowercase();
        candidates
            .iter()
            .any(|candidate| candidate == &slug || candidate.starts_with(&format!("{slug}-")))
    })?;
    let raw_window = entry.context_window?;
    let window_tokens = entry
        .effective_context_window_percent
        .filter(|percent| *percent > 0 && *percent <= 100)
        .map(|percent| (raw_window * percent) / 100)
        .unwrap_or(raw_window);
    Some(ModelContextResolution {
        window_tokens,
        quality_cliff_tokens: Some(DEFAULT_QUALITY_CLIFF_TOKENS),
        source: format!("codex models cache {}", entry.slug),
    })
}

fn resolve_claude_context_window(model: &str) -> Option<ModelContextResolution> {
    let normalized = normalize_model(model);
    let decimalized = decimalize_gpt_model(&normalized);
    if [normalized.as_str(), decimalized.as_str()]
        .iter()
        .any(|candidate| candidate.starts_with("gpt-5.5"))
    {
        Some(ModelContextResolution {
            window_tokens: CLAUDE_GPT_55_WINDOW_TOKENS,
            quality_cliff_tokens: Some(CLAUDE_GPT_55_QUALITY_CLIFF_TOKENS),
            source: "moonbox context model default claude gpt-5.5".into(),
        })
    } else {
        None
    }
}

fn resolve_anthropic_context_window(model: &str) -> Option<ModelContextResolution> {
    let model = normalize_model(model);
    let (window_tokens, quality_cliff_tokens) = if model.contains("claude-fable-5")
        || model.contains("claude-mythos-5")
        || model.contains("claude-opus-4-8")
        || model.contains("claude-opus-4-7")
        || model.contains("claude-sonnet-4-6")
    {
        (
            CLAUDE_LONG_CONTEXT_WINDOW_TOKENS,
            Some(CLAUDE_LONG_CONTEXT_QUALITY_CLIFF_TOKENS),
        )
    } else if model.contains("claude-haiku-4-5")
        || model.contains("claude-opus-4")
        || model.contains("claude-sonnet-4")
        || model.contains("claude-3")
        || model.contains("claude-instant")
    {
        (200_000, Some(DEFAULT_QUALITY_CLIFF_TOKENS))
    } else {
        return None;
    };
    Some(ModelContextResolution {
        window_tokens,
        quality_cliff_tokens,
        source: "anthropic model context table".into(),
    })
}

fn context_model_config_matches(agent: CliTool, model: &str, config: &ContextModelConfig) -> bool {
    config.agent.is_none_or(|configured| configured == agent)
        && model_pattern_matches(config.model.trim(), model)
}

fn model_pattern_matches(pattern: &str, model: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    let pattern = normalize_model(pattern);
    let candidates = model_candidates(model);
    if let Some(prefix) = pattern.strip_suffix('*') {
        candidates
            .iter()
            .any(|candidate| candidate.starts_with(prefix))
    } else {
        candidates
            .iter()
            .any(|candidate| candidate == &pattern || candidate.starts_with(&format!("{pattern}-")))
    }
}

fn context_model_config_source(config: &ContextModelConfig) -> String {
    let scope = config
        .agent
        .map(|agent| format!("{} ", agent.id()))
        .unwrap_or_default();
    format!("moonbox config {scope}{}", config.model.trim())
}

fn model_candidates(model: &str) -> Vec<String> {
    let normalized = normalize_model(model);
    let decimalized = decimalize_gpt_model(&normalized);
    let mut candidates = vec![normalized.clone(), strip_snapshot_suffix(&normalized)];
    if decimalized != normalized {
        candidates.push(decimalized.clone());
        candidates.push(strip_snapshot_suffix(&decimalized));
    }
    candidates.sort();
    candidates.dedup();
    candidates
}

fn normalize_model(model: &str) -> String {
    model.trim().to_ascii_lowercase()
}

fn strip_snapshot_suffix(model: &str) -> String {
    let mut parts = model.rsplitn(4, '-');
    let day = parts.next();
    let month = parts.next();
    let year = parts.next();
    let prefix = parts.next();
    if year.is_some_and(|value| value.len() == 4 && value.chars().all(|char| char.is_ascii_digit()))
        && month.is_some_and(|value| {
            value.len() == 2 && value.chars().all(|char| char.is_ascii_digit())
        })
        && day.is_some_and(|value| {
            value.len() == 2 && value.chars().all(|char| char.is_ascii_digit())
        })
    {
        prefix.unwrap_or(model).to_owned()
    } else {
        model.to_owned()
    }
}

fn decimalize_gpt_model(model: &str) -> String {
    let Some(rest) = model.strip_prefix("gpt-") else {
        return model.to_owned();
    };
    let mut pieces = rest.splitn(3, '-');
    let major = pieces.next().unwrap_or_default();
    let minor = pieces.next().unwrap_or_default();
    if major.chars().all(|char| char.is_ascii_digit())
        && minor.chars().all(|char| char.is_ascii_digit())
    {
        match pieces.next() {
            Some(tail) => format!("gpt-{major}.{minor}-{tail}"),
            None => format!("gpt-{major}.{minor}"),
        }
    } else {
        model.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_versioned_gpt_model_from_codex_cache() {
        let cache = CodexModelsCache {
            models: vec![CodexModelEntry {
                slug: "gpt-5.5".into(),
                context_window: Some(272_000),
                effective_context_window_percent: Some(95),
            }],
        };

        let resolved =
            resolve_model_context_window_from_cache("gpt-5.5-2026-04-24", &cache).unwrap();

        assert_eq!(resolved.window_tokens, 258_400);
        assert_eq!(
            resolved.quality_cliff_tokens,
            Some(DEFAULT_QUALITY_CLIFF_TOKENS)
        );
        assert_eq!(resolved.source, "codex models cache gpt-5.5");
    }

    #[test]
    fn resolves_dash_separated_gpt_model_from_codex_cache() {
        let cache = CodexModelsCache {
            models: vec![CodexModelEntry {
                slug: "gpt-5.5".into(),
                context_window: Some(272_000),
                effective_context_window_percent: Some(95),
            }],
        };

        let resolved = resolve_model_context_window_from_cache("gpt-5-5", &cache).unwrap();

        assert_eq!(resolved.window_tokens, 258_400);
    }

    #[test]
    fn keeps_claude_gpt_model_on_claude_context_default() {
        let resolved = resolve_model_context_window(CliTool::Claude, "gpt-5.5-2026-04-24").unwrap();

        assert_eq!(resolved.window_tokens, CLAUDE_GPT_55_WINDOW_TOKENS);
        assert_eq!(
            resolved.quality_cliff_tokens,
            Some(CLAUDE_GPT_55_QUALITY_CLIFF_TOKENS)
        );
        assert!(resolved.source.contains("claude gpt-5.5"));
    }

    #[test]
    fn matches_context_model_config_by_agent_and_model_pattern() {
        let configs = vec![ContextModelConfig {
            agent: Some(CliTool::Claude),
            model: "gpt-5.5*".into(),
            window_tokens: Some(900_000),
            quality_cliff_tokens: Some(450_000),
        }];

        let resolved =
            resolve_context_model_config(CliTool::Claude, "gpt-5.5-2026-04-24", &configs).unwrap();

        assert_eq!(resolved.window_tokens, 900_000);
        assert_eq!(resolved.quality_cliff_tokens, Some(450_000));
        assert!(resolved.source.contains("claude gpt-5.5*"));
        assert!(resolve_context_model_config(CliTool::Codex, "gpt-5.5", &configs).is_none());
    }

    #[test]
    fn resolves_current_claude_model_window() {
        let resolved = resolve_anthropic_context_window("claude-sonnet-4-6").unwrap();

        assert_eq!(resolved.window_tokens, 1_000_000);
        assert_eq!(resolved.quality_cliff_tokens, Some(500_000));
    }

    #[test]
    fn resolves_real_claude_opus_47_long_context_window() {
        let resolved = resolve_anthropic_context_window("claude-opus-4-7").unwrap();

        assert_eq!(resolved.window_tokens, 1_000_000);
        assert_eq!(resolved.quality_cliff_tokens, Some(500_000));
    }
}
