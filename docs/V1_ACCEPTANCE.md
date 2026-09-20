# Opilio v1 acceptance evidence

This matrix maps every release criterion in
[`OPILIO_V1_SPEC.md`](OPILIO_V1_SPEC.md) §24 to implementation evidence.

| Criterion | Evidence |
|---|---|
| Portable YAML works on macOS, Linux, Windows | `tests/config.rs` checks all platform config paths; `tests/transfer.rs` checks cross-platform import paths and semantic round-trip; CI tests all three OSes. |
| `status`, `on`, graceful `off`, reboot, action, SSH | `tests/v1_acceptance.rs` exercises the shared CLI APIs with fake boundaries; `tests/cli_lifecycle.rs` covers reboot and safety. |
| Tested Shelly and WoL providers | `tests/power_shelly.rs`, `tests/power_wol.rs`. |
| Group/site targets and controlled concurrency | `tests/status.rs`, `tests/cli_status.rs`, `tests/action.rs`, `tests/lifecycle.rs`. |
| `--yes` cannot grant force | `tests/cli_lifecycle.rs::yes_skips_confirmation_but_does_not_grant_force`. |
| Nonblocking flock TUI | `tests/tui.rs` covers reducer/rendering, long operations, polling backoff/recovery, safety dialogs, and bounded graphs. |
| DGX Spark UMA is not represented as VRAM | `tests/telemetry.rs` covers GB10 unified-memory fixtures and unsupported conventional memory. |
| Versioned JSON and distinct partial success | Stable fixtures and schema assertions in `tests/cli_status.rs`, `tests/cli_action.rs`, `tests/cli_lifecycle.rs`, `tests/cli_doctor.rs`, `tests/cli_scheduler.rs`, `tests/cli_history.rs`, and `tests/cli_transfer.rs`; exit codes are covered in `tests/status.rs` and `tests/v1_acceptance.rs`. |
| Native scheduler add/list/remove | `tests/scheduler.rs` generates systemd, cron, launchd, and Task Scheduler artifacts without host mutation; `tests/cli_scheduler.rs` uses a fake adapter for ownership and round-trip. |
| Import/export excludes keys and secrets | `tests/transfer.rs` checks selective recursive SSH export, private-key omission, resolved-secret rejection, archive path safety, bundle versioning, mappings, and round-trip. |
| History records operations and bounds failures | `tests/history.rs` checks rotation, locking-facing storage, UTF-8-safe failed-output tails, redaction, and malformed/truncated records; CLI integration tests record status/action/lifecycle/SSH. |
| Static config and runtime doctor are distinct | `tests/cli_config.rs` proves static checking does not probe; `tests/doctor.rs` and `tests/cli_doctor.rs` cover dependencies, target-only probes, site/VPN aggregation, advice, redaction, and exit aggregation. |

Release workflow evidence:

- `.github/workflows/ci.yml` runs formatting, strict Clippy, and the full suite
  on Linux, macOS, and Windows.
- `.github/workflows/release.yml` performs locked release builds for Linux
  x86-64, macOS Intel/Apple silicon, and Windows x86-64; it uploads one-binary
  archives plus SHA-256 files.
- `tests/v1_acceptance.rs` covers representative status/power/action/SSH and
  all native scheduler artifact families without hardware.

The runtime remains one executable using system OpenSSH. There is no Opilio
daemon, remote agent, database, container, or language-runtime dependency.
