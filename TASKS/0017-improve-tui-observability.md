# 0017 — Improve TUI observability

**Status:** done  
**Depends on:** 0010, 0012  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Make memory, service/model, and telemetry history presentation unambiguous and useful during live operation.

## Context

The first real DGX Spark deployment exposed three usability gaps: `RAM` did not say whether it meant used or available memory, only the first configured service reached the TUI, and fixed-scale three-row graphs hid normal variation.

## Requirements

- Label RAM as used memory and show used/total GiB alongside percentage.
- Preserve and render all configured service observations for a device.
- Render model fields extracted as strings or arrays, including OpenAI-compatible `/v1/models` responses used by SGLang and LiteLLM.
- Give telemetry histories more vertical resolution, decimal precision, a visible time window, and current/min/max labels.
- Make unavailable power history explain that a power meter is required.
- Configure the local Spark controller for SGLang services on ports 8000 and 18005 without embedding credentials.

## Acceptance criteria

- TUI tests prove RAM semantics, multiple service rows, model-array summaries, and informative chart labels.
- Existing polling, keyboard controls, and low-height rendering remain safe.
- Local config passes static validation and runtime diagnostics clearly report each configured endpoint.
- Full tests, formatting, and strict Clippy pass.

## Progress

- [x] Task picked up; status changed to `in-progress` in this file and root README.
- [x] Implementation completed.
- [x] Focused tests pass.
- [x] Full relevant test suite/lints pass.
- [x] Documentation/examples updated.
- [x] Status changed to `done` and root README dashboard updated.

## Validation

- `cargo test --test tui --quiet` — 9 passed.
- `cargo test --quiet --all-targets --all-features` — full suite passed.
- `cargo clippy --quiet --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt --check` — passed.
- `cargo run --quiet -- config check` — local Spark configuration valid.
- Impeccable detector over changed TUI files — no findings.

## Blockers / decisions needed

None currently.

## Notes / handoff

The controller can reach port 8000 health without credentials, while model information requires an authorization token. Port 18005 was not listening during initial inspection and should be represented honestly rather than hidden.

The TUI retains every service observation, flattens configured `model`/`models` JSON values, and labels dynamically scaled two-minute histories with current/min/max values.
