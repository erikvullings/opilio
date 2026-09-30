# 0024 — Show RAM GiB in graph

**Status:** done
**Depends on:** 0017, 0023
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Show the current absolute RAM usage in the RAM history title without changing its percentage scale.

## Context

The RAM graph currently shows only the percentage value. Users also need the concrete memory consumption at a glance, such as `110.0 GiB`, while interpreting the graph against its fixed 0–100% scale.

## Requirements

- Show current used GiB and percentage in the RAM graph title when byte telemetry is available.
- Keep RAM graph history, minimum, maximum, and vertical scale percentage-based.
- Fall back to the existing percentage-only title when used-byte telemetry is unavailable.
- Do not change GPU or power graph titles.

## Acceptance criteria

- RAM title renders `now 92.0 GiB (72.4%)` for matching telemetry.
- Missing used-byte telemetry still renders the current percentage.
- TUI tests, full tests, formatting, and strict Clippy pass.

## Progress

- [x] Added coverage for absolute usage and percentage-only fallback.
- [x] Added current used GiB to the RAM graph title when available.
- [x] Completed validation.

## Validation

- `cargo test --test tui --quiet`
- `cargo test --quiet --all-targets --all-features`
- `cargo clippy --quiet --all-targets --all-features -- -D warnings`
- `cargo fmt --check`
- Impeccable detector: no findings for `src/tui/render.rs` and `tests/tui.rs`

## Blockers / decisions needed

None.

## Notes / handoff

The existing `used / total` details row remains unchanged. `metric_history` accepts an optional absolute current value so GPU and power titles retain their existing formats.
