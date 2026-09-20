# Opilio

Opilio is a cross-platform Rust CLI/TUI for managing a small **flock** of remote machines without installing an Opilio agent on them. It combines safe power control (Shelly/WoL), system OpenSSH, named actions, telemetry, generic service/LLM health, native scheduling, structured output, and a LazyDocker-style TUI.

The product decisions from the requirements grill are captured in [`docs/OPILIO_V1_SPEC.md`](docs/OPILIO_V1_SPEC.md). Coding agents should treat that document as the v1 product contract and [`AGENTS.md`](AGENTS.md) as the task workflow.

See the [v1 user guide](docs/USER_GUIDE.md) for installation, setup, safety,
scheduling, migration, and troubleshooting. The
[acceptance matrix](docs/V1_ACCEPTANCE.md) links every release criterion to
automated evidence.

## Task workflow

Tasks are independent Markdown files under [`TASKS/`](TASKS/). Each task contains enough context, requirements, dependencies, implementation guidance, acceptance criteria, and validation steps for a fresh coding-agent session to pick it up. Tasks should be completed in dependency order, but independent tasks may be worked in parallel.

The task decomposition follows the tracer-bullet principle used by Matt Pocock's `to-tickets` skill: work is split into small demonstrable slices with explicit blocking edges rather than building all infrastructure first and integrating at the end. His current engineering skills describe `to-tickets` as producing tracer-bullet tickets sized for fresh agent sessions, with dependencies declared explicitly.

## Status dashboard

| Task | Status | Depends on | Outcome |
|---|---|---|---|
| [0001 Bootstrap Rust application](TASKS/0001-bootstrap-rust-application.md) | done | — | Buildable cross-platform CLI/library skeleton |
| [0002 Load and validate configuration](TASKS/0002-load-and-validate-configuration.md) | done | 0001 | Strict portable YAML + target model |
| [0003 Status and structured output](TASKS/0003-status-and-structured-output.md) | done | 0002 | `status`, target resolution, stable JSON/exit codes |
| [0004 OpenSSH execution layer](TASKS/0004-openssh-execution-layer.md) | done | 0002 | System SSH, commands/exec, multiplexing seam |
| [0005 Named actions and aliases](TASKS/0005-named-actions-and-aliases.md) | done | 0003, 0004 | Safe named remote actions + one-op aliases |
| [0006 Shelly power provider](TASKS/0006-shelly-power-provider.md) | done | 0003 | Local Shelly switching + electrical telemetry |
| [0007 Wake-on-LAN provider](TASKS/0007-wake-on-lan-provider.md) | done | 0003 | WoL power-on support |
| [0008 Safe lifecycle operations](TASKS/0008-safe-lifecycle-operations.md) | done | 0004, 0006, 0007 | on/off/reboot/power-cycle safety semantics |
| [0009 System and NVIDIA telemetry](TASKS/0009-system-and-nvidia-telemetry.md) | done | 0004 | CPU/RAM/GPU + DGX Spark UMA-aware telemetry |
| [0010 Generic service health](TASKS/0010-generic-service-health.md) | done | 0004 | Generic health/status/info incl. LLM model name |
| [0011 Operation history and logging](TASKS/0011-operation-history-and-logging.md) | done | 0005, 0008 | Rotating JSONL history + bounded failure output |
| [0012 Build the TUI dashboard](TASKS/0012-build-tui-dashboard.md) | done | 0008, 0009, 0010, 0011 | Keyboard-first live fleet dashboard |
| [0013 Diagnostics and doctor](TASKS/0013-diagnostics-and-doctor.md) | done | 0006, 0007, 0009, 0010 | Static config check + runtime diagnostics |
| [0014 Native scheduling adapters](TASKS/0014-native-scheduling-adapters.md) | done | 0005, 0011 | Linux/macOS/Windows schedule ls/add/rm |
| [0015 Portable export and import](TASKS/0015-portable-export-and-import.md) | done | 0002, 0004, 0013 | Safe setup migration + selective SSH config |
| [0016 Cross-platform hardening and release](TASKS/0016-cross-platform-hardening-release.md) | done | 0012, 0013, 0014, 0015 | v1 acceptance, docs, packaging, CI |

**Overall status:** v1 complete. `16 / 16` tasks done.

## Suggested milestones

**Milestone 1 — useful CLI:** 0001–0008. At the end, Opilio can load a flock, resolve targets, use SSH, run named actions, and safely control Shelly/WoL devices.

**Milestone 2 — observability and TUI:** 0009–0012. At the end, Opilio can monitor system/GPU/power/service state and expose it through the live TUI.

**Milestone 3 — operational polish:** 0013–0016. Diagnostics, native scheduling, migration between controller computers, and cross-platform release hardening complete v1.

## Key v1 constraints

Opilio ships as one Rust executable for macOS, Linux, and Windows. System OpenSSH is the intentional SSH dependency. There is no Opilio daemon or remote agent. Configuration is portable YAML; secrets are environment-variable references. Sites describe reachability contexts but do not manage VPNs. Aliases are single-operation shortcuts, not workflows. Telemetry history is in-memory; operation history is persistent rotating JSONL.

## Starting work

A coding agent should start with `TASKS/0001-bootstrap-rust-application.md`. Once a task is picked up, change its `Status` to `in-progress` and update this README dashboard. When finished and validated, mark it `done`, record validation evidence in the task, update the dashboard, and select the next unblocked task.

## Development

```sh
cargo run -- --help
cargo test --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

## Configuration

Opilio loads strict YAML from `--config <path>`, then `OPILIO_CONFIG`, then the
platform default documented in the [v1 specification](docs/OPILIO_V1_SPEC.md).
Start from [`examples/config.yaml`](examples/config.yaml); secrets must remain
environment references such as `${env:OPILIO_SHELLY_HOME_PASSWORD}`.

```sh
opilio config path
opilio config check
opilio doctor [device|group|site|all] [--json] [--quiet]
opilio status [device|group|site|all]
opilio on <device|group|site|all> [--wait] [--yes] [--parallel N]
opilio off <device|group|site|all> [--yes] [--force] [--parallel N]
opilio shutdown <device|group|site|all> [--yes] [--parallel N]
opilio reboot <device|group|site|all> [--yes] [--parallel N]
opilio power-off <device|group|site|all> --force [--yes] [--parallel N]
opilio power-cycle <device|group|site|all> --force [--yes] [--parallel N]
opilio device ls
opilio group ls
opilio site ls
opilio action ls
opilio action run <name> <device|group|site|all> [--parallel N]
opilio alias run <name>
opilio schedule ls [--json]
opilio schedule add <id> --at HH:MM [--json] -- <operation...>
opilio schedule rm <id> [--json]
opilio export <bundle> [--json] [--quiet]
opilio import <bundle> [--non-interactive] [--json] [--quiet]
opilio ssh <device>
opilio history [device|group|site|all] [--json]
opilio history show <id> [--json]
```

Running `opilio` without a subcommand opens the interactive fleet dashboard.
Use arrows or `h/j/k/l` to move between sites/groups and devices, `/` to
filter, `R` to refresh, `o`/`x`/`r` for lifecycle operations, `a` for named
actions, `s` for a system-OpenSSH handoff, and `?` for help. Collection
operations require confirmation. Polls and long operations run off the render
loop; unreachable sites back off automatically and manual refresh retries
immediately. Telemetry graphs are intentionally memory-only.

`export` writes a versioned tar bundle containing normalized portable flock
YAML, only exact OpenSSH `Host` entries used by configured devices and their
`ProxyJump` chain, and names/labels for required environment variables and SSH
keys. It never reads or embeds private keys, removes controller-local
`IdentityFile` paths, and refuses content containing a resolved secret.

`import` validates the complete bundle before writing, installs SSH entries in
the separate `~/.ssh/opilio/config` include, then statically reloads the
installed flock and runs targeted `doctor` checks. Interactive import asks once
for replacement SSH username/key mappings. `--non-interactive` installs only
safe pieces and returns partial-success (`3`) with unresolved environment/key
requirements; malformed, unsupported, or path-unsafe bundles return `2`.

`config check` only parses and statically validates configuration. It performs
no network, process, or hardware probes.

`doctor` is the separate runtime diagnostic path. It checks local system
OpenSSH, then only the configured resources selected by its optional target:
endpoint reachability, SSH authentication, power and telemetry providers,
generic services, and informative NVIDIA/UMA, Docker, and systemd capabilities.
Likely site-wide network/VPN failures are grouped without guessing that hosts
are powered off. Human and versioned JSON results include advisory corrections,
never edit configuration, redact configured secret references and resolved
values, and use the standard success/failure/partial-success exit codes. Doctor
does not scan the LAN.

`ssh` accepts exactly one configured device and hands the terminal directly to
system OpenSSH, preserving aliases, keys, ProxyJump, agents, and host-key rules
from the user's OpenSSH configuration. Groups and sites are rejected. A device
may set `shell` for remote command actions; it defaults to `/bin/sh`, invoked
explicitly with `-lc`.

`status` defaults to all devices and currently reports the reachability-neutral
`configured` state. Devices with `telemetry.provider: system` or `nvidia` also
collect Linux CPU/load, RAM, and uptime over OpenSSH; the NVIDIA provider
discovers supported `nvidia-smi` fields and adds GPU utilization, temperature,
power, and memory semantics. DGX Spark/GB10 reports `unified` memory and marks
conventional GPU-memory totals/usage unsupported rather than presenting them as
VRAM. Unavailable and unsupported providers/metrics remain explicit in JSON.
TUI callers can reuse the same provider API with an OpenSSH ControlMaster and
bounded in-memory time-series buffers. Use `--json` for the stable versioned
result document, `--quiet` to rely on the exit code alone, and `--parallel N` to
bound concurrent device work. JSON includes `schema_version`, the requested
`target`, aggregate counts, and sorted per-device `device`, `site`, `ssh`,
`status`, and `error` fields; the optional `telemetry` field is omitted for
devices without a configured provider.

Devices may also reference generic `services`. A service can combine a remote
SSH `status.command` with controller-side HTTP `health` and `info` probes.
Configure `states.stopped/loading/ready/error` as exact text or JSON scalar
values; Opilio does not embed Docker, systemd, vLLM, or SGLang semantics.
Successful 2xx health responses are ready unless a configured state value
matches. `info.extract` maps stable field names to RFC 6901 JSON pointers, for
example `model: /data/0/id`. Probe timeouts default to 5 seconds; live clients
should poll services every 7 seconds by default, slower than system telemetry.
HTTP header values must be environment secret references and resolved values
and reference names are omitted from status output and errors. Service results
are added under each configured device's optional `services` JSON field.

Named actions resolve implementations per device in the order device override,
one unambiguous group override, then default. Shell actions use each device's
explicit shell; structured `exec` actions quote arguments without shell
interpolation. Actions are sequential by default; `--parallel N` opts into
bounded concurrency. Human, `--json`, and `--quiet` output retain every device
result and use the standard success/failure/partial-success exits.

Configured Shelly power providers use the local device API and automatically
handle Gen1 and RPC-generation devices. The shared power API exposes outlet
on/off/state plus voltage, current, watts, and energy when supported; unavailable
measurements remain explicitly unknown or unsupported. Optional Basic/Digest
authentication resolves passwords only from configured environment references.

Wake-on-LAN providers validate a unicast MAC address and send the standard magic
packet to `255.255.255.255:9` by default. Set `broadcast` to an explicit IPv4
socket address such as `192.168.1.255:9` for a directed broadcast. WoL requests
startup only: it cannot observe outlet state or cut physical power, and Opilio
reports those limitations rather than inferring that a machine powered on or off.

Lifecycle operations are sequential by default and continue across independent
device failures; `--parallel N` sets a bounded override. Collection operations
show and confirm resolved devices unless `--yes` explicitly confirms them.
`--yes` never grants force: physical `power-off`/`power-cycle` and graceful
shutdown bypass require `--force`. Normal `off` sends the configured SSH
shutdown command, waits for SSH to become unreachable, and only then cuts a
configured Shelly outlet when `shutdown.cut_power` is enabled. WoL-only devices
stop after graceful shutdown. `on` returns after the power request unless
`--wait` is supplied, which waits only for SSH readiness. Human and versioned
JSON output report truthful per-device transition states and aggregate exits.

`alias run` expands a configured alias exactly once to its fixed operation,
target, and options. Aliases cannot reference aliases or contain workflow
steps. Read-only status aliases default to four workers; disruptive aliases
default to one. Lifecycle aliases use the same confirmation, safety, execution,
and result path as direct lifecycle commands.

CLI operations append one JSONL history record per resolved device in the
platform-local Opilio data directory. Records include a stable ID, timestamp,
source, operation/action, requested and resolved targets, duration, result,
exit code, and force flag. Successful remote output is never stored. Failed
stdout, stderr, and errors are redacted and retain only a UTF-8-safe tail
(64 KiB by default). `history` reports malformed or truncated records instead
of treating them as successful operations. Rotation defaults to four 4 MiB
files and uses a cross-process lock.

History storage can be tuned with `OPILIO_HISTORY_DIR`,
`OPILIO_HISTORY_MAX_FILE_BYTES`, `OPILIO_HISTORY_MAX_FILES`, and
`OPILIO_HISTORY_FAILURE_TAIL_BYTES`. Native scheduler integrations can pass the
hidden global `--source scheduled` option; shared TUI/library callers use the
typed `OperationSource` API.

`schedule add` installs a daily, local-time job using a systemd user timer on
Linux when the user manager is available (otherwise the existing crontab),
launchd on macOS, or Task Scheduler on Windows. The command after `--` must be
a deterministic operation (`status`, lifecycle, `action run`, or `alias run`);
interactive SSH and scheduler recursion are rejected. Scheduled operations run
with the selected absolute config path, are inherently confirmed, and record
history with source `scheduled`. Opilio marks and tracks its native artifacts,
lists/removes only those jobs, and delegates missed runs to the native
scheduler without catch-up logic.

Command exit codes are `0` for success, `1` when all device work fails, `2` for
configuration or usage errors, and `3` for partial success. Group, site, and
all-device status retain every per-device result rather than failing fast.
