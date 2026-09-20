# 0007 — Wake-on-LAN provider

**Status:** done
**Depends on:** 0003  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Add Wake-on-LAN as a simple power-on provider for non-Shelly machines.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Implement MAC parsing/validation and WoL magic-packet construction.
- Send UDP broadcast using configurable/default network behavior appropriate to the platform.
- Integrate provider capability reporting: WoL can request on but cannot cut physical power.
- Keep provider interface compatible with devices that use Shelly or WoL.
- Unit-test packet bytes and validation; keep network send behind a test seam.

## Acceptance criteria

- Known MAC produces correct magic packet.
- Invalid MAC/config fails statically.
- Capability model prevents treating WoL as physical power-off support.

## Implementation notes

- Prefer a vertical, demonstrable slice through shared library APIs rather than code that only a later task can exercise.
- Keep platform/hardware/process/network dependencies behind test seams.
- Treat JSON output and secret redaction as compatibility/security surfaces where applicable.
- Add dependencies only when they are maintained and materially reduce complexity.

## Decisions

- MAC addresses accept conventional colon or hyphen notation with exactly six
  two-digit hexadecimal unicast octets and serialize canonically with colons.
- WoL sends IPv4 UDP through an injectable sender, defaulting to the limited
  broadcast destination `255.255.255.255:9`; `broadcast` configures a directed
  broadcast address and port.
- The shared provider capability model distinguishes requesting power-on from
  cutting physical power. A successful WoL send leaves outlet state unknown and
  never claims that the machine booted or that power was physically switched.

## Progress

- [x] Task picked up; status changed to `in-progress` in this file and root README.
- [x] Implementation completed.
- [x] Focused tests pass.
- [x] Full relevant test suite/lints pass.
- [x] Documentation/examples updated if behavior is user-visible.
- [x] Status changed to `done` and root README dashboard updated.

## Validation

- `cargo test --test power_wol --test power_shelly --quiet` — 9 WoL tests and
  10 shared Shelly-provider tests passed.
- `cargo check --all-targets --all-features` — passed.
- `cargo test --all-targets --all-features` — full suite passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt --check` — passed.

## Blockers / decisions needed

None currently.

## Notes / handoff

- 2026-09-20 Copilot: Added strict typed MAC parsing, exact 102-byte magic
  packets, configurable/default IPv4 broadcast destinations, a fakeable UDP
  sender plus portable system implementation, production/configured provider
  construction, explicit send errors, and shared power-on/physical-cut
  capabilities. Added 9 public-interface tests including fake-send and local
  UDP coverage; check, full suite, strict clippy, and formatting pass. Lifecycle
  commands remain intentionally scoped to task 0008.
