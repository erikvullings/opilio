# 0013 — Diagnostics and doctor

**Status:** open  
**Depends on:** 0006, 0007, 0009, 0010  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Separate static configuration validation from runtime environment/device diagnostics.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Keep `config check` network-free and static.
- Implement `doctor [target]`: local OpenSSH availability, target reachability, SSH, power provider, telemetry capability, service probes, and useful remote capability observations.
- Detect informative remote capabilities such as NVIDIA/GB10/UMA, Docker, systemd without making them requirements.
- Aggregate likely site-level unreachability so VPN-off devices are not individually misdiagnosed as powered off.
- Suggest configuration snippets/corrections when useful, but never modify config automatically.
- No general LAN discovery.

## Acceptance criteria

- Doctor output distinguishes invalid config, missing local dependency, unreachable site/device, and unsupported capability.
- JSON form is machine-readable.
- Suggested snippets are advisory only and cannot mutate config.

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
