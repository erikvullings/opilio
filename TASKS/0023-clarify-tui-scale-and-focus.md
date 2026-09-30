# 0023 — Clarify TUI scale and focus

**Status:** done
**Depends on:** 0017, 0018
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Make percentage graphs visually truthful and keyboard focus unmistakable.

## Context

GPU and RAM histories use an adaptive min/max scale. A stable 96% sample is centered in a narrow local range and can render around half-height, contradicting its near-maximum meaning. The selected device is marked, but the Devices panel itself has no clear focused state when navigating with arrow keys.

## Requirements

- Plot RAM and GPU percentages against a fixed 0–100 scale.
- Keep power graphs adaptive because watts have no universal maximum.
- Give the focused Sites/Groups or Devices panel an explicit textual and visual treatment.
- Preserve existing keyboard behavior, compact layout safety, and state colors.

## Acceptance criteria

- A 96% GPU sample renders near the top of the graph.
- The focused panel title includes `[focus]`.
- Focused panels use a distinct border and selected-row treatment.
- Left/right navigation visibly moves focus between panels.
- TUI tests, full tests, formatting, and strict Clippy pass.

## Progress

- [x] Reproduced the graph scaling cause in `metric_history`.
- [x] Added failing regressions for absolute percentage scaling and focus movement.
- [x] Fixed RAM/GPU history to use a 0–100 scale while preserving adaptive watts.
- [x] Added explicit `[focus]` labels, thick cyan borders, and selected-row emphasis.
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

This is a scoped refinement of the existing TUI visual language, not a redesign.
