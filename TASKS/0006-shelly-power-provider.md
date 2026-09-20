# 0006 — Shelly power provider

**Status:** done
**Depends on:** 0003  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Implement direct local Shelly control and electrical telemetry behind a power-provider interface.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Define a generic power-provider interface used by application logic.
- Implement Shelly outlet on/off/state through the local device API.
- Read available voltage/current/watts and useful energy values.
- Support optional auth using environment-secret references; never log resolved credentials.
- Model unreachable/unsupported metrics explicitly rather than fabricating values.
- Use mock HTTP tests; hardware tests are opt-in/manual.
- Do not add SSH tunneling or VPN control.

## Acceptance criteria

- Mocked Shelly can be switched/read through the provider interface.
- Power telemetry maps to common DTOs.
- Auth values are redacted from diagnostics/errors/loggable structures.
- Network failures yield explicit unknown/unreachable states.

## Implementation notes

- Prefer a vertical, demonstrable slice through shared library APIs rather than code that only a later task can exercise.
- Keep platform/hardware/process/network dependencies behind test seams.
- Treat JSON output and secret redaction as compatibility/security surfaces where applicable.
- Add dependencies only when they are maintained and materially reduce complexity.

## Decisions

- The shared provider API uses separate typed outlet commands and observed
  states. Telemetry distinguishes values, unsupported capabilities, and
  temporarily unknown values.
- Generation is detected through `/shelly`: legacy responses use Gen1
  endpoints; Gen2 and later use the compatible RPC switch endpoints.
- HTTP Basic and RFC 7616 Digest challenges are handled by the production
  transport. Tests use the public fakeable HTTP boundary and require no
  hardware.

## Progress

- [x] Task picked up; status changed to `in-progress` in this file and root README.
- [x] Implementation completed.
- [x] Focused tests pass.
- [x] Full relevant test suite/lints pass.
- [x] Documentation/examples updated if behavior is user-visible.
- [x] Status changed to `done` and root README dashboard updated.

## Validation

- `cargo test --test power_shelly --quiet` — 10 task-focused tests passed.
- `cargo check --all-targets --all-features` — passed.
- `cargo test --all-targets --all-features` — full suite passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt --check` — passed.

## Blockers / decisions needed

None currently.

## Notes / handoff

- 2026-09-20 Copilot: Added typed shared power DTOs/provider interface and a
  local Shelly implementation covering Gen1 and RPC generations, outlet
  on/off/state, capability-aware electrical telemetry, cached generation
  detection, Basic/Digest environment-secret auth, explicit timeouts/errors,
  and secret redaction. Added 10 fake-HTTP public-interface tests; check, full
  suite, strict clippy, and formatting pass. Lifecycle CLI commands remain
  intentionally scoped to task 0008.
