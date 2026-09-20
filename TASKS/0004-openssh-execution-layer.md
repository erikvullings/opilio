# 0004 — OpenSSH execution layer

**Status:** open  
**Depends on:** 0002  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Delegate remote execution to system OpenSSH and provide one-shot plus multiplexed sessions.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Detect/invoke system `ssh`; do not embed an SSH protocol library.
- Treat configured SSH target as an OpenSSH host/alias and preserve user SSH config behavior.
- Implement one-shot command execution with timeout, stdout/stderr capture, and structured result.
- Support shell `command` execution via explicit configurable remote shell (Linux default `/bin/sh -lc`).
- Support structured `exec` with robust argument construction/escaping.
- Implement `opilio ssh <device>` as interactive handoff; reject groups/sites.
- Add a temporary ControlMaster/ControlPath abstraction for TUI polling, with cleanup on exit.
- Keep process execution behind a fakeable adapter for tests.

## Acceptance criteria

- Unit tests verify command construction/escaping/timeouts without requiring real SSH hosts.
- Interactive SSH resolves exactly one device.
- Multiplex lifecycle can be started/reused/closed through the library API.
- Existing OpenSSH configuration remains authoritative.

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
