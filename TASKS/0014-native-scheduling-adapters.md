# 0014 — Native scheduling adapters

**Status:** open  
**Depends on:** 0005, 0011  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Implement Opilio-owned native scheduled jobs on Linux, macOS, and Windows without a daemon.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Define common scheduler interface and ownership metadata.
- Implement `schedule ls`, `schedule add`, `schedule rm <id>`.
- Linux: systemd user timers where available, with documented fallback such as cron.
- macOS: launchd.
- Windows: Task Scheduler.
- Jobs invoke deterministic Opilio operation/alias commands non-interactively and are recorded in history when run.
- Manage/list/remove only jobs created by Opilio.
- Use native scheduler missed-run semantics; no Opilio catch-up mechanism.
- Build adapters so unit tests inspect generated commands/files rather than altering the developer's scheduler.

## Acceptance criteria

- Adapter contract tests exist for all three OS families.
- Ownership prevents unrelated scheduler entries appearing in `opilio schedule ls`.
- Add/remove round-trip is covered with fake platform adapters.
- No daemon/background Opilio process is introduced.

## Implementation notes

- Prefer a vertical, demonstrable slice through shared library APIs rather than code that only a later task can exercise.
- Keep platform/hardware/process/network dependencies behind test seams.
- Treat JSON output and secret redaction as compatibility/security surfaces where applicable.
- Add dependencies only when they are maintained and materially reduce complexity.

## Progress

- [ ] Task picked up; status changed to `in-progress` in this file and root README.
- [ ] Implementation completed.
- [ ] Focused tests pass.
- [ ] Full relevant test suite/lints pass.
- [ ] Documentation/examples updated if behavior is user-visible.
- [ ] Status changed to `done` and root README dashboard updated.

## Validation

Record commands/tests and concise results here before marking done.

## Blockers / decisions needed

None currently.

## Notes / handoff

Add anything a fresh agent needs to resume this task.
