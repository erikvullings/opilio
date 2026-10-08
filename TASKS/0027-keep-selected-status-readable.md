# 0027 — Keep selected status readable

**Status:** done
**Depends on:** 0023
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Ensure the selected device's status text remains legible on the cyan cursor row.

## Context

The device state span explicitly sets a green foreground, which overrides the
black foreground of the selected row and makes `running` difficult to read.

## Decisions

- Use the existing selected-row foreground for the status span when its device
  row is selected and focused; preserve semantic state colors otherwise.

## Progress

- [x] Add a renderer regression check and fix selected-row contrast.
- [x] Validate and include the fix in the pending release branch.

## Validation

- `cargo test --test tui selected_device_status_uses_readable_cursor_foreground
  --locked --quiet` — failed before the fix (green instead of black), passed
  after the fix.
- `cargo test --test tui --locked --quiet` — 16 tests passed.
- `cargo fmt --check` and strict `cargo clippy` — passed.
- Impeccable detector reported no findings for the changed UI and test.

## Blockers / decisions needed

None.

## Notes / handoff

The focused selection uses black-on-cyan for the state span; state colors
remain unchanged when the device list is not focused.
