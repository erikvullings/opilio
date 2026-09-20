# 0015 — Portable export and import

**Status:** open  
**Depends on:** 0002, 0004, 0013  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Make a configured flock easy to move between controller computers without exporting secrets.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Implement `export <bundle>` containing portable config, required environment-variable names, and only relevant OpenSSH Host entries plus required referenced jump hosts.
- Never include private SSH keys or resolved environment-secret values by default.
- Keep Opilio-managed SSH entries isolated from unrelated user config and removable/regenerable.
- Implement interactive import that can remap remote username/key/path once and apply to applicable hosts.
- Implement `--non-interactive`: import safe/resolvable pieces and report unresolved local setup.
- Handle platform path differences without baking controller-local paths into portable flock config.
- After import, run equivalent static validation and targeted doctor diagnostics.
- Use a versioned bundle manifest so future versions can migrate safely.

## Acceptance criteria

- Export tests prove unrelated SSH hosts/private keys/secret values are absent.
- Import tests cover username/key remapping and Windows/macOS/Linux path differences.
- Bundle version is validated.
- Round-trip preserves flock semantics while allowing controller-local setup differences.

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
