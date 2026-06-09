use super::model::{
    CliTool, SourceCapabilities, SourceCapability, SourceCapabilityStatus, SourceProvenance,
};

pub fn source_capabilities(tool: CliTool, provenance: SourceProvenance) -> SourceCapabilities {
    match provenance {
        SourceProvenance::Missing => missing_capabilities(),
        SourceProvenance::Fixture => fixture_capabilities(tool),
        SourceProvenance::Real => real_capabilities(tool),
    }
}

fn missing_capabilities() -> SourceCapabilities {
    SourceCapabilities {
        local_store: cap(
            SourceCapabilityStatus::Unavailable,
            "source store is not present in the isolated home",
        ),
        rich_local_rpc: cap(
            SourceCapabilityStatus::Unavailable,
            "no source adapter is active",
        ),
        cloud_metadata: cap(
            SourceCapabilityStatus::Unavailable,
            "no source adapter is active",
        ),
        deep_link: cap(
            SourceCapabilityStatus::Unavailable,
            "no source adapter is active",
        ),
        export_search: cap(
            SourceCapabilityStatus::Unavailable,
            "no source adapter is active",
        ),
        remote_control: cap(
            SourceCapabilityStatus::Unavailable,
            "no source adapter is active",
        ),
        fork_resume: cap(
            SourceCapabilityStatus::Unavailable,
            "no session can be resumed",
        ),
        native_handoff: cap(
            SourceCapabilityStatus::Unavailable,
            "no native handoff is registered",
        ),
        ..SourceCapabilities::default()
    }
}

fn fixture_capabilities(tool: CliTool) -> SourceCapabilities {
    SourceCapabilities {
        local_store: cap(
            SourceCapabilityStatus::Available,
            format!("{tool} fixture corpus is available for non-executing tests"),
        ),
        rich_local_rpc: cap(
            SourceCapabilityStatus::Unavailable,
            "fixtures do not expose provider RPC surfaces",
        ),
        cloud_metadata: cap(
            SourceCapabilityStatus::Unavailable,
            "fixtures do not include cloud metadata",
        ),
        deep_link: cap(
            SourceCapabilityStatus::Unavailable,
            "fixtures do not open provider apps or deep links",
        ),
        export_search: cap(
            SourceCapabilityStatus::Unavailable,
            "fixtures use bounded local JSON, not provider export/search",
        ),
        remote_control: cap(
            SourceCapabilityStatus::Unavailable,
            "fixtures never drive a live provider runtime",
        ),
        fork_resume: cap(
            SourceCapabilityStatus::Unavailable,
            "fixtures are non-executing and cannot resume sessions",
        ),
        native_handoff: cap(
            SourceCapabilityStatus::Unavailable,
            "no native handoff is registered",
        ),
        ..SourceCapabilities::default()
    }
}

fn real_capabilities(tool: CliTool) -> SourceCapabilities {
    match tool {
        CliTool::Codex => SourceCapabilities {
            local_store: cap(
                SourceCapabilityStatus::Available,
                "read-only state_5.sqlite thread index plus rollout JSONL fallback",
            ),
            rich_local_rpc: cap(
                SourceCapabilityStatus::Unavailable,
                "Codex app-server support is implemented but not configured; set MOONBOX_CODEX_APP_SERVER_FIXTURE or MOONBOX_CODEX_APP_SERVER_PROXY=1 to opt in",
            ),
            cloud_metadata: cap(
                SourceCapabilityStatus::Unknown,
                "Codex cloud task metadata is modeled separately and is not mixed into local threads",
            ),
            deep_link: cap(
                SourceCapabilityStatus::Available,
                "open-app can preview codex://threads/<id> deep links without launching",
            ),
            export_search: cap(
                SourceCapabilityStatus::Unknown,
                "provider export/search surface is not verified",
            ),
            remote_control: cap(
                SourceCapabilityStatus::Unavailable,
                "Moonbox does not start Codex remote-control or app-server daemons",
            ),
            fork_resume: cap(
                SourceCapabilityStatus::Available,
                "original resume command can target codex resume <session>",
            ),
            native_handoff: cap(
                SourceCapabilityStatus::Unavailable,
                "no native handoff is registered",
            ),
            ..SourceCapabilities::default()
        },
        CliTool::Claude => SourceCapabilities {
            local_store: cap(
                SourceCapabilityStatus::Available,
                "read-only history.jsonl and project transcript JSONL stores remain the local resume baseline",
            ),
            rich_local_rpc: cap(
                SourceCapabilityStatus::Available,
                "captured Claude stream-json/SDK init and result metadata is parsed when present; Moonbox does not invoke Claude",
            ),
            cloud_metadata: cap(
                SourceCapabilityStatus::Unknown,
                "cloud metadata is not probed or mixed into local Claude resume rows",
            ),
            deep_link: cap(
                SourceCapabilityStatus::Unknown,
                "provider deep-link support is not verified",
            ),
            export_search: cap(
                SourceCapabilityStatus::Unknown,
                "provider export/search surface is not verified",
            ),
            remote_control: cap(
                SourceCapabilityStatus::Unavailable,
                "remote and remote-control surfaces are recognized as separate surfaces but are not launched, probed, or merged into local resume rows",
            ),
            fork_resume: cap(
                SourceCapabilityStatus::Available,
                "original resume command can target claude --resume <session>; fork parent metadata is parsed when present",
            ),
            native_handoff: cap(
                SourceCapabilityStatus::Unavailable,
                "no native handoff is registered",
            ),
            ..SourceCapabilities::default()
        },
        CliTool::Hermes => SourceCapabilities {
            local_store: cap(
                SourceCapabilityStatus::Available,
                "read-only Hermes state.db plus local registry supplements",
            ),
            rich_local_rpc: cap(
                SourceCapabilityStatus::Planned,
                "Hermes gateway all-source inventory is planned for M64",
            ),
            cloud_metadata: cap(
                SourceCapabilityStatus::Planned,
                "gateway platform and source metadata are planned for M64",
            ),
            deep_link: cap(
                SourceCapabilityStatus::Unknown,
                "provider deep-link support is not verified",
            ),
            export_search: cap(
                SourceCapabilityStatus::Planned,
                "Hermes export/stats/search integration is planned for M65",
            ),
            remote_control: cap(
                SourceCapabilityStatus::Unknown,
                "live runtime control is not probed by the current adapter",
            ),
            fork_resume: cap(
                SourceCapabilityStatus::Available,
                "original resume command can target hermes --resume <session>",
            ),
            native_handoff: cap(
                SourceCapabilityStatus::Unavailable,
                "no native handoff is registered",
            ),
            ..SourceCapabilities::default()
        },
    }
}

fn cap(status: SourceCapabilityStatus, detail: impl Into<String>) -> SourceCapability {
    SourceCapability {
        status,
        detail: detail.into(),
    }
}
