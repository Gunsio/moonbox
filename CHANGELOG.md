# Changelog

All notable changes to Moonbox will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project uses semantic versioning once tagged releases start.

## [Unreleased]

### Added

- Root agent operating rules covering one-milestone-per-PR governance, fixture-safe
  tests, read-only source stores, and documentation sync expectations.
- Rust + Ratatui TUI workbench with Vim-style navigation.
- Global session list with source tags, search, source filtering, original
  session preview, and target picker.
- Canonical Timeline, Work Capsule, compile request, compile output, launch
  plan, and verification report models.
- File-backed fixture adapters for Codex, Claude, and Hermes.
- Real read-only Codex adapter for the `~/.codex/state_5.sqlite` resume thread
  index, with rollout fallback from `~/.codex/sessions`.
- Real read-only Claude adapter for the `~/.claude/history.jsonl` resume index,
  with timeline and detail hydration from `~/.claude/projects`.
- Real read-only Hermes adapter for `~/.hermes/state.db`, listing all
  non-archived Hermes sources by default while keeping explicit ID lookup
  available across the wider Hermes store.
- Fallible `SourceAdapter` discovery.
- Replaceable `CapsuleCompiler` trait with fixture and process-backed compilers.
- External compiler runner using JSON stdin/stdout, timeout handling, and
  structured process errors.
- Guarded target launcher execution through `launch --execute`.
- Target-specific Codex, Claude, and Hermes command generation with structured
  `target_command` plan output.
- Guarded original-session execution through `open --execute`.
- Structured original open plan output through `open --json`.
- `action` discriminators in dry-run `open --json`, `open-app --json`, and
  `launch --json` output, using `original_resume`, `app_deep_link`, and
  `target_handoff`.
- `moonbox actions --session <id> --json` reports a read-only action
  availability model shared by CLI and TUI for Inspect, Resume, Jump, Fork,
  Handoff, Yank, and Archive; TUI `o` opens that action menu for the selected
  session.
- `moonbox setup install codex-sdk|claude-sdk|matt-handoff` installs supported
  runner SDK and community handoff-skill setup targets. Skill Picker, Launch,
  and failed Handoff Review panels can suspend Moonbox, run the setup command,
  then refresh catalog state on return.
- TUI action menu `New Session` starts the selected target CLI from the first
  user prompt in the source session, carrying image and attachment references as
  explicit path text instead of claiming provider-native image transfer.
- TUI action menu `Lark Doc` runs from `o Enter`: it uses the same loading and
  review flow as normal Handoff, shows the generated handoff Markdown first,
  then creates and opens a Feishu/Lark document when `Enter` is pressed on the
  preview. It suspends Moonbox for document creation or `lark-cli`
  install/update when needed. The same capability is available as
  `moonbox export --to lark --mode handoff`, which previews and, with
  `--execute`, creates the document through `lark-cli docs +create
  --api-version v2 --as user`.
- TUI action menu `Resume` now requires the explicit `r` shortcut; pressing
  `Enter` on Resume stays inside Moonbox, so `o Enter` cannot accidentally open
  the original provider session.
- TUI `y` opens a Yank panel with copy-only actions for first user input, last
  AI output, Session ID, ready handoff text, and compact portable JSON, while
  keeping provider source stores read-only and avoiding target session launch.
- TUI action menu `Fork` now calls provider-native session fork for Codex and
  Claude, keeps Hermes explicitly unavailable until it exposes a native fork
  command, and records launches as `native_fork` instead of `original_resume`.
- TUI archive overlay stores Archive / Unarchive state in Moonbox config,
  hides archived sessions by default, adds an `Archived` filter for search and
  restore, and gives the selected row short feedback before the list compacts.
- TUI header and loading screen show the running Moonbox package version so
  stale installed binaries are visible during local validation.
- TUI action menu localizes action labels and availability reasons in
  Simplified Chinese.
- First-class `moon` binary alias installed alongside `moonbox`.
- Homebrew tap prerelease distribution workflow for `Gunsio/tap`, including
  tagged release artifact checksums and formula verification guidance.
- Homebrew install guidance now includes Homebrew 5 tap trust, and the tap
  formula uses Apple Silicon bottles for Tahoe and Sequoia so users do not need
  Rust, LLVM, or current Apple Command Line Tools for the common install path.
- Version metadata and release/Homebrew templates now target the v0.1.1
  prerelease for the M84/M85 TUI polish rollout.
- TUI original resume now uses a suspend-and-return flow by default: `enter`
  leaves the alternate screen, runs the selected source CLI on the real
  terminal, restores Moonbox after exit, reloads the selected session timeline,
  and keeps `MOONBOX_RESUME_MODE=exec` for the older one-way process replacement
  behavior.
- TUI session inventory now uses user-readable context size language. The list
  prioritizes token count and raw source size instead of exposing internal
  `events` terminology, while Session Details labels parsed counts as
  `Timeline Items`. `SessionSummary` now carries a serde-default
  `source_size_bytes` field populated for JSONL-backed Codex and Claude
  sessions, including Codex SQLite rows that point at rollout JSONL.
- TUI Session Details now exposes value-ranked Session Anatomy for selected
  local Codex and Claude JSONL sessions. Normal Details show the highest-value
  continuation/trust/debug/trace signals, while zoomed Details expands bounded
  size, event, content, compact-frontier, token, sidecar, and analyzer-note
  sections. Large sources are tail-sampled and labeled explicitly instead of
  blocking the UI on full-file scans.
- TUI Data Space Picker opened with `d`, showing Local and SSH spaces explicitly
  saved in Moonbox config with current/selected state, target address,
  configuration source, and the read-only inventory command before loading.
- TUI Data Space Picker `n` / `a` add-flow for SSH hosts now accepts pasted
  `ssh user@host`, `ssh://user@host:22`, and OpenSSH `Host` blocks, writing
  Moonbox `ssh_hosts` config without modifying a user's OpenSSH config.
- TUI Data Space Picker `x` deletes saved SSH spaces from Moonbox config after a
  second confirmation keypress; OpenSSH aliases discovered by `moonbox ssh` are
  no longer auto-loaded into the picker.
- SSH data-space execution now uses the host, user, port, and identity file
  saved in Moonbox config instead of assuming the saved name is an OpenSSH alias.
- SSH data-space inventory now searches common user install paths and falls back
  from `moonbox` to `moon` before reporting that the remote inventory command is
  missing.
- SSH data-space session selection now hydrates the selected remote timeline via
  a read-only remote `compile-request --json` dry run, so SSH inventories do not
  render as empty summary-only timelines.
- Data-space load failures now reopen the picker with a red error block and
  matching red status styling instead of relying on a muted footer message.
- TUI header now marks remote inventory explicitly as `Data: SSH: <host>` so SSH
  data spaces are not confused with local session stores.
- SSH data-space sessions are read-only in the TUI: `enter` opens the target
  handoff flow instead of trying local original resume, and the `o` action menu
  blocks Resume while keeping Handoff available.
- TUI Handoff Review now uses `r` as the explicit local target launch action;
  `enter` is review-only, `y` copies the actual target command, and target
  completion returns to Moonbox with visible run-again/copy/back actions.
- TUI Handoff Review preparation now runs in the background with a cancellable
  loading panel, opens at the bottom action area, and supports `gg` / `G` jumps
  for long reviews.
- Real-session target handoff reviews now disable `r` before spawning when the
  built-in draft compiler is still in use; users can copy the command or
  configure an external compiler instead of hitting a late launcher error.
- Handoff Review and target launch notices now show concise target command
  summaries, keeping the full prompt available for copy/execution without
  flooding the modal.
- Agent-backed handoff compiler catalog entries now discover local generic
  handoff skills and expose Codex / Claude runner choices in the same Skill
  Picker. Codex uses the official `openai-codex` SDK bridge with
  `Sandbox.read_only`, while Claude uses a temporary local-plugin bridge for the
  Claude Agent SDK and filters execution to the selected `plugin:skill`.
- Agent runner preflight now distinguishes installed Codex / Claude CLIs from
  missing Python SDK modules, scans common Python interpreters before warning,
  and keeps the Handoff Review failure panel visible with checked interpreters,
  install commands, and `MOONBOX_*_SDK_PYTHON` override hints.
- Agent runner preflight now also discovers Moonbox-managed SDK venvs at
  `~/.moonbox/venvs/codex-sdk/bin/python` and
  `~/.moonbox/venvs/claude-sdk/bin/python`, and recommends venv setup commands
  instead of direct installs into externally managed Homebrew Python.
- Agent handoff generation now sends a bounded context pack with rewind-window
  bounds, session index, compact frontier, tool/approval evidence, file changes,
  attachments, raw references, and redaction details instead of handing the
  runner an unbounded transcript slice.
- Empty agent handoff artifacts now fail validation before they can enter the
  runnable Handoff Review path.
- Handoff Review generation now compiles the selected compiler in the
  background after loading the source timeline, reports queued /
  preparing_context / starting_runner / running_skill / verifying progress, and
  keeps the in-process job alive when the review panel is hidden.
- Handoff generation is now strictly user-confirmed from the Launch picker:
  Skill Picker `Enter` only saves the chosen handoff skill, failed Review panels
  require explicit `r` retry, ready artifacts are reused instead of regenerated,
  and Moonbox cleans the SDK runner process group after success or timeout so
  orphaned agent app-server processes cannot keep burning tokens.
- Session browsing now keeps startup inventory-only, changes selection without
  rebuilding `WorkbenchData`, then starts the selected session's bounded
  timeline preview only after a short navigation debounce. Plain navigation,
  filter changes, and live search never compile handoff output, run SDK
  workers, or start immediate timeline IO; Review remains the explicit boundary
  for AI handoff generation.
- Timeline preview, selected-session load, and Handoff Review pending states
  now render an animated `| / - \` spinner so long background work no longer
  looks frozen.
- `moonbox-handoff` is now bundled as a first-party handoff prompt in the same
  skill-first catalog and Handoff Review runner path as community skills,
  without writing to user skill homes.
- Moonbox-generated handoff worker sessions are hidden from the main TUI
  inventory by default and reappear only when explicitly searched, keeping
  background handoff jobs from flooding the user's normal continuation list.
- Provider-injected control blocks such as `<skill>...</skill>` and
  `<turn_aborted>...</turn_aborted>` are no longer rendered as user timeline
  turns or used as session titles.
- Agent handoff runners now treat a closed child stdin after successful output
  as normal process completion, avoiding false `Broken pipe` failures while
  still surfacing timeouts, non-zero exits, and unreadable stdout.
- Skill Picker is now skill-first: Codex / Claude agent catalog rows collapse
  into one handoff skill choice, installed local skill paths or install sources
  are shown directly, and runner IDs / SDK package commands are kept out of the
  skill-selection surface.
- Opt-in hook event channel foundation through `moonbox hooks
  status/install/uninstall` and the silent `moonbox hook-event` handler.
  Provider config writes are preview-first, require `--apply`, preserve existing
  Claude/Codex hooks, remove only Moonbox-owned entries, report Codex feature
  gating and trust-review limits, and append fail-open JSONL events with
  cwd/tmux metadata to a size-bounded Moonbox spool.
- Hook-gated live session status and waiting queue in the TUI: when hooks are
  enabled, Moonbox replays and tails the local spool for `RUN` / `WAIT` / `IDLE`
  / `END` row badges, recent-action summaries, `Live on` / stale / error status,
  an SSH data-space unavailable indicator, and a compact `WAITING ON YOU` panel.
  Disabled hooks leave the Dashboard, Timeline, status bar, and Enter behavior
  unchanged. Codex hook injection now writes snake_case event names while
  uninstall/status still recognize legacy PascalCase Moonbox entries.
- Opt-in Smart Enter / tmux jump routing in the TUI. The setting lives behind
  Settings (`,`), remains disabled by default even after hooks are installed,
  previews the selected session's Enter route before saving, and only jumps when
  hook-captured tmux socket/pane metadata validates through `tmux list-panes`.
  Missing metadata, dead sessions, SSH data spaces, and tmux failures fall back
  to the guarded resume/handoff path without creating panes, sending input, or
  mutating source stores.
- TUI UI preferences in Settings: English is the default language, Simplified
  Chinese is optional, and Moonbox plus the four Luoshen product themes are
  backed by semantic color tokens and compact image-linked ASCII sigils: `~>`
  for startled swan flight, `S~` for the coursing dragon curve, `*o` for the
  radiant chrysanthemum bloom, and `/\` for the lush pine crown. Settings
  previews language and theme values before saving, supports reset and unsaved
  row indicators, persists only to Moonbox config, and keeps session
  transcripts, prompts, agent output, tool output, code, paths, cwd, branch
  names, metadata, and handoff content unmodified. Deprecated Tokyo Night /
  Gruvbox palettes stay in the reusable theme crate for compatibility but are no
  longer Moonbox Settings choices.
- M97 Luoshen TUI Theme Pack: Moonbox's semantic theme layer now lives in the
  reusable `moonbox-theme` workspace crate with stable theme ids, metadata,
  provenance, Ratatui adapters, image-linked ASCII sigils, and truecolor / ANSI
  / `NO_COLOR` fallback behavior. The TUI ships the first-party Luoshen family:
  Startled Swan, Coursing Dragon, Radiant Chrysanthemum, and Lush Pine.
- M99 v0.1.3 prerelease packaging: package metadata, lockfile metadata, release
  artifact staging examples, and Homebrew formula templates now target
  `0.1.3` / `v0.1.3` so M97 Luoshen themes and M98 skill-first handoff review
  can ship together through tagged GitHub release artifacts.
- M100 Exact Handoff Artifact Review: agent-backed Handoff Review now treats
  the generated Markdown file as the single source of truth. The review body is
  the full generated handoff document, while `Enter` / `r` starts the target
  agent with a concise handoff task note, artifact path, and source-session
  metadata. `y` copies the full handoff text, `p` copies the generated file
  path, and Moonbox runner, skill path, redaction, and bounded-context details
  move behind `d` instead of being mixed into the handoff body. Community skill
  artifacts under either the current `TMPDIR` or `/tmp` are accepted when they
  keep the Moonbox handoff filename prefix and `.md` extension.
- M101 v0.1.4 release packaging: package metadata, lockfile metadata, release
  artifact staging examples, and Homebrew formula templates now target
  `0.1.4` / `v0.1.4` so the M100 exact artifact review flow can ship through
  tagged GitHub release artifacts.
- SSH data-space selected-session details now use the remote
  `compile-request --json` response as the detail source, preserving bounded
  anatomy computed on the remote host instead of trying to read remote paths on
  the local machine. Older remote Moonbox binaries that omit anatomy now show a
  clear `remote-unavailable` fallback note in Zoom Details.
- Configurable compiler skill presets in `~/.config/moonbox/config.json`,
  including optional description, homepage, and GitHub stars metadata for the
  TUI Skill Picker.
- Compiler catalog output through `moonbox compilers`, including source,
  status, score, command, arguments, timeout, and quality reason.
- Hardened verification checks for Work Capsule version, required fields,
  handoff context, risk context, capsule size, handoff label markers, and
  execute-time target command preflight.
- Fixture-only replay evaluation through `moonbox replay-eval`, covering every
  Codex/Claude/Hermes source-target pair without scanning or opening local
  session stores.
- Synthetic fixture replay regressions for target mismatch, oversized capsule,
  and missing-tool preflight, with explicit scenario coverage in replay output.
- CI gates for fixture replay evaluation and `cargo package --locked`
  verification.
- Fixture-safe CLI smoke script for non-executing command surfaces, with source
  homes redirected away from real local session stores.
- Fixture-safe install smoke script that verifies `cargo install --path`
  produces working `moonbox` and `moon` executables, then exercises installed
  session listing, Doctor diagnostics, completion generation, and replay
  evaluation with source homes redirected away from real local session stores.
- Fixture-safe integration tests for public CLI contracts across `moonbox` and
  `moon` binaries.
- Documentation build gate with rustdoc warnings treated as errors.
- Full local quality gate script for patch hygiene plus CI/release checks.
- Cargo package hygiene gate that validates expected package contents and
  rejects editor backups, rejected patches, temporary files, and build
  directories before release packaging.
- Supply-chain gate with cargo-deny advisories, duplicate-version, license, and
  source checks.
- Shell completion generation for Bash, Zsh, Fish, PowerShell, and Elvish.
- Non-executing `moonbox doctor` diagnostics for config, session discovery,
  target binaries, and compiler catalog readiness.
- `MOONBOX_SESSION_MODE=fixture` to force embedded fixture sessions and disable
  real source-store discovery in demos, CI, and release smoke.
- TUI Doctor panel with `D` / `:doctor`, refresh, and JSON copy support for
  the same non-executing diagnostics.
- Fixture-safe TUI render regression tests for main, Doctor, and Launch views.
- Verifier-backed TUI target readiness explanation rows in the launch picker
  and Handoff Review.
- Documentation asset smoke coverage for README screenshot semantics,
  installation commands, and unpublished Homebrew wording.
- Hidden fixture-only `docs-snapshot` maintenance command that renders the real
  Ratatui Handoff Review buffer to SVG for the README screenshot asset.
- Draft Homebrew formula template plus a fixture-safe Homebrew docs smoke gate.
- Release artifact staging script that produces source, Cargo crate, and host
  binary archives, generated shell completions, `SHA256SUMS`, and
  `release-manifest.json` without publishing.
- Fixture-safe release artifact smoke gate that validates staged checksums,
  manifest metadata, and archive contents.
- Source filtering for the public `moonbox sessions` command through
  `--filter <source>`.
- Per-session source provenance fields in `moonbox sessions --json`:
  `source_provenance`, `source_path`, and `parse_skip_count`.
- Structured Doctor source adapter reports under `source_adapters`, including
  provenance, active/missing state, store path, session count, skipped record
  count, last indexed timestamp, and adapter filter status.
- Versioned source adapter capability reports for local store, rich local RPC,
  cloud metadata, deep links, export/search, remote control, fork/resume, and
  native handoff support, plus per-session `runtime_status` / `runtime_reason`
  fields that keep live runtime activity separate from `updated_at`.
- Source adapter fidelity reports with serde-default `status`,
  `primary_surface`, optional `fallback_surface`, and detail text, making
  full-fidelity, partial, fallback, and missing source surfaces explicit in
  `doctor --json`.
- Bounded JSONL real-store discovery through `MOONBOX_SESSION_SCAN_LIMIT`,
  with Doctor scan-cost fields for list limit, scan entry limit, visited entry
  count, and truncation state.
- Bounded JSONL session summary parsing through
  `MOONBOX_SESSION_SUMMARY_LINE_LIMIT`, so listed large sessions do not require
  full-file parsing before the TUI becomes usable.
- Bounded TUI timeline previews through `MOONBOX_TIMELINE_EVENT_LIMIT`, with a
  visible truncation marker for large sessions.
- Bounded timeline event body previews through
  `MOONBOX_TIMELINE_DETAIL_CHAR_LIMIT`, defaulting to 4000 characters so zoomed
  Timeline panels can show long-form context without unbounded transcript
  rendering.
- Animated TUI loading screen while source sessions are indexed before the
  workbench becomes interactive.
- Async selected-session preview hydration in the TUI, with stale-result
  protection for rapid navigation and loading guards before launch, verify,
  compile, rewind, or original-resume preview actions.
- Cached TUI session filters plus windowed session-list rendering, so large
  real indexes do not require re-filtering and re-formatting every row on every
  frame.
- Shared verifier policy for CLI and TUI launch validation.
- Real `--capsule` file parsing and target mismatch verification.
- README screenshot, installation notes, and Homebrew release planning docs.
- Production panic-boundary lint policy for non-test builds, denying `unsafe`,
  `unwrap()`, `expect()`, `panic!`, `todo!`, and `unimplemented!`.
- Minimal documented Rust library entrypoint through `moonbox::run()`, with
  CLI internals kept crate-private until a deliberate library API is stabilized.
- TUI real-vs-draft labeling that separates real source metadata from built-in
  Work Capsule draft guidance.
- Read-only SSH inventory through `moonbox ssh` / `moon ssh`, combining
  Moonbox `ssh_hosts` config entries with concrete OpenSSH `Host` aliases from
  `~/.ssh/config` or `MOONBOX_SSH_CONFIG` without opening remote connections.
- TUI data spaces backed by local session stores plus SSH spaces explicitly
  saved in Moonbox config, switchable with `{` / `}` without opening, resuming,
  or launching any remote session.
- Handoff Review target-input preview, including target program, cwd, argument
  count, and the exact prompt argument that will be passed to the target CLI.
- Verifier `compiler_mode` checks that mark built-in draft compilers as a
  warning for real source handoffs.
- Workspace continuation snapshots through `moonbox snapshot`, capturing git
  HEAD, branch, staged/unstaged/untracked paths, bounded diff previews, key
  project files, environment summary, and explicit test-command results without
  reading or opening source CLI sessions.
- Auditable Capsule source maps with `raw_source_map`, `raw_refs`, and
  `coverage` fields, enriched from the canonical timeline for built-in and
  external compiler output.
- Redaction policy reports on Capsule compile requests and Work Capsules,
  covering secret-like value scanning, sensitive path masking, event/file
  allowlists, prompt-injection warnings, and external compiler disclosure.
- Semantic verifier checks for raw source map consistency, compiler coverage
  gaps, todo/timeline event references, local file references, and
  patch-shaped diff evidence.
- Fixture adapter contract coverage for session summary fields, report metadata,
  timeline schema version, unique event ids, and user/rewind anchors.
- Continuation protocol plans in `launch --json`, covering explicit
  `prompt_only` target input, unsupported native Capsule import requests, and
  preview-only reversible branch/worktree workspace restore commands.
- `launch` and `verify` options for `--continuation` and
  `--workspace-restore`, with fixture-safe contract coverage proving unsupported
  import/restore requests are blocked instead of silently downgraded.
- Explicit opt-in Codex app-server source adapter support through
  `MOONBOX_CODEX_APP_SERVER_FIXTURE` or `MOONBOX_CODEX_APP_SERVER_PROXY=1`,
  preferring `thread/list`, `thread/read`, and `thread/turns/list` data while
  keeping local SQLite/JSONL as fallback.
- Non-executing `moonbox open-app` / `moon open-app` plans that preview
  `codex://threads/<id>` deep links for Codex sessions without launching the
  desktop app.
- Claude stream-json / SDK transcript metadata parsing for captured local JSONL
  records, including `system` init, `result`, `session_id`, cost, duration, API
  duration, turn count, hook events, partial stream events, fork parent metadata,
  and remote / remote-control observability records without invoking Claude.
- Hermes all-source inventory metadata in `sessions --json` through
  serde-default `provider_metadata`, including source, platform, user id,
  session key, parent session id, origin metadata, model config, system prompt
  snapshot, handoff state, archived state, and token breakdown.
- `moonbox sessions --hermes-source <source>` / `moon sessions --hermes-source
  <source>` provider-source filtering for Hermes inventories, with comma or
  repeated values and aliases such as `api-server`.
- `moonbox sessions --hermes-search <query>` / `moon sessions --hermes-search
  <query>` for read-only local Hermes message search, returning matching
  sessions with `provider_metadata.search` and
  `provider_metadata.continuation_points`.
- High-fidelity canonical timeline event metadata. `TimelineEvent` keeps the
  stable `id`, `time`, `kind`, `title`, and `detail` fields while adding
  serde-default `metadata` for raw refs, message/provider item ids, tool
  calls/results, approvals, attachments, file-change evidence, runtime
  snapshots, system/config snapshots, and token/cost data.

### Fixed

- Lark handoff export execution now compiles the selected session with the
  configured handoff runner and writes the generated Markdown artifact directly
  into the Feishu/Lark document, instead of exporting a fallback capsule summary
  or a temporary artifact path.
- Handoff Review success for agent-backed community skills is now skill-first:
  Moonbox reads the generated temporary Markdown artifact, shows that Markdown
  as the default Review body, removes capsule/verifier wrapper text from the
  user-facing surface, passes only a short Moonbox guard plus the reviewed
  Markdown to the target agent, hides built-in draft compilers from the TUI
  Skill Picker, and lets `Enter` confirm the target-agent launch while `y`
  remains the copy-command path.
- Handoff Review now reuses an already-running background generation job instead
  of spawning duplicate Codex/Claude SDK workers when users reopen Launch or
  press `enter` again. The pending panel shows target, compiler, stage, elapsed
  time, timeout, and explicitly says `enter` will not start another SDK process.
- Handoff Review retry and Skill Picker entry now refresh the compiler catalog
  and SDK preflight state. Missing Python SDK checks are no longer cached as
  permanent failures, so installing a runner SDK while Moonbox is open can be
  recovered by pressing `r` to retry from the failure panel.
- Pressing `H` / `x` while the selected session is still hydrating now opens the
  normal Launch target picker immediately. The picker shows a warning that the
  selected session context will load when Review starts, and `Enter` starts the
  background handoff job instead of waiting on the preloaded details pane.
- TUI startup now stops preloading the newest real session's timeline, anatomy,
  and capsule preview. The loading screen only waits for the read-only inventory;
  details hydrate on the first action that actually needs session context.
- Agent Handoff Review now treats community skill Markdown output as a handoff
  artifact rather than a legacy capsule: missing Moonbox-only semantic refs warn
  instead of blocking run/copy, while hard source / target / rewind mismatches
  still block. The pending panel also labels skill / runner, localized stage,
  elapsed time, timeout limit, and blocker reasons explicitly.
- The startup loading screen now follows the saved UI language preference and
  describes the bounded read-only startup index instead of hard-coded English
  scan text.
- Session Details now uses calmer theme-token metadata coloring, shortened path
  rendering, only the high-signal handoff marker, and a spatial Zoom Details
  layout that separates overview, Session Anatomy, Handoff Snapshot, and
  location/path content.
- Header chrome is now neutral global status instead of sharing Action Path
  focus styling; the Handoff Skill label names provider or built-in draft mode,
  and Zoom Action Path now expands into route, Enter behavior, cwd inventory,
  target readiness, and review-mode details.
- Simplified Chinese UI preferences now localize the main TUI chrome, including
  the header, session inventory, timeline/details/action panels, status line,
  key hints, and Launch target picker, while preserving source session content
  byte-for-byte.
- Launch target validation now explains stale handoffs generated by another
  skill/compiler as a regenerate-before-launch action instead of exposing raw
  `generated_by ... vs compiler ...` verifier internals in the target picker.
- Skill Picker now follows the selected UI language and user-facing handoff
  model: agent-backed rows are labeled as `Skill`, built-in fallback rows are
  labeled as draft templates, and missing stars metadata no longer appears as
  `n/a` or `not configured`.
- Skill Picker no longer exposes runner setup as skill setup: duplicate Codex /
  Claude runner rows collapse into one handoff skill row, `y` copies the skill
  path or install source, and SDK install / login / package-manager choices stay
  in launch preflight.
- Handoff Review now treats a selected-skill change as a stale review: it hides
  the old draft artifact, explains that regeneration is required, makes Enter
  start the background handoff job with the current skill, and keeps run/copy
  disabled until that regenerated review is ready.
- Real-session Handoff Review no longer opens the built-in draft compiler path:
  `engineering-handoff` and other draft templates require choosing an AI
  handoff skill first, `Enter` opens the Skill Picker instead of rendering the
  historical draft page, and real-session Skill Picker rows hide built-in draft
  templates.
- Failed background Handoff Review generation now stays visible in the Launch
  panel with the attempted skill/compiler, elapsed time, failure reason, and
  explicit retry / choose-skill actions instead of flashing and disappearing.
- Skill Picker now handles Enter while opened above Launch: selecting a handoff
  skill applies it and returns to Launch without starting AI generation.
  Installed community `handoff` skills also show the third-party provider and
  GitHub source link, not just the local `SKILL.md` path.
- Launch target picker now treats stale handoffs generated by a previously
  selected skill/compiler as a recoverable state: `Enter` starts a background
  regeneration with the current skill, while unrelated blocked targets remain
  blocked.
- Codex provider-injected AGENTS / environment context envelopes are no longer
  rendered as user timeline turns or used as user rewind anchors.
- Hermes SQLite session discovery and timeline loading now tolerate stores
  whose `messages` table does not include an `active` column, treating those
  legacy rows as active instead of aborting TUI startup.
- Version metadata and release/Homebrew templates now target the v0.1.2 hotfix
  prerelease for the Hermes SQLite schema compatibility rollout.
- Starred TUI sessions now keep their `*` marker visible even when the same row
  also needs a warning or failed health marker.
- Claude source health now treats `failed` as the latest AI outcome state, so
  recovered historical result errors remain visible in the timeline without
  marking the whole session failed.
- Timeline selected rows no longer shift body text or switch the body to bold
  when focus moves across events.

### Changed

- README is now a concise community-facing project page focused on the
  cross-CLI session workbench, Moonlight Box session-management vision, hotkeys,
  Luoshen themes, install paths, and acknowledgements; milestone-style change
  history stays in this changelog instead of the README.
- Documentation screenshots now render four fixture scenes through
  `docs-snapshot --scene`: Action Menu, Yank panel, Handoff Review, and zoomed
  Timeline details.
- Work Capsule and launch plan JSON now emit `handoff_label` instead of the
  misleading `target_branch` name, while still accepting legacy
  `target_branch` capsule input for compatibility.
- Text launch and verify output now labels verifier readiness as
  `preflight_ready` and scopes it to structural and semantic preflight while
  still requiring user review before handoff.
- Real-session `launch --execute` now blocks built-in draft compiler handoffs
  unless `--allow-draft` is explicitly passed; fixture sessions remain
  executable for safe tests and demos.
- README planning now tracks the accepted M68-M72 product design milestones:
  handoff trail signature, session portraits, pre-flight pill, command
  palette, and visual system polish.
- README now frames Capsule as Moonbox's product/schema name for the generic
  continuation-package category, keeping `capsule` CLI and JSON fields stable
  while improving external discoverability.
- Claude local-command XML-like records such as `<local-command-caveat>`,
  `<local-command-stdout>`, and `<command-name>` are now treated as internal
  tool events instead of user messages, rewind anchors, or resume-index titles.
- Claude Doctor capabilities now report captured stream-json / SDK metadata
  parsing as available while keeping remote / remote-control surfaces explicitly
  unavailable for launch/probing and separate from local resume rows.
- Compiler stdin, Capsule JSON export, verifier output, and target handoff
  prompts now use the shared redaction policy; target prompts include a
  dedicated Privacy / Redaction section while local execution routing remains
  exact for verifiable dry-run previews.
- Handoff Review readiness groups now prioritize Target Readiness, Workspace
  Restore, Source Health, Capsule Health, and Semantic Evidence, and launch
  validation summaries stay concise while full verifier checks remain available
  in JSON and TUI readiness details.
- Generated dry-run launch plans report `capsule_path: null` and do not emit
  fake `--capsule` paths.
- Codex, Claude, and Hermes source discovery use real local stores when any
  real store is present; fixture fallback is reserved for the no-real-store
  demo/CI case or explicit fixture mode.
- Codex and Claude default session lists continue to mirror each source CLI's
  own resume surface, while Hermes now defaults to all non-archived provider
  sources instead of only `source = cli` rows.
- Hermes Doctor capabilities now report all-source local inventory, captured
  source metadata, and local export/search-equivalent continuation point search
  as available; Hermes gateway/export commands remain non-invoked.
- Hermes search results carry snippets, bookends, message ids, and scroll
  context so continuation points can be located without expanding long sessions
  blindly.
- Capsule `raw_refs` now preserve message ids and provider item ids from
  canonical event metadata, giving verifier/export surfaces a stronger audit
  link back to source events without breaking legacy Capsule input.
- Explicit session lookup routes obvious Hermes/Codex/Claude ids to the likely
  adapter before expensive full-store fallback.
- CLI launch/verify uses lightweight session artifacts instead of constructing
  a full TUI workbench for explicit session ids.
- Target launch execution is opt-in and refuses to spawn a target command when
  verification fails.
- TUI target handoff now uses a dedicated `x` shortcut, with `H` and `t` kept
  as compatibility aliases, and a two-stage flow: choose a target, review the
  target command, then explicitly run with `r` or copy with `y`.
- TUI launch key hints now distinguish target selection from Handoff Review, so
  `y` is shown as unavailable until review.
- TUI launch copy now points at the actual target command; the modal shows a
  concise command summary so long handoff prompts do not flood the review.
- TUI Handoff Review `r` now restores the terminal and then launches the
  verified target CLI, while `enter` is review-only.
- Original-session execution is opt-in and uses source-specific resume
  entrypoints; Hermes resume commands now use `hermes --resume <session>`.
- TUI original-session copy now points at `moonbox open --execute`.
- TUI original-session review `enter` now restores the terminal, prints the
  exact original resume command, and on Unix originally replaced the Moonbox
  process with the source CLI so the terminal was handed off without Moonbox
  waiting in the foreground. M82 changes the default TUI behavior to
  suspend-and-return and keeps one-way exec behind `MOONBOX_RESUME_MODE=exec`;
  `y` still copies the guarded Moonbox wrapper.
- Main-list `enter` now directly opens the selected session with its original
  CLI; target handoff moved to the explicit `x` shortcut. M82 makes the default
  original open path return to Moonbox after the source CLI exits.
- TUI timeline hides provider-injected context rows such as
  `<environment_context>`, right-aligns event times, and scrolls by actual
  wrapped row height so the selected event stays visible.
- TUI timeline visually groups consecutive assistant messages into one
  source-specific `Codex xN` / `Claude Code xN` / `Hermes xN` block, and `j/k`
  navigation now moves by those visible groups instead of silently stepping
  through folded assistant events.
- TUI `S` now opens a Skill Picker instead of blindly cycling compiler skills;
  the picker shows status, kind, description, setup guidance, and command/link
  metadata before `enter` applies the pending selection.
- Built-in draft compiler rows now identify themselves as fallback draft
  templates in Skill Picker instead of presenting repository links, `n/a`
  stars, or production skill affordances.
- Action Path now shows the selected cwd plus Codex / Claude / Hermes session
  counts for that same path.
- Action Path now renders an explicit `source -> rewind -> target` route and a
  short 720 ms handoff trail when the target picker enters Handoff Review; Esc
  or `q` closes the Review and cancels the trail.
- Session rows now include readable activity metrics: visible rows show
  event/token activity, while Handoff Review and Session Details expose cached
  timeline role counts as explicit user / assistant / tool / rewind labels.
- TUI `+` / `=` now zooms the focused panel and `-` restores the default
  layout; zoom follows tab focus so Sessions, Timeline, Details, and Action
  Path can each be expanded without resetting selection or scroll state.
- Selected Timeline rows now preserve role accent colors, so active user turns
  stay blue and active AI groups stay gold instead of collapsing into one
  selected-state color.
- Compiler execution precedence is now explicit: environment override,
  configured default, first ready external preset, then built-in draft fallback.
- Built-in draft compilers now show warning catalog/readiness signals instead
  of presenting themselves as production-ready compiler skills.
- Unknown compiler ids and disabled compiler presets now return structured
  configuration errors instead of silently compiling through the fixture path.
- Saving the last selected target now preserves compiler presets and
  `default_compiler` in the user config file.
- TUI verify status no longer hard-codes the verifier check count.
- TUI session movement, source filtering, `/` search, `gg`, and `G` now update
  the selected row immediately, keep the left list compact with source-colored
  `Cdx` / `Clu` / `Hms` badges while preserving original source titles, hide
  healthy source markers in the left rail, show selected session metadata with
  Raw Title and Source Health in the right Session Details panel, and hydrate
  the timeline/capsule preview in the background from the current session index
  snapshot.
- TUI session-list secondary rows now use relative resume-picker timestamps
  such as `16s ago` / `3m ago`; exact timestamps remain available in the right
  Session Details panel.
- TUI `c` now refreshes the Work Capsule and opens Handoff Review directly; the
  old TUI-only `d Diff` surface was removed to keep the handoff flow linear.
- TUI sessions can now be starred with `s` (`*` remains an alias), persisted in
  user config, and filtered through the `Star` source filter before `All`.
- Session titles captured from Codex, Claude, and Hermes real stores now keep a
  longer preview for the right Session Details panel while the list remains
  windowed and clipped by the TUI.
- TUI header no longer shows a fake `/ 100K` token budget, keeps compiler status
  width stable across `ACTIVE` / `LOADING` / `COMPILED`, and mutes unselected
  session titles so only the active row reads as highlighted.
- The right Session Details panel now shows a compact Handoff Snapshot; full
  capsule decisions, todo, evidence, and risks move to Handoff Review.
- TUI session rows now render the selection arrow inline and only show the
  star/status marker when needed, removing the large blank gap before `Cdx` /
  `Clu` / `Hms` source badges.
- Resume-index rows whose event count is unknown now still hydrate their real
  timeline from `source_path`; truly empty timelines build a pending capsule
  instead of crashing the TUI by compiling a missing rewind id.
- CLI runtime now lives behind a shared library entrypoint used by both
  `moonbox` and `moon`.
- Replay eval JSON now separates matrix and synthetic case counts, labels each
  case with `case_kind` and `scenario`, and reports expected scenario coverage.
- README screenshot now shows the current Handoff Review target-input flow on a
  transparent SVG canvas, generated from the real TUI render path.
- Draft Homebrew formula now points at a staged GitHub release source archive
  instead of a GitHub auto-generated tag archive.
- TUI `{` / `}` now cycles the main session inventory between Local and saved
  Moonbox SSH data spaces. Remote spaces run `ssh <target> moonbox sessions
  --json`, load the returned sessions as a read-only inventory, and keep
  failures visible in the status line while preserving the previous local list.
- Codex session titles now prefer `session_index.jsonl` `thread_name` values
  over stale `state_5.sqlite` titles, so renamed Codex threads are searchable
  and displayed the same way as the Codex resume picker.
- Codex Doctor capability reports now distinguish unconfigured app-server
  fallback, configured app-server success, and app-server fallback errors; deep
  links are reported as non-executing preview support instead of a planned M62
  item.
- Timeline cursor and selected rewind anchor markers now use separate visual
  treatment, so the active row and the saved rewind point no longer both look
  like the current selection.
- Handoff Review readiness output is grouped into Source Health, Capsule
  Health, and Target Readiness sections, with the full target prompt available
  from the review panel before launch.
- Auto session discovery now uses fixture fallback only when no real source
  stores are present, so real local indexes are not mixed with demo sessions.
- `open --execute` and `launch --execute` now require an explicit `--session`;
  dry-runs can still omit `--session` to preview the newest discovered session.
- Doctor diagnostics now use a single source inventory scan for session
  discovery and adapter health reporting, avoiding duplicate real-store scans.
- Doctor check details and the TUI selected-session metadata now show the same
  source fidelity status/surface, so degraded local fallback paths are visible
  without interpreting capability matrices.
- TUI session rows now include REAL/FIXTURE provenance badges, while the Doctor
  overlay exposes full adapter path and skip-count details.
- The TUI top bar now exposes one `Pre-flight: PASS/WARN/BLOCKED` signal with
  Strong/Medium/Weak confidence language, and the `D` panel expands Compiler,
  Doctor, and Verify evidence in one place.
- `:` now opens a floating Command Palette with fuzzy completion, descriptions,
  parameter hints, aliases, empty-state guidance, and dry-run / review / exit
  risk labels for existing TUI actions.
- TUI visual roles now use stable semantic colors for confidence, source badges,
  rewind/target path nodes, and Action Path inventory counts; narrow headers
  collapse the brand to `MOONBOX`.
- Capsule is now a first-class local object: `capsule save/list/show/launch`,
  `export/import`, and `delete` are backed by an isolated SQLite store via
  `MOONBOX_CAPSULE_STORE`, import validates Moonbox envelopes before saving,
  and the TUI Command Palette can open a saved Capsule inventory overlay.
- Launch Ledger records local `open --execute`, `launch --execute`, and
  `capsule launch --execute` success, failure, and blocked outcomes in an
  isolated SQLite ledger via `MOONBOX_LAUNCH_LEDGER`; `launches list/show/link`
  and `capsule launches <name>` expose the local audit trail without opening or
  resuming source sessions.
- Codex and Claude inline `<image ...>` timeline markers are promoted into
  `TimelineAttachment` metadata, and the TUI renders image attachment rows
  instead of leaking raw image markup into user turns.
- Timeline-focused `e` opens a scrollable selected-event detail overlay without
  changing `enter` original resume/open semantics or `space` rewind selection.
- Timeline detail overlays now expand folded assistant groups, so rows such as
  `Codex x88` show every grouped event id, timestamp, and body instead of only
  the first event.
- Timeline detail overlays render bounded truecolor previews for image
  attachments that expose safe local PNG/JPEG artifact paths, and explain why
  attachments without local paths cannot be previewed. Expanded assistant
  groups now use compact per-event rows instead of repeating `Title` / `Body`
  labels for every grouped event.
- Replay-eval fixture invariants now return structured `CoreError` failures
  instead of panicking, and generated SVG docs snapshot code no longer relies on
  infallible string-write `expect` calls.
- Internal adapters, compiler runners, source stores, and TUI state no longer
  leak through the public Rust crate API.
- TUI session-list panel titles use stable-width slots so source/filter
  switching no longer shifts the top border line.
- TUI timeline rendering folds low-signal tool/function-call rows by default,
  and timeline navigation/rewind selection skip hidden tool events.
- Canonical timeline parsing now folds adjacent duplicate events from Codex,
  Claude, and Hermes before rendering, so provider double-writes do not show as
  repeated timeline rows.
- Real-session default rewind selection now prefers user turns or explicit
  rewind markers instead of assistant/tool output or arbitrary fixed fixture
  ids such as `evt-091`.
- TUI `space` now rejects assistant/tool rows as rewind anchors while still
  allowing those rows to remain visible for reading context.
- TUI `Esc` no longer exits from the main workbench or loading screen; exit is
  reserved for `q`, `Ctrl-C`, or explicit handoff actions. `Esc` still closes
  overlays and cancels command/search modes.
- Built-in compiler output is labeled as deterministic draft guidance; real
  fields are limited to session id, title, cwd, selected rewind, and source
  health until an external compiler skill is configured.
- Target handoff prompts now render a readable Work Capsule Summary with
  source, target, goal, state, decisions, todo, evidence, risks, and
  instructions instead of dumping the capsule as raw JSON into the target CLI
  first screen.
- TUI target handoff also accepts `x` as the primary cross-CLI handoff
  shortcut, while `H` and `t` remain compatibility aliases.
- `capsule`, `compile-request`, and `compile-output` now accept explicit
  `--session`, `--target`, `--rewind`, and `--compiler` options, replacing the
  old hard-coded Codex-to-Hermes inspection defaults with the same selected
  session/rewind model used by launch and the TUI.
- CLI smoke coverage now calls those inspection surfaces with explicit fixture
  session, target, and rewind arguments, so the gate cannot depend on a user's
  persisted last-target setting.

### Not Yet Released

- Homebrew formula, release archives, and package registry publishing are
  planned but not published.
