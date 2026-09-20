# 0002 — Load and validate configuration

**Status:** done
**Depends on:** 0001  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Implement portable YAML loading, strict validation, domain objects, and target/action resolution.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Implement config lookup order: `--config`, `OPILIO_CONFIG`, platform default.
- Model devices, optional single site, groups, actions, aliases, services, shutdown/power/telemetry settings.
- Support `${env:NAME}` references without leaking resolved values.
- Validate unknown references, duplicate/conflicting membership, mutually exclusive `command`/`exec`, and malformed provider settings.
- Implement action override resolution: device > one unambiguous applicable group > default; overlapping conflicting group overrides are errors.
- Implement target resolution for device/group/site/all.
- Add `config path`, `config check`, and list commands for devices/groups/sites/actions.
- Keep YAML Git-friendly and controller-local state out of the primary config.

## Acceptance criteria

- Valid sample config loads on all platforms.
- Invalid references/ambiguous overrides fail with actionable diagnostics.
- Unit tests cover config precedence, target resolution, override precedence, ambiguity, and secret redaction.
- `opilio config check` performs no network calls.

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

## Validation

- `cargo test --test config --test cli_config --quiet` — 14 task-focused tests passed.
- `cargo test --all-targets --all-features` — full suite passed (16 tests).
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt --check` — passed.
- All six required CLI commands smoke-tested against `examples/config.yaml`.

## Blockers / decisions needed

None currently.

## Notes / handoff

- 2026-09-20 Copilot: Added strict typed YAML models and validation, portable
  path precedence, redacted environment secret references, target/action
  resolution, configuration/list CLI commands, and a validated sample.
  Verified 14 focused tests, the 16-test full suite, strict clippy, formatting,
  and command-line smoke tests. No known limitations within task scope.
