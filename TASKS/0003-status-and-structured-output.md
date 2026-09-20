# 0003 — Status and structured output

**Status:** done
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

## Decisions

- The versioned status DTO contains the requested target, aggregate counts, and
  sorted per-device results; terminal table/detail rendering is a separate layer.
- Status uses a configured-only source today, with an injectable source seam for
  later runtime probes.
- Read-only work defaults to four workers and uses a reusable bounded executor;
  `--parallel N` accepts only non-zero values.

## Progress

- [x] Task picked up; status changed to `in-progress` in this file and root README.
- [x] Implementation completed.
- [x] Focused tests pass.
- [x] Full relevant test suite/lints pass.
- [x] Documentation/examples updated if behavior is user-visible.
- [x] Status changed to `done` and root README dashboard updated.

## Validation

- `cargo test --test status --test cli_status --quiet` — 10 task-focused tests passed.
- `cargo check --all-targets --all-features` — passed.
- `cargo test --all-targets --all-features` — full suite passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt --check` — passed.
- `cargo run --quiet -- --config examples/config.yaml status --json` and
  `status spark-home --quiet` — smoke tests passed.

## Blockers / decisions needed

None currently.

## Notes / handoff

- 2026-09-20 Copilot: Added the end-to-end `status` command with device/group/
  site/all resolution, configured-only status adapter, bounded concurrency,
  non-fail-fast aggregation, stable versioned JSON DTOs, human table/detail
  output, quiet mode, and exit codes 0/1/2/3. Added 10 focused compatibility,
  target, concurrency, and partial-success tests; full tests, check, strict
  clippy, formatting, and CLI smoke tests pass. No known task-scope limitations.
