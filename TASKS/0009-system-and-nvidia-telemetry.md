# 0009 — System and NVIDIA telemetry

**Status:** done
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

## Decisions

- Telemetry providers return typed `available`, `unsupported`, or `unavailable`
  states; numeric failures never become zero.
- Linux CPU utilization is sampled from two `/proc/stat` readings rather than
  presenting lifetime CPU time as current utilization.
- NVIDIA fields are selected from `nvidia-smi --help-query-gpu`; DGX Spark or
  GB10 identity forces unified-memory semantics and suppresses conventional
  GPU-memory totals and usage.
- CLI status uses one-shot OpenSSH, while the same executor seam is implemented
  by `ControlMaster` for frequent TUI polling.
- Status JSON keeps schema version 1 and omits the additive `telemetry` member
  for unconfigured devices, preserving existing documents.

## Progress

- [x] Task picked up; status changed to `in-progress` in this file and root README.
- [x] Implementation completed.
- [x] Focused tests pass.
- [x] Full relevant test suite/lints pass.
- [x] Documentation/examples updated if behavior is user-visible.
- [x] Status changed to `done` and root README dashboard updated.

## Validation

- `cargo test --test telemetry --test status --test cli_status --quiet` — passed;
  11 telemetry tests plus 1 task-specific status integration test, with the
  existing status/JSON compatibility tests retained.
- `cargo check --all-targets --all-features` — passed.
- `cargo fmt --all -- --check` — passed.
- `cargo test --all-targets --all-features` — full suite passed (95 tests).
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.

## Blockers / decisions needed

None currently.

## Notes / handoff

- 2026-09-20 Copilot: Added fixture-driven Linux `/proc` and capability-driven
  NVIDIA parsers, honest DGX Spark/GB10 UMA DTOs, one-shot and multiplexed
  OpenSSH telemetry adapters, cancellation-aware interval scheduling, bounded
  in-memory histories, and optional status JSON integration. Verified 12
  task-specific tests without real hosts, all 95 project tests, check, format,
  and strict clippy. No known task-scope limitations.
