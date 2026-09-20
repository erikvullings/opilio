# 0016 — Cross-platform hardening and release

**Status:** open  
**Depends on:** 0012, 0013, 0014, 0015  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Close the v1 acceptance criteria and make Opilio distributable on macOS, Linux, and Windows.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Run full v1 acceptance matrix from the spec.
- Harden platform directory handling, process spawning, terminal behavior, scheduler detection, and OpenSSH discovery.
- Document installation, config examples, environment secrets, Shelly/WoL setup, TUI keys, scheduling, export/import, safety semantics, and troubleshooting.
- Add representative example config for local + VPN sites and DGX Spark/LLM service.
- Verify JSON compatibility fixtures and exit codes.
- Add release CI producing platform binaries/checksums and appropriate package-manager follow-up guidance (packaging channels may be separate follow-up tasks).
- Perform security/redaction review and failure-mode review.
- Ensure README task dashboard reflects completion.

## Acceptance criteria

- CI/release builds succeed on Linux, macOS, Windows.
- All v1 acceptance criteria in `docs/OPILIO_V1_SPEC.md` are checked off with evidence.
- Fresh-machine smoke test can import/configure and run `config check`, `doctor`, and `status`.
- No v1 feature requires an Opilio daemon or remote agent.

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
