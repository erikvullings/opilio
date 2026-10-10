# 0029 — TUI recovery cycle and outlet state

**Status:** done
**Depends on:** 0008, 0011, 0012, 0028
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Make the TUI's Reboot key recover a Shelly-powered, unresponsive device and show the physical outlet state independently of SSH and power draw.

## Context

An SSH reboot cannot recover a device whose SSH path is stuck. TUI polling currently shows watts, but not the Shelly's on/off switch state. The user requested a confirmed graceful-shutdown-first physical cycle with a forced cut on failure, an observable pause, and a record of each stage.

## Decisions

- TUI Reboot requires confirmation even for one device and warns about the forced-cut fallback. CLI `reboot` stays an SSH reboot.
- The TUI first attempts graceful SSH shutdown and waits for SSH to go away; on failure it still cuts Shelly power. It holds power off for ten seconds to respect the Shelly request cadence, then requests power on.
- An unsupported provider must fail before attempting SSH shutdown; a failed physical cut must not proceed to power on.
- A Shelly HTTP 429 during power restore gets one delayed retry; an unsuccessful restore explicitly warns that the outlet may remain off.
- Suspend normal device polling during a recovery cycle and defer post-cycle Shelly observation for ten seconds to avoid colliding with switch requests. Treat the last observed outlet value as unknown during the transition.
- Reuse the TUI's existing Shelly provider for switching, preserving its generation and digest-auth cache instead of issuing extra discovery/authentication requests.
- Show the Shelly outlet as on, off, or unknown separately from SSH state and watts.
- Persist each stage's result in operation history without secret values.

## Acceptance criteria

- Confirmation and cancellation work for a single device and collections.
- Fake lifecycle tests cover graceful success, SSH failure fallback, cut failure, and ten-second wait.
- TUI displays progress throughout the wait, final outcome and explicit outlet state.
- Stage history includes failures and force flags; history failure is visible.
- Existing CLI lifecycle semantics and JSON output remain unchanged.

## Progress

- [x] Wrote failing focused tests for the recovery sequence and confirmation.
- [x] Implemented lifecycle, history, power-state, and TUI integration.
- [x] Validated, documented, and installed the updated release binary.

## Validation

- `cargo test --quiet --all-targets --all-features` — passed.
- `cargo clippy --quiet --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt --check` — passed.
- `git diff --check` — passed.
- `cargo build --release --quiet` — passed; installed and byte-compared `~/.local/bin/opilio`.
- The Impeccable detector returned no TUI findings.
- No live power operations were executed; tests use fake lifecycle steps and local history files.

## Blockers / decisions needed

None; the user selected graceful shutdown followed by a confirmed physical-cycle fallback.

## Notes / handoff

The existing TUI process must be restarted to load the new executable. Its
Shelly password environment variable must be present at startup. A recovery
success means power was restored, not that SSH or services have finished
booting. The CLI `reboot` and JSON output retain their existing semantics.
