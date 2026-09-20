# 0002 — Load and validate configuration

**Status:** open  
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
