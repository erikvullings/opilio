# 0011 — Operation history and logging

**Status:** open  
**Depends on:** 0005, 0008  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Persist a safe rotating JSONL audit/history of Opilio operations.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Define history record IDs and metadata: timestamp, source, operation/action, requested target, device, duration, result/exit code, force flag where relevant.
- Write rotating JSONL in a platform-appropriate local data/log directory.
- Store metadata for successes without full command output.
- On failure, store only a configurable bounded tail of stdout/stderr (default around 64 KiB).
- Redact environment-secret values and avoid intentional secret capture.
- Implement `history`, `history <target>`, `history show <id>`, plus JSON output.
- Ensure scheduled/CLI/TUI callers can identify their source.

## Acceptance criteria

- Rotation, query, target filtering, bounded failure output, and malformed-tail recovery are tested.
- Tests prove known secret values are not emitted when redaction is possible.
- Successful command output is not persisted wholesale.

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
