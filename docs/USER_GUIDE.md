# Opilio v1 user guide

Opilio is one local executable. It installs no daemon and no remote agent.
Remote access intentionally uses the controller's system OpenSSH client.

## Install

Download the archive for the controller from the GitHub release:

| Controller | Archive |
|---|---|
| Linux x86-64 | `opilio-x86_64-unknown-linux-gnu.tar.gz` |
| macOS Intel | `opilio-x86_64-apple-darwin.tar.gz` |
| macOS Apple silicon | `opilio-aarch64-apple-darwin.tar.gz` |
| Windows x86-64 | `opilio-x86_64-pc-windows-msvc.zip` |

Verify the adjacent `.sha256` file before extracting:

```sh
# Linux
sha256sum --check opilio-*.sha256
# macOS
shasum -a 256 --check opilio-*.sha256
```

On Windows, compare `Get-FileHash -Algorithm SHA256 <archive>` with the value in
`<archive>.sha256`. Put `opilio`/`opilio.exe` on `PATH`. Also install the
OpenSSH client and ensure `ssh -V` works. Package-manager channels (for example
Homebrew, WinGet, and Linux repositories) are follow-up distribution work; the
release archives are the supported v1 installation path.

## Configure a controller

Copy [`examples/config.yaml`](../examples/config.yaml) to one of:

- Linux: `$XDG_CONFIG_HOME/opilio/config.yaml`, or
  `~/.config/opilio/config.yaml` when `XDG_CONFIG_HOME` is unset.
- macOS: `~/.config/opilio/config.yaml`.
- Windows: `%APPDATA%\opilio\config.yaml`.

`--config <path>` takes precedence over `OPILIO_CONFIG`, which takes precedence
over the platform default. The same YAML is portable across all controllers;
keep controller-local SSH keys, scheduler state, and secrets outside it.

Configure each `ssh` value as an exact host or alias understood by system
OpenSSH. Opilio preserves normal keys, agents, host-key policy, and
`ProxyJump`. Remote command actions default to `/bin/sh -lc`; set a device's
`shell` when needed.

Secrets must be environment references:

```yaml
password: "${env:OPILIO_SHELLY_HOME_PASSWORD}"
headers:
  Authorization: "${env:OPILIO_LLM_API_TOKEN}"
```

Set those variables in the environment that starts Opilio or the native
scheduler. Never put resolved values in YAML. JSON, diagnostics, history, and
exports redact configured resolved values.

Validate before contacting hardware:

```sh
opilio config path
opilio config check
```

`config check` is static and network-free. `doctor` is the separate runtime
check:

```sh
opilio doctor all
opilio doctor lab --json
opilio status all
```

On a fresh controller, `opilio import setup.tar --non-interactive` can install
the portable flock and isolated SSH entries before these checks. Exit `3`
means local environment variables or key mappings still need attention.

## Power and services

For Shelly, set `power.type: shelly`, a controller-reachable host, and optional
Basic/Digest credentials. Gen1 and RPC devices are detected automatically.
Opilio reports outlet state and supported voltage/current/watts/energy.

For Wake-on-LAN, set `power.type: wol`, a unicast MAC address, and optionally a
directed IPv4 `broadcast` socket. WoL can request startup but cannot observe or
cut power. VPN-backed sites are organizational/reachability contexts only;
connect the VPN outside Opilio.

System and NVIDIA telemetry run over SSH. NVIDIA collection detects DGX
Spark/GB10 unified memory and does not label it as conventional VRAM. Generic
services can combine an SSH status command with controller-side HTTP health
and info probes; `info.extract` uses RFC 6901 JSON pointers.

## Safe operation

```sh
opilio on alpha                 # request power, then return
opilio on alpha --wait          # wait only for SSH readiness
opilio off alpha                # graceful shutdown; optionally cut Shelly
opilio reboot alpha
opilio action run update alpha
opilio ssh alpha
```

Collections show their resolved devices and require confirmation unless
`--yes` is supplied. `--yes` never grants force. `power-off` and `power-cycle`,
and graceful-shutdown bypass via `off --force`, require explicit `--force`.
Operations continue across independent device failures. Default disruptive
parallelism is one; opt in with `--parallel N`.

Automation exit codes are:

| Code | Meaning |
|---:|---|
| 0 | success |
| 1 | operation failed |
| 2 | configuration or usage error |
| 3 | partial success |

Versioned JSON includes `schema_version` and retains every per-device result.
Use `--quiet` where offered when only the exit code is needed.

## TUI

Run `opilio` with no command in an interactive terminal.

| Key | Action |
|---|---|
| arrows or `h/j/k/l` | navigate |
| `/` | filter |
| `R` | refresh immediately |
| `o`, `x`, `r` | on, off, reboot |
| `a` | choose named action |
| `s` | suspend the TUI and hand off to system SSH |
| `?` | help |
| `q` | quit |

Polling and operations run outside the render loop. Telemetry graphs remain in
memory. Opilio closes temporary OpenSSH control sessions and restores the
terminal on normal exit; Windows uses ordinary OpenSSH sessions because its
client does not provide Unix control sockets.

## Scheduling and migration

```sh
opilio schedule add nightly --at 23:30 -- status all --json
opilio schedule ls --json
opilio schedule rm nightly
```

Linux uses a systemd user timer when available, otherwise cron; macOS uses
launchd; Windows uses Task Scheduler. Opilio lists/removes only marked jobs it
created. Jobs are daily local-time native schedules, use an absolute executable
and config path, and rely on native missed-run behavior. No Opilio process
stays running.

```sh
opilio export setup.tar
opilio import setup.tar
opilio import setup.tar --non-interactive --json
```

Exports contain normalized flock YAML, only referenced exact SSH hosts and
their jump-host chain, and names/labels for required local setup. They never
read or include private keys or resolved environment secrets. Import validates
the whole bundle before writing and keeps managed SSH entries under
`~/.ssh/opilio/config`.

## Troubleshooting

- **Exit 2 / config error:** run `opilio config check`; unknown fields,
  references, duplicate membership, ambiguous action overrides, and malformed
  secret references are rejected.
- **OpenSSH unavailable:** install the system client, put `ssh` on `PATH`, and
  test the configured alias with `ssh <alias>`.
- **A whole site is unreachable:** establish its VPN/network route, then run
  `opilio doctor <site>` or press `R`; Opilio does not manage VPNs.
- **Shelly unavailable:** verify controller routing, device generation/auth,
  and the referenced environment variable.
- **WoL does not wake:** verify firmware WoL support and use the subnet's
  directed broadcast if the default broadcast is filtered.
- **Scheduled job fails:** use an absolute config, ensure scheduler-visible
  environment variables/SSH agent or keys, then inspect `opilio schedule ls`
  and `opilio history`.
- **TUI refuses to start:** use an interactive terminal; scripts should use CLI
  commands and `--json`/`--quiet`.

