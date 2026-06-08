# AGENTS.md

- One milestone = one branch + one commit + one PR.
- Before starting a new milestone, verify `main`, `origin/main`, and relevant PR status.
- Do not start dependent milestones from unmerged milestone branches unless explicitly requested.
- Never open, resume, or launch recent real sessions in tests.
- Source session stores are read-only.
- Update README, CHANGELOG, and the Feishu plan document for each completed milestone.
- User-reported experience issues interrupt the milestone queue and take priority until resolved or explicitly deferred.
- Prefer fixture or isolated homes for all contract tests and smoke tests.
- TUI changes require visual and interaction review, not just a compile pass.
- Hold implementation quality to top-tier open-source standards; do not shrink scope for speed.
- Hold TUI aesthetics, interaction detail, and keyboard ergonomics to a high product standard.
- Ask the user when a decision is uncertain, risky, or not recoverable from repository evidence.
- If GitHub state and external docs disagree, GitHub main/PR status is the code-availability source of truth until docs are reconciled.
- Keep documentation sparse: fixed operating rules only, no filler.
