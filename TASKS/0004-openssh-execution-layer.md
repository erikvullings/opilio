# 0004 — OpenSSH execution layer

**Status:** done
**Depends on:** 0002  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Delegate remote execution to system OpenSSH and provide one-shot plus multiplexed sessions.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Detect/invoke system `ssh`; do not embed an SSH protocol library.
- Treat configured SSH target as an OpenSSH host/alias and preserve user SSH config behavior.
- Implement one-shot command execution with timeout, stdout/stderr capture, and structured result.
- Support shell `command` execution via explicit configurable remote shell (Linux default `/bin/sh -lc`).
- Support structured `exec` with robust argument construction/escaping.
- Implement `opilio ssh <device>` as interactive handoff; reject groups/sites.
- Add a temporary ControlMaster/ControlPath abstraction for TUI polling, with cleanup on exit.
- Keep process execution behind a fakeable adapter for tests.

## Acceptance criteria

- Unit tests verify command construction/escaping/timeouts without requiring real SSH hosts.
- Interactive SSH resolves exactly one device.
- Multiplex lifecycle can be started/reused/closed through the library API.
- Existing OpenSSH configuration remains authoritative.

## Implementation notes

- Prefer a vertical, demonstrable slice through shared library APIs rather than code that only a later task can exercise.
- Keep platform/hardware/process/network dependencies behind test seams.
- Treat JSON output and secret redaction as compatibility/security surfaces where applicable.
- Add dependencies only when they are maintained and materially reduce complexity.

## Decisions

- Device `shell` is an explicit remote executable path, defaulting to
  `/bin/sh`; command actions invoke it with `-lc`.
- OpenSSH host aliases are passed through unchanged and no connection settings
  are duplicated from the user's SSH configuration.
- Captured output defaults to 64 KiB per stream. The process boundary drains
  excess bytes to avoid deadlock and reports truncation separately.
- ControlMaster paths are caller-provided controller-local paths so the TUI can
  own placement; sessions use `ControlPersist=no`, support reuse, close
  explicitly, and retry best-effort cleanup on drop.

## Progress

- [x] Task picked up; status changed to `in-progress` in this file and root README.
- [x] Implementation completed.
- [x] Focused tests pass.
- [x] Full relevant test suite/lints pass.
- [x] Documentation/examples updated if behavior is user-visible.
- [x] Status changed to `done` and root README dashboard updated.

## Validation

- `cargo test --test ssh --test cli_ssh --quiet` — 8 task-focused SSH tests
  passed without contacting a remote host.
- `cargo check --all-targets --all-features` — passed.
- `cargo fmt --all -- --check` — passed.
- `cargo test --all-targets --all-features` — full suite passed (35 tests).
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.

Record commands/tests and concise results here before marking done.

## Blockers / decisions needed

None currently.

## Notes / handoff

- 2026-09-20 Copilot: Added system OpenSSH discovery, fakeable cross-platform
  process requests, bounded output, timeout/cancellation and exit-state
  reporting, safely quoted shell/structured exec construction, interactive
  single-device CLI handoff, and reusable/cleaned-up ControlMaster sessions.
  Verified 8 SSH-focused tests through public interfaces, all 35 tests, check,
  formatting, and strict clippy. Tests use process fakes and never contact
  remote hosts; no known task-scope limitations.
