# 0019 — Diagnose lifecycle sudo failures

**Status:** done  
**Depends on:** 0008  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Make non-interactive sudo failures during shutdown and reboot actionable without exposing remote output.

## Context

A real TUI `off` operation failed immediately with exit code 1 because the configured `sudo shutdown -h now` required a password. Opilio intentionally runs remote lifecycle commands without interactive stdin, but the error did not explain that requirement.

## Requirements

- Keep lifecycle SSH execution non-interactive.
- When a failed lifecycle command invokes `sudo`, explain that non-interactive authorization may be required.
- Do not echo arbitrary stdout/stderr or resolved secrets.
- Lock the behavior at the production executor seam.

## Acceptance criteria

- Regression test reproduces a failed sudo shutdown through `SystemLifecycleExecutor`.
- Non-sudo failures retain the generic exit-code diagnostic.
- Full tests, formatting, and strict Clippy pass.

## Progress

- [x] Task picked up; status changed to `in-progress` in this file and root README.
- [x] Regression test fails before the fix.
- [x] Implementation completed.
- [x] Full relevant test suite/lints pass.
- [x] Status changed to `done` and root README dashboard updated.

## Validation

- History record `92577393-c5f9-4c39-a224-eb58cdb7b351` reproduced the TUI failure: exit code 1 after 478 ms at graceful shutdown.
- Safe remote probes proved SSH and `/usr/sbin/shutdown` work, while sudo requires password authentication for the exact shutdown command.
- `cargo test --test lifecycle production_executor_explains_noninteractive_sudo_failures_without_echoing_output --quiet` — failed before the fix and passed afterward.
- `cargo test --quiet --all-targets --all-features` — full suite passed.
- `cargo clippy --quiet --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt --check` — passed.
- `cargo run --quiet -- config check` — local configuration valid.

## Blockers / decisions needed

The Spark requires a controller-owner decision to add exact `NOPASSWD` sudoers rules for shutdown/reboot. Opilio does not modify remote sudo policy.

## Notes / handoff

Historical operation `92577393-c5f9-4c39-a224-eb58cdb7b351` failed in 478 ms at the graceful shutdown step. A safe remote probe confirmed `sudo: a password is required`.

The lifecycle executor now identifies failed `sudo` commands as likely non-interactive authorization failures without copying arbitrary remote output into the TUI or history.
