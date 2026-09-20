# 0010 — Generic service health

**Status:** done
**Depends on:** 0004  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Add configurable generic service status/health/info probes without hard-coding vLLM/SGLang.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Implement generic service definitions using configured SSH command and/or HTTP endpoints.
- Support status, health, and info probes with timeouts.
- Allow configured extraction of useful JSON fields such as model name while keeping the domain generic.
- Represent stopped/loading/ready/error/unknown states where evidence supports them.
- Poll service health less frequently than system telemetry by default.
- Do not add Docker/systemd/vLLM/SGLang management APIs; named actions remain the control mechanism.

## Acceptance criteria

- Mock HTTP/SSH fixtures demonstrate ready, loading, stopped, error, malformed, timeout states.
- A vLLM-compatible `/v1/models` response can yield a model name through generic configuration.
- No provider-specific service code is required for the example.

## Implementation notes

- Prefer a vertical, demonstrable slice through shared library APIs rather than code that only a later task can exercise.
- Keep platform/hardware/process/network dependencies behind test seams.
- Treat JSON output and secret redaction as compatibility/security surfaces where applicable.
- Add dependencies only when they are maintained and materially reduce complexity.

## Decisions

- Status commands run through the shared OpenSSH execution layer. HTTP health
  and info URLs are called from the controller through a separate test seam.
- Probe timeouts default to 5 seconds. The exported service polling default is
  7 seconds, compared with the existing 2-second telemetry default.
- State mappings are configuration-driven, case-insensitive exact matches
  against text or JSON scalar values. A 2xx health response is `ready` when no
  configured state matches; other HTTP statuses are `error`.
- Info extraction maps generic output names to RFC 6901 JSON pointers. Info
  failures remain visible without overriding independently established health.
- HTTP header values accept environment secret references only. Neither
  resolved values nor reference names appear in observations or errors.

## Progress

- [x] Task picked up; status changed to `in-progress` in this file and root README.
- [x] Implementation completed.
- [x] Focused tests pass.
- [x] Full relevant test suite/lints pass.
- [x] Documentation/examples updated if behavior is user-visible.
- [x] Status changed to `done` and root README dashboard updated.

## Validation

- `cargo test --test service --test status --test config --test cli_status --quiet`
  — passed; includes 13 new task tests (11 service, 1 config, 1 status).
- `cargo check --all-targets --all-features` — passed.
- `cargo fmt --all -- --check` — passed.
- `cargo test --all-targets --all-features --quiet` — full suite passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.

## Blockers / decisions needed

None currently.

## Notes / handoff

- 2026-09-20 Copilot: Added generic command/HTTP service collection with
  explicit timeouts and errors, configured state normalization, JSON-pointer
  info extraction, redacted environment-backed headers, OpenSSH/ControlMaster
  reuse, additive human/JSON status DTOs, and slower service polling defaults.
  Verified ready/loading/stopped/error/unknown, combinations, malformed data,
  failures, timeouts, model extraction, and credential redaction through fakes
  and local mock HTTP servers. All focused and full validation passed.
