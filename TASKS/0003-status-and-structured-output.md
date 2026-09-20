# 0003 — Status and structured output

**Status:** open  
**Depends on:** 0002  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Create the first end-to-end CLI slice with target resolution, result aggregation, JSON, and exit codes.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Implement `opilio status [target]`, initially with configured/static/reachability-neutral state as adapters are added later.
- Default target is all devices.
- Create stable result DTOs separate from terminal formatting.
- Implement human table/detail formatting and `--json`; add `--quiet` where meaningful.
- Establish exit codes 0 success, 1 failed, 2 config/usage, 3 partial success.
- Add controlled concurrent execution abstraction suitable for later probes and operations.
- Group/site processing must retain per-device results and not abort remaining devices on one failure.

## Acceptance criteria

- Human and JSON status outputs are snapshot/schema tested.
- Partial-success exit behavior is tested.
- Device/group/site/all target resolution is demonstrated through the command.

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
