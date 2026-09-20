# Opilio v1 Implementation Specification

## 1. Product summary

Opilio is a cross-platform Rust CLI/TUI for managing a small flock of remote machines without installing an Opilio agent on them. It is intended initially for DGX Spark and similar Linux systems, but the domain model is generic.

The executable is `opilio`. Running `opilio` with no arguments opens the TUI; every important operation must also be available through a scriptable CLI.

## 2. v1 goals

- Manage named devices, groups, and sites.
- Power devices on through Shelly smart plugs and/or Wake-on-LAN.
- Shut devices down gracefully over SSH and optionally cut physical power afterwards.
- Reboot and force power-cycle machines with explicit safety semantics.
- Delegate SSH to system OpenSSH and reuse `~/.ssh/config`/platform equivalent.
- Execute a small set of named remote actions using shell commands or structured exec.
- Show system, NVIDIA GPU/UMA, Shelly power, and generic service/LLM status.
- Provide a LazyDocker-style, keyboard-first TUI with live in-memory graphs.
- Schedule Opilio operations using native OS schedulers; no Opilio daemon.
- Provide stable JSON output and meaningful exit codes for automation/agents.
- Keep configuration portable and Git-friendly across multiple controller computers.
- Export/import non-secret setup information and selectively migrate Opilio SSH host entries.
- Use environment-variable secret references in v1.
- Maintain a rotating local operation history/log.
- Support macOS, Linux, and Windows from v1.

## 3. Explicit non-goals

- No remote Opilio agent.
- No Ansible-like arbitrary orchestration language, DAGs, loops, conditions, or workflows.
- No arbitrary group `exec` command as the primary remote automation model.
- No VPN management.
- No general LAN discovery in v1.
- No persistent telemetry database, Prometheus, or daemon.
- No encrypted secret vault or native credential-store requirement in v1.
- No runtime action parameters/templates in v1.
- No config editor in the TUI.
- No embedded terminal or full Docker/systemd/vLLM administration client.
- No Opilio missed-schedule catch-up semantics.

## 4. Domain model

### Device
A managed machine. It may belong to zero or one site and zero or more groups. It has an SSH target and optional power/telemetry/service configuration.

### Site
An optional single-valued organizational/reachability property. Sites represent network contexts such as `home` or `lab-vpn`. Opilio observes reachability but does not connect VPNs. Sites are valid CLI/TUI targets but do not participate in action override inheritance.

### Group
An arbitrary logical collection of devices. Devices may belong to multiple groups. Groups can provide action overrides. If multiple applicable groups override the same action and no device override resolves the ambiguity, configuration resolution must fail rather than choose by order/priority.

### Action
A named, predetermined remote operation. It is either a shell `command` or structured `exec`, never both. Actions can have a default implementation plus group/device overrides. Resolution order: device > one unambiguous group > default.

### Alias
A shortcut for exactly one Opilio operation plus target/options. It is not a workflow.

### Service
A generic application/service observation configured by the user. Opilio understands generic health/status/info, not vLLM, SGLang, Docker, or systemd semantics. An LLM service may expose a health endpoint and model-information endpoint.

### Power provider
A mechanism capable of changing physical/boot state. v1 providers: Shelly and Wake-on-LAN. A device may use the mechanism(s) applicable to it.

## 5. Configuration

Default config locations:

- Linux: `$XDG_CONFIG_HOME/opilio/config.yaml`, fallback `~/.config/opilio/config.yaml`
- macOS: `~/.config/opilio/config.yaml`
- Windows: `%APPDATA%\\opilio\\config.yaml`

Lookup order:

1. `--config <path>`
2. `OPILIO_CONFIG`
3. platform default

The primary YAML describes the flock and is portable/Git-friendly. Controller-local state (SSH keys, scheduler records, temporary control sockets, TUI state) stays outside it.

Illustrative schema:

```yaml
sites:
  home:
    label: Home
  lab:
    label: Lab via VPN

devices:
  spark-home:
    site: home
    ssh: spark-home
    groups: [sparks]
    power:
      type: shelly
      host: shelly-spark-home
      auth:
        password: "${env:OPILIO_SHELLY_HOME_PASSWORD}"
    shutdown:
      command: sudo shutdown -h now
      cut_power: true
      timeout: 2m
    telemetry:
      provider: nvidia
    services:
      - llm

  spark-2:
    site: lab
    ssh: spark-2
    groups: [sparks, sglang]
    power:
      type: wol
      mac: "00:11:22:33:44:55"

groups:
  sparks:
    devices: [spark-home, spark-2]
  sglang:
    devices: [spark-2]

actions:
  update:
    command: sudo apt update && sudo apt upgrade -y
    timeout: 30m

  start-llm:
    cwd: ~/llm/vllm
    exec:
      program: ./start.sh
      args: []
    timeout: 2m
    overrides:
      sglang:
        command: systemctl start sglang
      spark-home:
        command: docker start vllm

aliases:
  shutdown-sparks:
    operation: off
    target: sparks
    parallel: 4

services:
  llm:
    status:
      command: docker inspect -f '{{.State.Status}}' vllm
    health:
      url: http://localhost:8000/health
    info:
      url: http://localhost:8000/v1/models
```

The exact schema may evolve during implementation, but changes must preserve these semantics and remain strict/validated.

## 6. CLI hierarchy

Representative interface:

```text
opilio                         # open TUI
opilio status [target]
opilio on <target> [--wait]
opilio off <target> [--yes] [--force]
opilio shutdown <target>
opilio reboot <target>
opilio power-off <target> [--force]
opilio power-cycle <target> [--force]
opilio ssh <device>
opilio action ls
opilio action run <name> <target>
opilio device ls
opilio group ls
opilio site ls
opilio config path
opilio config edit
opilio config check
opilio doctor [target]
opilio schedule ls
opilio schedule add ...
opilio schedule rm <id>
opilio history [target]
opilio history show <id>
opilio export <bundle>
opilio import <bundle> [--non-interactive]
```

`ssh` accepts exactly one device, never a group/site.

`status` with no target means all configured devices.

## 7. Structured output and exit codes

All operational/read commands should support stable `--json` output. Also support `--quiet` where meaningful. JSON is a public interface and requires compatibility tests.

Exit codes:

- `0`: success
- `1`: operation failed
- `2`: configuration/usage error
- `3`: partial success

Group/site operations return per-device results. A failure on one device does not prevent remaining devices from being processed unless operation semantics explicitly require otherwise.

## 8. Concurrency

- Read-only status/telemetry may be concurrent by default.
- Disruptive operations such as update/off/reboot are sequential by default (`parallel=1`).
- User may override with `--parallel N` or alias configuration.
- Results are consolidated per device.

## 9. Safety semantics

`--yes` only suppresses interactive confirmation. It does not change shutdown behavior.

`--force` permits bypassing graceful OS shutdown and physically cutting/cycling power when supported.

Normal `off` follows configured graceful strategy: SSH shutdown -> wait for shutdown condition/timeout -> cut Shelly power if configured.

Single-device normal operations do not prompt. Group/site disruptive operations show resolved devices and require confirmation unless `--yes`. Scheduled invocations are inherently non-interactive and behave as confirmed.

Forced physical power operations require explicit `--force` semantics and appropriate TUI confirmation.

## 10. SSH architecture

Use system OpenSSH rather than an embedded SSH implementation. Reuse normal SSH aliases/configuration, keys, ProxyJump, agents, host-key checking, etc.

One-shot CLI operations invoke ordinary SSH.

While the TUI is open, use temporary OpenSSH connection multiplexing (`ControlMaster`) per reachable device to avoid reconnecting for frequent telemetry/service polls. Close temporary control connections on clean TUI exit. Nothing is installed remotely.

Remote shell `command` actions should invoke a defined shell (default Linux `/bin/sh -lc`) rather than relying implicitly on login-shell behavior. Structured `exec` avoids shell parsing where possible.

## 11. Power providers

### Shelly
Use the local Shelly API directly from Rust. Required v1 capabilities where supported by the device:

- outlet on/off
- outlet state
- voltage/current/power watts
- energy where useful

Shelly must be directly reachable from the controller's current network/VPN; Opilio does not tunnel Shelly traffic through SSH in v1.

### Wake-on-LAN
Send WoL magic packets directly. WoL-only devices may be shut down via SSH but cannot have physical power cut by Opilio unless another provider is configured.

## 12. `on` semantics

`opilio on` is non-blocking by default: request the underlying power transition and return.

`opilio on <device> --wait` waits for SSH availability only. It does not imply service/model readiness. Keep states separate: powered -> booting -> SSH-ready -> service state -> application/model readiness.

## 13. Telemetry

Telemetry is optional and provider-based.

### System provider
Collect ordinary Linux metrics over SSH, such as CPU/load, system RAM, and uptime.

### NVIDIA provider
Collect NVIDIA GPU metrics and system metrics. Auto-detect DGX Spark/GB10 unified-memory behavior; do not present conventional VRAM values as accurate when the platform uses unified memory and the relevant conventional metric is unsupported/misleading.

### Shelly telemetry
Implicit from the configured Shelly power provider: outlet state and electrical measurements.

TUI polling defaults:

- system/GPU: about 2 s
- Shelly power: about 2 s
- service health/info: about 5-10 s
- unreachable device probe: about 10 s
- apparently unreachable site: progressive backoff, e.g. 10 -> 20 -> 30 s

Manual refresh immediately retries. History for graphs is in memory only.

## 14. Service status

Services are generic configured observations. They may use remote commands and/or HTTP health/info endpoints. Opilio may extract useful fields such as model name from configured JSON endpoints, but must not become a vLLM/SGLang-specific management client.

Desired LLM presentation includes states such as stopped/loading/ready/error and model name when available.

## 15. TUI

Use a Rust TUI library such as Ratatui. One dashboard, master/detail, keyboard-first, with arrows plus Vim-style navigation and `/` filtering/search.

Conceptual layout:

```text
 Opilio ---------------------------------------------------------
 Sites / Groups          Devices                 Details
 v Home                  > spark-home            running
   all                     spark-dev              SSH     yes
 > Lab                                            RAM     91/128 GB
 Groups                                           GPU     87%
 > sparks                                         Power   118 W
 > development                                    LLM     Qwen...
                                                  RAM  sparkline
                                                  GPU  sparkline
                                                  Watt sparkline
 [o] On [x] Off [r] Reboot [a] Action [s] SSH [?] Help
```

TUI v1 operations: status, on/off/reboot, named actions, SSH, confirmations, live telemetry/service status, in-memory graphs, recent failure indication. No config editor/full log viewer/embedded terminal.

## 16. Diagnostics

`opilio config check` performs static validation only: YAML/schema, references, ambiguity, mutually exclusive fields, etc.

`opilio doctor [target]` performs runtime diagnostics: local `ssh` availability, reachability, SSH, power provider, NVIDIA/UMA capability, Docker/systemd presence where informative, service checks, etc. It may suggest configuration snippets but never modifies configuration automatically.

No general LAN discovery in v1; doctor introspects configured resources only.

## 17. Scheduling

Delegate scheduling to native OS mechanisms behind one abstraction:

- Linux: systemd user timers where available, with an appropriate fallback such as cron
- macOS: launchd
- Windows: Task Scheduler

Opilio manages only jobs it created. `schedule ls/add/rm` use Docker-like noun/subcommand syntax.

No Opilio daemon. No Opilio catch-up behavior for missed runs; native scheduler semantics apply. Scheduled jobs invoke Opilio commands/aliases and are recorded in history.

## 18. Import/export and portability

`opilio export` creates a safe, non-secret setup bundle containing the portable flock configuration, only the OpenSSH Host entries referenced by Opilio devices (and necessary referenced jump-host entries), and metadata describing required local setup/secrets.

Do not export private SSH keys or environment-secret values by default.

`opilio import` is an interactive migration/setup process. It may ask once for changed remote SSH username/key mappings and apply the answer to applicable devices. It should install/import Opilio-owned SSH entries separately from unrelated user SSH config. Non-interactive import imports what is safe/resolvable and reports unresolved requirements.

After import, run static validation and targeted diagnostics.

## 19. Secrets

v1 secret mechanism: environment-variable references, e.g. `${env:OPILIO_LLM_API_KEY}`. Never include resolved secret values in JSON output, logs, export bundles, diagnostics, or errors.

Native OS credential stores are a possible future optional feature, not a v1 dependency.

## 20. Logging/history

Maintain a rotating local JSONL operation log. Store metadata for every operation: timestamp, source (CLI/TUI/scheduled), operation/action, target/resolved device, duration, result, exit code, force flag where relevant.

For successful remote commands, do not persist full stdout/stderr. For failed operations, retain only a bounded tail (e.g. configurable ~64 KiB) of stdout/stderr for diagnostics. Never intentionally log secrets.

Expose `history`, `history <target>`, `history show <id>`, and JSON output.

## 21. Cross-platform/runtime footprint

Ship as a single Rust executable for macOS, Linux, and Windows. No Opilio daemon, database, container runtime, or language runtime required. System OpenSSH is the intentional external dependency for SSH functionality.

Implement Shelly HTTP, WoL, config parsing, logging, and TUI inside the binary. Remote actions may of course depend on commands installed on remote systems.

## 22. Recommended Rust architecture

Prefer a library-first design so CLI, TUI, and scheduled invocations use the same application/domain APIs.

Suggested modules/crates (start as modules unless separation becomes valuable):

```text
src/
  main.rs
  lib.rs
  app/              # use cases / orchestration
  config/           # YAML model, loading, validation, resolution
  domain/           # Device, Group, Site, Action, Service, state/results
  target/           # target resolution
  ssh/              # system OpenSSH + multiplexing
  power/
    mod.rs
    shelly.rs
    wol.rs
  telemetry/
    mod.rs
    system.rs
    nvidia.rs
  service/          # generic status/health/info probes
  scheduler/
    mod.rs
    linux.rs
    macos.rs
    windows.rs
  history/          # JSONL rotation/query
  transfer/         # export/import
  cli/
  tui/
```

Recommended ecosystem candidates: clap, serde/serde_yaml, tokio, reqwest, ratatui/crossterm, tracing, thiserror/anyhow at appropriate boundaries, directories, and platform-specific scheduler adapters. Choose exact dependencies during implementation based on maintained/current crates.

## 23. Testing strategy

Test through stable seams, not implementation details:

- config parsing/strict validation/reference resolution
- target resolution and overlapping-group action ambiguity
- action override precedence
- command vs exec mutual exclusion
- safety planning (`--yes` vs `--force`)
- structured JSON schemas and exit codes
- SSH command construction/multiplex lifecycle via fake process adapter
- Shelly provider via mock HTTP server
- WoL packet construction
- telemetry parsing including DGX Spark/UMA fixtures
- service HTTP/command parsing
- scheduler adapter command/artifact generation per OS without mutating host scheduler in unit tests
- export redaction/selective SSH extraction/import mapping
- history redaction/rotation/bounded failure output
- TUI state/update logic separately from terminal rendering

Integration tests should use fake adapters wherever practical. Hardware-dependent tests are opt-in/manual.

## 24. Acceptance criteria for v1

A release candidate is acceptable when:

- the same portable YAML can be used on macOS, Linux, and Windows controllers;
- `status`, `on`, graceful `off`, reboot, named action, and SSH work for configured devices;
- Shelly and WoL providers have tested implementations;
- group/site targeting and controlled concurrency work;
- forced power semantics cannot be triggered accidentally by `--yes` alone;
- TUI can monitor and operate a small flock without blocking on long boot/model-load times;
- DGX Spark telemetry does not misrepresent UMA as conventional VRAM;
- JSON output is documented/tested and partial success is distinguishable;
- native scheduler adapters can add/list/remove Opilio-owned jobs;
- import/export never exports private keys or resolved environment secrets by default;
- history records operations and bounded failed output;
- `config check` and `doctor` clearly distinguish static configuration problems from reachability/runtime problems.
