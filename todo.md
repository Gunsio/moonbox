# Moonbox TODO

> Planning inventory from the 2026-07-13 handoff. Items below are not claims of implemented behavior.

## Incoming high-priority requests

- [x] **Codex full-access Resume in Action Menu** — completed as a separate, explicitly dangerous local native-Codex action. It preserves ordinary Resume; excludes Claude, Hermes, SSH, and K2-wrapped Codex; shows the full command; and requires `Shift+R` after review.
- [x] **Codex child-session visibility** — stopped collapsing independently resumable `forked_from_id` sessions, which hid most recent Codex multi-agent work after the provider update.
- [x] **Large Timeline continuation** — `G` now keeps a truncated Timeline
  responsive by loading the next bounded page in the background and landing at
  its new end; repeat `G` while the marker remains.
- [x] **Timeline page progress** — while `G` loads a bounded page, keep the
  current preview visible and show actual parsed-event percentage plus
  loaded/target counts; never infer the value from elapsed time.
- [x] **Timeline progress accuracy and placement** — prevent buffered parser
  updates from collapsing to a visible `100%`; keep a resident Timeline load
  status in the third Session Details column, with real page progress only
  while loading. After a page completes but more history remains, show the
  loaded count and next `G` action instead of stale `100%`; do not expose
  implementation labels such as `Preview` or imply whole-session progress
  before the source has reached its end.

## P0 — Experience regressions to verify and fix

- [ ] **Lark handoff document correctness** — ensure the created document title comes from the selected session and the body contains the complete reviewed handoff artifact, never only a local artifact path or generation-status text.
- [ ] **Handoff artifact discovery** — accept valid temp-dir `*-handoff.md` artifacts from external skills, not only files with a `moonbox-handoff-*` prefix; reject runner-status text as an artifact body.
- [ ] **Picker and menu consistency** — make Action Menu, Yank, Settings, Skill Picker, Launch, and Data Space Picker share stable selection semantics: marker gutter, fixed icon column, no layout shift, strong selected state, secondary descriptions on a following line, and status indicators only for warning/blocked states.
- [ ] **Modal sizing** — size pickers and dialogs to their content within sensible terminal-cell bounds; prevent excess empty space and narrow-terminal overflow.
- [ ] **Fixture-safe end-to-end review** — reproduce the above flows in an isolated home or fixtures, with render and interaction review; never resume, fork, launch, or otherwise use a real session.

## P1 — Foundation and workflow closure

- [ ] **M108: Live State Completion** — reliably determine process liveness and attachment to a locatable tmux pane. Expose Jump only when both are known; unknown state must not be presented as jumpable.
- [ ] **M109: Header / Footer information architecture** — retain only high-value global/session context: data source, filter, theme, defaults, hooks/live state, alive/tmux/cwd/branch, actions, and blocking reasons.
- [ ] **M110: Deterministic Primary Action** — implement explainable, low-risk rules: Jump only when safe, otherwise Resume/Handoff/Inspect. Do not use opaque or heuristic “smart” recommendations.
- [ ] **M113: Settings closure** — consolidate defaults for target, skill, runner, archive/fork behavior, and installation/integration state so workflows do not require repeated choices.
- [ ] **M114: Theme polish** — visually review all four themes across screens, especially selection, warning/blocked states, narrow widths, and ANSI fallback.
- [ ] **M115: Product-quality audit** — produce an evidence-backed, prioritized audit of architecture, state ownership, naming, fixture safety, tests, and obsolete UI paths.

## P2 — Product research and later capabilities

- [ ] **Cross-machine Export / Import** — define and implement a portable file format, limits, provider/session mapping, attachment handling, and `moonbox import`. Compact Yank JSON is not a recoverable provider-native session.
- [ ] **Goose adapter** — first establish Goose’s actual session-store format and command capabilities; keep this late in the roadmap.
- [ ] **Multi-agent concurrent new sessions** — support sending one task to multiple agents and manage them in tmux only after live-state, tmux, and cwd foundations are complete.
- [ ] **Session-management guidance** — research decision criteria for Resume, Fork, Rewind, Handoff, Abandon, and Guidance. Do not implement an asserted “intelligent recommendation” without evidence.

## Delivery gates

- [ ] Start each milestone from verified `main` / `origin/main` and check relevant PR status; do not use unmerged milestone branches as a base.
- [ ] Keep one milestone to one branch, one commit, and one PR.
- [ ] Treat provider session stores as read-only and never open, resume, launch, fork, copy, or restore real sessions in tests.
- [ ] Use fixtures or isolated homes for contract, smoke, visual, and interaction tests.
- [ ] For every completed milestone, update `README.md`, `CHANGELOG.md`, and the Feishu plan document.
- [ ] For TUI changes, require visual and interaction review in addition to compilation and automated tests.
