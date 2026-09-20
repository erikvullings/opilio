# 0012 — Build TUI dashboard

**Status:** open  
**Depends on:** 0008, 0009, 0010, 0011  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Build the v1 LazyDocker-style keyboard-first master/detail TUI using shared application APIs.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Implement one dashboard with sites/groups, device list, detail pane, help/dialog overlays.
- Arrow and Vim-style navigation; `/` filter/search; manual refresh.
- Show distinct off/unreachable/running/booting/error/unknown states.
- Display RAM, GPU, Shelly watts and in-memory sparklines when available.
- Display generic LLM/service health/model information.
- Support on/off/reboot, named action chooser, SSH handoff, and safety confirmations.
- Use temporary OpenSSH multiplexing while TUI is active.
- Adaptive polling: fast for reachable telemetry/power, slower service probes, backoff for unreachable devices/sites; automatically recover when connectivity returns.
- Show recent failure indication but do not build a full log viewer/config editor/embedded terminal.

## Acceptance criteria

- TUI state/update reducer is unit tested independently of terminal rendering.
- Simulated devices demonstrate site loss/recovery and adaptive polling.
- Long boot/model load never blocks UI interaction.
- Safety dialogs match CLI semantics.

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
