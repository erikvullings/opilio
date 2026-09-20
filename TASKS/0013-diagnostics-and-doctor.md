# 0013 — Diagnostics and doctor

**Status:** done
**Depends on:** 0006, 0007, 0009, 0010  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Separate static configuration validation from runtime environment/device diagnostics.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Keep `config check` network-free and static.
- Implement `doctor [target]`: local OpenSSH availability, target reachability, SSH, power provider, telemetry capability, service probes, and useful remote capability observations.
- Detect informative remote capabilities such as NVIDIA/GB10/UMA, Docker, systemd without making them requirements.
- Aggregate likely site-level unreachability so VPN-off devices are not individually misdiagnosed as powered off.
- Suggest configuration snippets/corrections when useful, but never modify config automatically.
- No general LAN discovery.

## Acceptance criteria

- Doctor output distinguishes invalid config, missing local dependency, unreachable site/device, and unsupported capability.
- JSON form is machine-readable.
- Suggested snippets are advisory only and cannot mutate config.

## Implementation notes

- Prefer a vertical, demonstrable slice through shared library APIs rather than code that only a later task can exercise.
- Keep platform/hardware/process/network dependencies behind test seams.
- Treat JSON output and secret redaction as compatibility/security surfaces where applicable.
- Add dependencies only when they are maintained and materially reduce complexity.

## Progress

- [x] Task picked up; status changed to `in-progress` in this file and root README.
- [x] Implementation completed.
- [x] Focused tests pass.
- [x] Full relevant test suite/lints pass.
- [x] Documentation/examples updated if behavior is user-visible.
- [x] Status changed to `done` and root README dashboard updated.

## Decisions

- `doctor` exposes a versioned typed report through a shared library API; all
  process, network, hardware, and service work is behind `DoctorProbe`.
- Runtime failures are separated from advisory capability absences. Missing
  Docker/systemd, non-UMA hardware, and unsupported optional telemetry warn but
  do not fail the command.
- A site is reported as likely network/VPN-unreachable only when at least two
  selected devices in that site all fail reachability. No power state is
  inferred from reachability.
- Exit `1` means local OpenSSH is missing or every selected device has a
  required runtime failure; exit `3` means only some selected devices fail.

## Validation

- `cargo test --test doctor --test cli_doctor --quiet` — 12
  task-specific public-interface tests passed.
- `cargo check --all-targets --all-features` — passed.
- `cargo fmt --all -- --check` — passed.
- `cargo test --all-targets --all-features --quiet` — full suite passed (136
  tests).
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.

## Blockers / decisions needed

None currently.

## Notes / handoff

- 2026-09-20 Copilot: Added `doctor [target]` with human/JSON/quiet output,
  typed actionable checks, standard aggregate exits, configured-target-only
  probing, local OpenSSH detection, reachability/SSH classification, Shelly/WoL
  diagnostics, system/NVIDIA telemetry capability, DGX Spark/GB10 UMA
  detection, informative Docker/systemd presence, generic service probes,
  site/VPN failure aggregation, advisory suggestions, and secret redaction.
  Verified static `config check` never enters runtime probes and doctor never
  mutates configuration. All focused/full validation passed; no known
  task-scope limitations.
