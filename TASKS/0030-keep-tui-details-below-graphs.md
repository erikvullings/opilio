# 0030 — Keep TUI details below graphs

**Status:** done
**Depends on:** 0028, 0029
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Keep device summary and graph placement stable when status errors appear.

## Context

Task 0028 placed status and operation errors above device details so long
errors would not be clipped on short terminals. This moves the entire main
interface whenever SSH or provider errors change. The user wants status
details under the graphs instead.

## Decisions

- Reserve an error region below the graphs, never above device details.
- Preserve the device summary on short terminals and continue to show
  actionable error text when space permits.
- Keep both recent operation failures and status-polling details visible
  without conflating them.

## Acceptance criteria

- An SSH error appears after the graphs without changing summary/graph
  positions at a normal terminal height.
- A missing password variable remains visible on a compact terminal.
- Targeted TUI tests, formatting, and strict Clippy pass.

## Progress

- [x] Wrote a failing placement regression for long DNS/SSH errors.
- [x] Moved the error region and validated compact rendering.

## Validation

- `cargo test --quiet --test tui` — 21 passed.
- `cargo clippy --quiet --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt --check` — passed.
- `git diff --check` — passed.
- `cargo build --release --quiet` — passed; installed and byte-compared `~/.local/bin/opilio`.

## Blockers / decisions needed

None.

## Notes / handoff

The error area is reserved below all graphs. Normal-height device summary and
graph positions do not shift when polling errors appear; compact terminals
preserve the device summary and show the actionable message by shrinking graph
space when necessary. Restart the running TUI to load the updated binary.
