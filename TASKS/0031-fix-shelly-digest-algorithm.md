# 0031 — Fix Shelly digest algorithm

**Status:** done
**Depends on:** 0022
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Stop persistent Shelly HTTP 429 errors in the TUI and give operators a way to
detect intermittent Shelly failures without writing a custom poller.

## Context

Task 0022 treated HTTP 429 as a request-volume problem. The real cause was
authentication: `diqwest` rebuilds cached digest contexts without the
challenge's `algorithm`, so every reused request was signed with MD5. Shelly
Gen2+ plugs require SHA-256, rejected the request, and locked the controller
out with HTTP 429. A fresh handshake with curl succeeded every time, which
isolated the client. The TUI's 429 suppression hid the symptom.

## Decisions

- Perform a fresh digest handshake per request instead of reusing a cached
  `DigestAuthSession`.
- The regression test server issues a SHA-256 challenge and answers any other
  digest with 429, like a real plug.
- Remove the TUI's 429 masking; keep the 10-second power poll cadence.
- Add `opilio doctor --power-samples N` (10 s apart); the default of 1 keeps
  existing output and JSON shape.

## Acceptance criteria

- Polling three live plugs every 10 s produces no 429 after the existing
  lockout expires.
- Doctor reports intermittent Shelly failures as `k of N samples failed`.
- Formatting, strict Clippy, and all tests pass.

## Progress

- [x] Reproduced 429 with Opilio's client; curl digest succeeded.
- [x] Fixed digest handling and strengthened the regression test.
- [x] Removed TUI 429 masking.
- [x] Added doctor power sampling with unit tests.

## Validation

- Live poll of three plugs every 10 s for 75 s: one residual lockout 429, then
  all samples succeeded.
- `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo test --all-targets --all-features` — passed.

## Blockers / decisions needed

None.

## Notes / handoff

Two Sparks were found powered off. Their plugs crashed during a weekly network
vulnerability scan and powered up with the relay off. That is plug firmware and
configuration, not Opilio; see Troubleshooting in `docs/USER_GUIDE.md`.
