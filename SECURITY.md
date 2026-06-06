# Security Policy

Moonbox handles local session metadata and will eventually read session logs,
tool traces, Work Capsules, and launcher configuration. Treat those artifacts
as sensitive.

## Supported Versions

Moonbox has not shipped a stable release yet. Security fixes should target the
latest development branch and the next tagged release once release branches
exist.

| Version | Supported |
| --- | --- |
| Unreleased | Yes |

## Reporting a Vulnerability

Until GitHub private vulnerability reporting is enabled for this repository,
open a minimal public issue that says a security report is available, without
including secrets, logs, tokens, session contents, or exploit details. The
maintainer will provide a private channel for the full report.

Reports should include:

- Affected command or workflow.
- Expected and actual behavior.
- Minimal reproduction steps.
- Whether local session contents, credentials, or tool outputs can be exposed.
- Any affected platform details.

## Sensitive Data Rules

- Do not paste real session logs, provider tokens, API keys, OAuth tokens,
  cookies, private file paths with secrets, or customer data into issues or PRs.
- Redact Work Capsules before sharing if they contain private project context.
- Prefer small synthetic fixtures for tests.
- Do not add telemetry or remote network calls without explicit design review
  and documentation.
