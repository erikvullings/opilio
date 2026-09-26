# 0021 — Distinguish TUI status errors

**Status:** done
**Depends on:** 0012, 0019
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Prevent expected SSH disconnects after a successful power-off from appearing as a failed TUI operation.

## Context

After `x` successfully shut down the Spark and cut Shelly power, the TUI displayed `Recent failure: OpenSSH failed ... Connection closed`. Operation history recorded the `off` operation as succeeded, and the Spark was unreachable as expected. The dashboard currently stores both polling errors and operation failures in `recent_failure`, so the banner incorrectly attributes status-polling noise to the completed operation.

## Requirements

- Keep lifecycle history and operation outcomes authoritative.
- Track status-polling details separately from operation failures.
- Clear an earlier operation failure after a successful operation.
- Treat SSH unreachability as expected when the power provider confirms the outlet is off.
- Preserve actionable status details for genuinely unreachable or degraded devices.

## Acceptance criteria

- A successful `off` does not leave a `Recent failure` banner.
- An outlet confirmed off does not attach an SSH connection error to its dashboard sample.
- Unexpected reachability failures remain visible as status details.
- Targeted TUI tests, formatting, and strict Clippy pass.

## Progress

- [x] Reproduced against the live Spark and correlated TUI output with successful history.
- [x] Regression tests failed before the fix.
- [x] Implementation completed.
- [x] Validation completed.

## Validation

- Live history record `0eb7820d-bb2e-4135-ab52-207ba361793b` recorded `off` as succeeded in 2190 ms.
- A safe SSH probe after the operation timed out, confirming the Spark was offline.
- `cargo test --quiet --all-targets --all-features` — passed.
- `cargo clippy --quiet --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt --check` — passed.
- `cargo run --quiet -- config check` — local configuration valid.

## Blockers / decisions needed

None.

## Notes / handoff

The initial sudo problem is resolved. `/etc/sudoers.d/opilio` matches the exact shutdown command with `Options: !authenticate`.

Polling errors now populate a separate `status_detail` field and render as `Status detail`, while `recent_failure` is reserved for operation failures. Successful operations clear an earlier operation failure. When Shelly confirms its outlet is off, expected SSH loss is represented by the `off` state without an SSH error.
