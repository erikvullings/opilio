# 0028 — Show missing secrets in TUI

**Status:** done
**Depends on:** 0021, 0022
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Show missing password/environment-variable errors clearly in the TUI without exposing secret values.

## Context

The TUI `on` operation for spark-0 failed because its Shelly password variable was unavailable to the running process, but the device stayed marked unreachable and the failure text was below the graphs. When SSH is also unreachable, polling replaces the Shelly provider error with the SSH error.

## Requirements

- Keep an operation failure visible even when subsequent status polls fail.
- Preserve Shelly configuration/secret errors alongside SSH reachability errors.
- Show actionable errors within the selected device's details on shorter terminals.
- Do not show secret values.

## Acceptance criteria

- A missing Shelly password variable is visible in a rendered TUI frame even if SSH is unreachable.
- An operation failure naming a missing variable is visible on a compact terminal.
- Confirmed outlet-off behavior still hides expected SSH loss.
- Targeted TUI tests, formatting, and strict Clippy pass.

## Progress

- [x] Reproduced the missing-variable operation failure in live history.
- [x] Added failing regression tests for error propagation and compact rendering.
- [x] Implemented the fix.
- [x] Validated behavior.

## Validation

- `cargo test --quiet --lib tui::runtime::tests` — 10 passed.
- `cargo test --quiet --test tui` — 17 passed.
- `cargo fmt --check` — passed.
- `cargo clippy --quiet --all-targets --all-features -- -D warnings` — passed.
- `git diff --check` — passed.

## Blockers / decisions needed

None.

## Notes / handoff

The Shelly error was lost when SSH failed, and errors below the graphs were
clipped on short terminals. Both are now preserved and rendered above graphs.
Operation failures also name the failure in the TUI footer. Keep
status-polling errors separate from operation failures.
