# 0011 — Operation history and logging

**Status:** done
**Depends on:** 0005, 0008  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Persist a safe rotating JSONL audit/history of Opilio operations.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Define history record IDs and metadata: timestamp, source, operation/action, requested target, device, duration, result/exit code, force flag where relevant.
- Write rotating JSONL in a platform-appropriate local data/log directory.
- Store metadata for successes without full command output.
- On failure, store only a configurable bounded tail of stdout/stderr (default around 64 KiB).
- Redact environment-secret values and avoid intentional secret capture.
- Implement `history`, `history <target>`, `history show <id>`, plus JSON output.
- Ensure scheduled/CLI/TUI callers can identify their source.

## Acceptance criteria

- Rotation, query, target filtering, bounded failure output, and malformed-tail recovery are tested.
- Tests prove known secret values are not emitted when redaction is possible.
- Successful command output is not persisted wholesale.

## Implementation notes

- Prefer a vertical, demonstrable slice through shared library APIs rather than code that only a later task can exercise.
- Keep platform/hardware/process/network dependencies behind test seams.
- Treat JSON output and secret redaction as compatibility/security surfaces where applicable.
- Add dependencies only when they are maintained and materially reduce complexity.

## Decisions

- History is one versioned JSONL record per resolved device, ordered newest
  first when queried and identified by a UUID.
- The active log rotates under a cross-process file lock to three retained
  backups plus the active file by default. A missing newline is repaired before
  append so a truncated tail cannot consume the next valid record.
- Failure stdout, stderr, and errors are redacted before each field is limited
  to a UTF-8-safe 64 KiB tail. Successful records omit all command output.
- Storage limits and location use `OPILIO_HISTORY_*` environment settings.
  Frontends identify themselves with the typed `cli`, `tui`, or `scheduled`
  source.

## Progress

- [x] Task picked up; status changed to `in-progress` in this file and root README.
- [x] Implementation completed.
- [x] Focused tests pass.
- [x] Full relevant test suite/lints pass.
- [x] Documentation/examples updated if behavior is user-visible.
- [x] Status changed to `done` and root README dashboard updated.

## Validation

- `cargo test --test history --test cli_history --quiet` — 9 focused
  public-interface tests passed, covering rotation, concurrent append,
  filtering/show, malformed and truncated recovery, redaction, bounded UTF-8
  tails, successful-output omission, source typing, human/JSON output, and
  action integration.
- `cargo check --all-targets --all-features` — passed without warnings.
- `cargo fmt --all -- --check` — passed.
- `cargo test --all-targets --all-features --quiet` — full suite passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.

## Blockers / decisions needed

None currently.

## Notes / handoff

- 2026-09-20 Copilot: Implemented locked rotating platform-local JSONL history,
  typed source/operation/target/device/result metadata, UUID/timestamp IDs,
  configurable UTF-8-safe failure tails, configured-secret and URL-encoded
  redaction, explicit malformed/truncated warnings, target/show human and
  stable JSON commands, and central recording for status, action, lifecycle,
  aliases, and interactive SSH. Verified 9 focused tests plus full check,
  format, test, and strict-clippy validation; no known task-scope limitations.
