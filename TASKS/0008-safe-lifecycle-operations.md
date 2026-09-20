# 0008 — Safe lifecycle operations

**Status:** open  
**Depends on:** 0004, 0006, 0007  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Deliver safe on/off/shutdown/reboot/power-off/power-cycle behavior across devices and collections.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Implement `on`, `off`, `shutdown`, `reboot`, `power-off`, `power-cycle`.
- `on` returns after requesting power by default; `--wait` waits only for SSH availability.
- Normal `off`: graceful SSH shutdown -> wait/timeout -> cut Shelly power only when configured.
- `--yes` suppresses confirmation only. `--force` authorizes bypassing graceful shutdown/physical cut.
- Single-device routine operations need no prompt; disruptive group/site operations resolve/show affected devices and confirm unless `--yes`.
- Scheduled/non-interactive callers must be able to explicitly confirm without interactive input.
- Default disruptive parallelism is 1; failures do not stop unrelated devices.
- Represent powered/booting/SSH-ready/unreachable/unknown without overclaiming.

## Acceptance criteria

- Tests prove `--yes` alone cannot force physical cut.
- Group confirmation includes resolved devices.
- WoL-only off degrades to OS shutdown and never claims physical cut.
- `on --wait` stops at SSH readiness, not service/model readiness.
- Human/JSON result and partial failure behavior are covered.

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
