# 0014 — Native scheduling adapters

**Status:** done
**Depends on:** 0005, 0011  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Implement Opilio-owned native scheduled jobs on Linux, macOS, and Windows without a daemon.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Define common scheduler interface and ownership metadata.
- Implement `schedule ls`, `schedule add`, `schedule rm <id>`.
- Linux: systemd user timers where available, with documented fallback such as cron.
- macOS: launchd.
- Windows: Task Scheduler.
- Jobs invoke deterministic Opilio operation/alias commands non-interactively and are recorded in history when run.
- Manage/list/remove only jobs created by Opilio.
- Use native scheduler missed-run semantics; no Opilio catch-up mechanism.
- Build adapters so unit tests inspect generated commands/files rather than altering the developer's scheduler.

## Acceptance criteria

- Adapter contract tests exist for all three OS families.
- Ownership prevents unrelated scheduler entries appearing in `opilio schedule ls`.
- Add/remove round-trip is covered with fake platform adapters.
- No daemon/background Opilio process is introduced.

## Implementation notes

- Prefer a vertical, demonstrable slice through shared library APIs rather than code that only a later task can exercise.
- Keep platform/hardware/process/network dependencies behind test seams.
- Treat JSON output and secret redaction as compatibility/security surfaces where applicable.
- Add dependencies only when they are maintained and materially reduce complexity.

## Progress

- [x] Task picked up; status changed to `in-progress` in this file and root README.
- [x] Implementation completed.
- [x] Focused tests pass.
- [x] Full relevant test suite/lints pass.
- [x] Documentation/examples updated if behavior is user-visible.
- [x] Status changed to `done` and root README dashboard updated.

## Decisions

- `schedule add` uses a stable, user-selected lowercase ID and a portable daily
  local time (`HH:MM`); native missed-run behavior is preserved.
- A private ownership registry plus markers in every native artifact/cron line
  forms the authority for listing and removal. Missing or changed artifacts are
  reported as partial listing warnings; foreign entries are ignored.
- Scheduled commands are parsed through the normal CLI and restricted to
  status, lifecycle, named-action, and alias execution.

## Validation

- `cargo test --test scheduler --test cli_scheduler --quiet` — 8
  task-specific public-interface tests passed.
- Focused scheduled-confirmation and native-registry ownership tests — 2 passed.
- `cargo check --all-targets --all-features` — passed.
- `cargo fmt --all --check` — passed.
- `cargo test --all-targets --all-features` — full suite passed (146 tests).
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.

## Blockers / decisions needed

None currently.

## Notes / handoff

- 2026-09-20 Copilot: Added the shared native scheduler API and complete
  `schedule ls/add/rm` CLI with stable human/JSON output, owned metadata,
  systemd user timers with safe cron fallback, launchd plists, and Windows Task
  Scheduler XML. Scheduled operation/alias commands use absolute executable and
  config paths, source history as `scheduled`, and skip collection prompts
  without weakening force checks. Added portable artifact/quoting tests, fake
  add/list/remove tests, partial-list diagnostics, and non-owned removal denial.
  All focused/full validation passed; no known task-scope limitations.
