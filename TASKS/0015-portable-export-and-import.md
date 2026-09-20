# 0015 — Portable export and import

**Status:** done
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

- [x] Task picked up; status changed to `in-progress` in this file and root README.
- [x] Implementation completed.
- [x] Focused tests pass.
- [x] Full relevant test suite/lints pass.
- [x] Documentation/examples updated if behavior is user-visible.
- [x] Status changed to `done` and root README dashboard updated.

## Validation

- `cargo test --test transfer --test cli_transfer --quiet` — 8
  task-specific public-interface tests passed.
- `cargo check --all-targets --all-features` — passed.
- `cargo fmt --all -- --check` — passed.
- `cargo test --all-targets --all-features --quiet` — full suite passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.

## Decisions

- Bundles are uncompressed versioned tar files with fixed, single-component
  members (`manifest.yaml`, `flock.yaml`, `ssh_config`); imports reject unknown,
  duplicate, non-file, oversized, absolute, and traversal members before writes.
- Export normalizes validated flock YAML, strips SSH comments/includes and
  controller-local `IdentityFile` paths, recursively follows exact
  `ProxyJump` aliases, and rejects executable SSH directives or any selected
  content containing a resolved configured secret.
- Imported SSH entries live in `~/.ssh/opilio/config`; one idempotent `Include`
  is prepended to the user's SSH config so exact entries precede broad defaults.
- Interactive mapping is collected once and applies replacement user/key values
  only to entries that declared those values. Noninteractive import never
  prompts and reports missing environment variables and key mappings.

## Blockers / decisions needed

None currently.

## Notes / handoff

- 2026-09-20 Copilot: Added versioned safe tar export/import, selective exact
  OpenSSH extraction with recursive ProxyJump inclusion, private-key and secret
  exclusion, fixed-member/path validation, atomic destination writes, isolated
  idempotent SSH include management, one-call interactive username/key mapping,
  noninteractive unresolved-requirement reports, versioned human/JSON CLI
  output, and static-validation-before-targeted-doctor sequencing. Verified 8
  focused tests plus check, formatting, full suite, and strict clippy; no known
  task-scope limitations.
