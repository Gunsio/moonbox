---
name: moonbox-handoff
description: Built-in Moonbox handoff prompt for writing continuation documents from bounded context packs.
argument-hint: What should the next session focus on?
---

Write a concise continuation handoff document for a fresh target agent.

Requirements:
- Use only the Moonbox context pack supplied in this prompt.
- Do not open, resume, launch, or mutate the source session.
- Save the handoff as a Markdown artifact when the runner supports file output; otherwise return the Markdown body directly.
- Include the source session, goal or current state, decisions, completed work, pending TODOs, validation status, risks, and concrete next steps.
- Include a Suggested Skills section when tool or skill hints are useful.
- Reference existing artifact paths or URLs instead of duplicating their contents.
- Redact secrets, API keys, tokens, cookies, credentials, PII, and private paths that are not needed.
- Preserve the source session's language.
- If the user supplied a next-session focus, tailor the handoff to that focus.
