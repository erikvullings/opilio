# 0016 — Cross-platform hardening and release

**Status:** done
**Depends on:** 0012, 0013, 0014, 0015  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Close the v1 acceptance criteria and make Opilio distributable on macOS, Linux, and Windows.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Run full v1 acceptance matrix from the spec.
- Harden platform directory handling, process spawning, terminal behavior, scheduler detection, and OpenSSH discovery.
- Document installation, config examples, environment secrets, Shelly/WoL setup, TUI keys, scheduling, export/import, safety semantics, and troubleshooting.
- Add representative example config for local + VPN sites and DGX Spark/LLM service.
- Verify JSON compatibility fixtures and exit codes.
- Add release CI producing platform binaries/checksums and appropriate package-manager follow-up guidance (packaging channels may be separate follow-up tasks).
- Perform security/redaction review and failure-mode review.
- Ensure README task dashboard reflects completion.

## Acceptance criteria

- CI/release builds succeed on Linux, macOS, Windows.
- All v1 acceptance criteria in `docs/OPILIO_V1_SPEC.md` are checked off with evidence.
- Fresh-machine smoke test can import/configure and run `config check`, `doctor`, and `status`.
- No v1 feature requires an Opilio daemon or remote agent.

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

## Validation

- `cargo test --test v1_acceptance --test cli_action --test ssh --test config
  --test tui --test scheduler --test transfer` — 49 focused tests passed,
  including 5 v1 acceptance tests with representative fake adapters.
- `cargo test --test cli_transfer
  transfer_cli_emits_human_and_versioned_json_reports -- --exact` — fresh
  controller export/import/config-check/doctor/status smoke path passed.
- `cargo check --all-targets --all-features` — passed.
- `cargo fmt --all -- --check` — passed.
- `cargo test --all-targets --all-features` — full suite passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `cargo build --locked --release` — native release build passed.
- Native `.tar.gz` creation plus `shasum -a 256 -c` — passed.
- `yq eval '.' .github/workflows/{ci,release}.yml` — both workflows parsed.

## Decisions

- Release automation builds locked, single-binary archives for Linux x86-64,
  macOS Intel/Apple silicon, and Windows x86-64, emits a SHA-256 sidecar for
  each, and publishes tag assets. Package-manager channels remain follow-up
  distribution work.
- Unix TUI polling retains temporary OpenSSH ControlMaster reuse with shortened
  hashed socket names. Windows uses ordinary system OpenSSH because its client
  does not provide Unix control sockets.
- Configured resolved secrets are redacted from status, lifecycle, and action
  reports before human/JSON rendering or history recording. Captured process
  failures retain the bounded tail rather than the beginning.
- The v1 release version is `1.0.0`; system OpenSSH remains the only intentional
  external runtime dependency.

## Blockers / decisions needed

None currently.

## Notes / handoff

- 2026-09-20 Copilot: Audited every §24 acceptance criterion and recorded
  implementation/test evidence in `docs/V1_ACCEPTANCE.md`. Added representative
  fake-adapter status/on/off/action/SSH acceptance coverage, a fresh-controller
  import/config-check/doctor/status smoke path, all-platform native artifact
  generation checks, resolved-secret output regression tests, bounded process
  tail capture, safer terminal restoration, short Unix control sockets, and a
  non-multiplexed Windows TUI SSH path. Added the complete v1 user guide,
  expanded portable example, v1 version, and tag/manual release workflow with
  four platform archives and checksums. Focused tests, full suite, check,
  formatting, strict Clippy, native release build, workflow parsing, and local
  archive checksum verification passed. No known v1 task-scope gaps.
- 2026-09-20 Copilot: Closed all seven post-review findings: bounded runtime
  SSH status, transactional scheduler ownership and imports, lifetime TUI SSH
  pooling with bounded startup/eviction/shutdown, complete TUI failure
  fan-out, lazy history initialization, and versioned JSON/quiet read
  commands. Added failure-injection and public-interface regression coverage;
  task remains `done`.
