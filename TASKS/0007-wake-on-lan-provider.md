# 0007 — Wake-on-LAN provider

**Status:** open  
**Depends on:** 0003  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Add Wake-on-LAN as a simple power-on provider for non-Shelly machines.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Implement MAC parsing/validation and WoL magic-packet construction.
- Send UDP broadcast using configurable/default network behavior appropriate to the platform.
- Integrate provider capability reporting: WoL can request on but cannot cut physical power.
- Keep provider interface compatible with devices that use Shelly or WoL.
- Unit-test packet bytes and validation; keep network send behind a test seam.

## Acceptance criteria

- Known MAC produces correct magic packet.
- Invalid MAC/config fails statically.
- Capability model prevents treating WoL as physical power-off support.

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
