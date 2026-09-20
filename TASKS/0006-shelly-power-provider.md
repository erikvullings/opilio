# 0006 — Shelly power provider

**Status:** open  
**Depends on:** 0003  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Implement direct local Shelly control and electrical telemetry behind a power-provider interface.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Define a generic power-provider interface used by application logic.
- Implement Shelly outlet on/off/state through the local device API.
- Read available voltage/current/watts and useful energy values.
- Support optional auth using environment-secret references; never log resolved credentials.
- Model unreachable/unsupported metrics explicitly rather than fabricating values.
- Use mock HTTP tests; hardware tests are opt-in/manual.
- Do not add SSH tunneling or VPN control.

## Acceptance criteria

- Mocked Shelly can be switched/read through the provider interface.
- Power telemetry maps to common DTOs.
- Auth values are redacted from diagnostics/errors/loggable structures.
- Network failures yield explicit unknown/unreachable states.

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
