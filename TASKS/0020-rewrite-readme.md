# 0020 — Rewrite product README

**Status:** done  
**Depends on:** 0016, 0018  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Make the repository landing page easy to scan, focused on Opilio's functionality, and grounded by a current TUI screenshot.

## Requirements

- Lead with the product value and core capabilities.
- Include a real screenshot from the current TUI build with useful alt text.
- Provide a short install/configure/run path.
- Explain power safety and the agentless architecture without duplicating the full user guide.
- Keep development, acceptance, and task-tracking links available below the product content.
- Retain the task status table required by `AGENTS.md`, but move it out of the primary reading path.

## Acceptance criteria

- A new reader can identify the supported workflows and try Opilio from the first screenful.
- The screenshot contains no secrets or private controller paths.
- All README links and referenced assets exist.
- Markdown and repository validation pass.

## Progress

- [x] Task picked up.
- [x] TUI screenshot captured.
- [x] README rewritten.
- [x] Links and formatting validated.
- [x] Status changed to `done`.

## Validation

- Captured `docs/assets/opilio-tui.png` from the current Ratatui renderer using a deterministic, secret-free fleet fixture.
- Verified every relative Markdown link and image target exists.
- `cargo fmt --check` — passed.
- `cargo test --quiet --all-targets --all-features` — passed.

## Blockers / decisions needed

None currently.

## Notes / handoff

Keep deep command semantics in `docs/USER_GUIDE.md`; the README should remain a functional overview.
