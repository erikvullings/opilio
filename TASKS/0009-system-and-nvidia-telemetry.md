# 0009 — System and NVIDIA telemetry

**Status:** open  
**Depends on:** 0004  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Collect remote system/NVIDIA metrics, including correct DGX Spark unified-memory semantics.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Implement optional system telemetry over multiplexed SSH: CPU/load, RAM, uptime and useful basics.
- Implement NVIDIA telemetry behind a provider; discover supported metrics rather than assuming all `nvidia-smi` fields exist.
- Detect DGX Spark/GB10/unified-memory behavior and expose memory semantics honestly; do not label unsupported conventional VRAM as real VRAM.
- Define typed telemetry snapshots suitable for CLI/TUI.
- Add in-memory time-series buffers for TUI sparklines; no persistent telemetry DB.
- Polling orchestration must support different intervals and cancellation.

## Acceptance criteria

- Parser tests use fixtures for ordinary NVIDIA systems and Spark/UMA cases.
- Unsupported values remain absent/unknown rather than zero/fabricated.
- Snapshot/time-series APIs are independent of terminal rendering.

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
