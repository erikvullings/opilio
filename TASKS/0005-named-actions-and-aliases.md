# 0005 — Named actions and aliases

**Status:** done
**Depends on:** 0003, 0004  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Run deterministic configured remote actions and one-operation aliases across targets.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Implement `action ls` and `action run <name> <target>`.
- Execute either `command` or structured `exec`; no runtime parameters/templates.
- Honor cwd, timeout, default/group/device override resolution.
- Use conservative default parallelism for disruptive actions; support `--parallel N`.
- Implement aliases as exactly one existing Opilio operation plus target/options.
- Do not add workflows, loops, conditions, arbitrary group exec, or DAG semantics.
- Provide per-device human/JSON results and partial-success exit code.

## Acceptance criteria

- Default and override action implementations are exercised in tests.
- Conflicting group override never resolves by order.
- Alias expansion is deterministic and cannot recurse into a workflow language.
- JSON results remain stable.

## Implementation notes

- Prefer a vertical, demonstrable slice through shared library APIs rather than code that only a later task can exercise.
- Keep platform/hardware/process/network dependencies behind test seams.
- Treat JSON output and secret redaction as compatibility/security surfaces where applicable.
- Add dependencies only when they are maintained and materially reduce complexity.

## Decisions

- Named actions use a shared executor API over the existing OpenSSH layer;
  command and structured-exec construction remains owned by that layer.
- Actions are conservatively sequential by default. `--parallel N` is the
  explicit bounded-concurrency override.
- `alias run <name>` performs one closed-enum operation expansion. Alias
  configuration cannot name another alias or contain workflow steps.
- Read-only status aliases default to four workers; disruptive aliases default
  to one. Lifecycle alias dispatch remains with the lifecycle operation tasks.

## Progress

- [x] Task picked up; status changed to `in-progress` in this file and root README.
- [x] Implementation completed.
- [x] Focused tests pass.
- [x] Full relevant test suite/lints pass.
- [x] Documentation/examples updated if behavior is user-visible.
- [x] Status changed to `done` and root README dashboard updated.

## Validation

- `cargo test --test action --test alias --test cli_action --test config
  --quiet` — 11 new task tests plus 12 configuration tests passed, including
  override precedence and ambiguity.
- `cargo run --quiet -- --config examples/config.yaml action ls --json` —
  smoke test passed.
- `cargo check --all-targets --all-features` — passed.
- `cargo fmt --all -- --check` — passed.
- `cargo test --all-targets --all-features` — full suite passed (46 tests).
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.

## Blockers / decisions needed

None currently.

## Notes / handoff

- 2026-09-20 Copilot: Added named action list/run through shared library and CLI
  APIs, OpenSSH-backed command/structured-exec execution, cwd/timeout and
  device/group/default resolution, bounded sequential-by-default target
  execution, stable human/JSON aggregation and exits, and closed one-operation
  aliases with option validation and cycle/workflow prevention. Verified 11
  new focused tests, existing precedence/ambiguity coverage, all 46 tests,
  check, formatting, strict clippy, and a CLI smoke test. Lifecycle aliases
  resolve now and intentionally dispatch only when their dedicated operations
  are implemented by later tasks.
