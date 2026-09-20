# 0008 — Safe lifecycle operations

**Status:** done
**Depends on:** 0004, 0006, 0007  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Deliver safe on/off/shutdown/reboot/power-off/power-cycle behavior across devices and collections.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Implement `on`, `off`, `shutdown`, `reboot`, `power-off`, `power-cycle`.
- `on` returns after requesting power by default; `--wait` waits only for SSH availability.
- Normal `off`: graceful SSH shutdown -> wait/timeout -> cut Shelly power only when configured.
- `--yes` suppresses confirmation only. `--force` authorizes bypassing graceful shutdown/physical cut.
- Single-device routine operations need no prompt; disruptive group/site operations resolve/show affected devices and confirm unless `--yes`.
- Scheduled/non-interactive callers must be able to explicitly confirm without interactive input.
- Default disruptive parallelism is 1; failures do not stop unrelated devices.
- Represent powered/booting/SSH-ready/unreachable/unknown without overclaiming.

## Acceptance criteria

- Tests prove `--yes` alone cannot force physical cut.
- Group confirmation includes resolved devices.
- WoL-only off degrades to OS shutdown and never claims physical cut.
- `on --wait` stops at SSH readiness, not service/model readiness.
- Human/JSON result and partial failure behavior are covered.

## Implementation notes

- Prefer a vertical, demonstrable slice through shared library APIs rather than code that only a later task can exercise.
- Keep platform/hardware/process/network dependencies behind test seams.
- Treat JSON output and secret redaction as compatibility/security surfaces where applicable.
- Add dependencies only when they are maintained and materially reduce complexity.

## Decisions

- Lifecycle requests first produce a per-device safety plan. Frontends must
  explicitly confirm collection operations and every forced operation before
  executing that plan; scheduled callers use the same API with confirmation
  already recorded.
- `power-off` and `power-cycle` require `--force`. `off --force` bypasses SSH
  shutdown and waiting; normal `off` cuts Shelly power only after successful
  graceful shutdown and shutdown detection when `shutdown.cut_power` is true.
- Successful requests report transition states rather than inferred steady
  state: `booting`, `ssh_ready`, `shutting_down`, `rebooting`, `powered_off`,
  or `unreachable`. Failures remain `unknown`.

## Progress

- [x] Task picked up; status changed to `in-progress` in this file and root README.
- [x] Implementation completed.
- [x] Focused tests pass.
- [x] Full relevant test suite/lints pass.
- [x] Documentation/examples updated if behavior is user-visible.
- [x] Status changed to `done` and root README dashboard updated.

## Validation

- `cargo test --test lifecycle --test cli_lifecycle --quiet` — 18 focused
  public-interface lifecycle/planning/CLI tests passed.
- `cargo check --all-targets --all-features` — passed without warnings.
- `cargo fmt --all -- --check` — passed.
- `cargo test --all-targets --all-features` — full suite passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.

## Blockers / decisions needed

None currently.

## Notes / handoff

- 2026-09-20 Copilot: Added shared lifecycle safety planning/execution, typed
  truthful transition reports, production OpenSSH/Shelly/WoL orchestration,
  bounded concurrency and continuation, interactive/explicit confirmation,
  force-only bypass/cut/cycle paths, CLI and lifecycle-alias dispatch, and
  stable human/JSON output. Verified 18 task-focused tests, full suite, check,
  formatting, and strict clippy; no known task-scope limitations.
