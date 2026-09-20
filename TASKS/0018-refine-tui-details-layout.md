# 0018 — Refine TUI details layout

**Status:** done  
**Depends on:** 0017  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Improve the Details panel hierarchy and omit power information when no meter exists.

## Requirements

- Hide both power details and history when no watt samples exist.
- Align detail labels and values as a consistent two-column table.
- Add one blank row after the service/model block.
- Add one blank row after the RAM-used history.
- Preserve compact-terminal rendering and show power again when metering data exists.

## Acceptance criteria

- Renderer tests cover metered and unmetered devices.
- Renderer tests cover aligned rows and the requested spacing.
- Full tests, formatting, strict Clippy, and the Impeccable layout scan pass.

## Progress

- [x] Task picked up; status changed to `in-progress` in this file and root README.
- [x] Implementation completed.
- [x] Focused tests pass.
- [x] Full relevant test suite/lints pass.
- [x] Status changed to `done` and root README dashboard updated.

## Validation

- `cargo test --test tui --quiet` — 10 passed.
- `cargo test --quiet --all-targets --all-features` — full suite passed.
- `cargo clippy --quiet --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt --check` — passed.
- Impeccable layout detector over `src/tui/render.rs` — no findings.

## Blockers / decisions needed

None currently.

## Notes / handoff

Power visibility is inferred from current or historical watt data so a transient failed poll does not erase a previously useful graph.

Details use a fixed-width label column and an unconstrained value column. One-row gaps separate the service/model block from telemetry and RAM history from GPU history.
