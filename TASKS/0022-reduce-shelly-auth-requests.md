# 0022 — Reduce Shelly auth requests

**Status:** done
**Depends on:** 0006, 0012
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Prevent authenticated Shelly Gen3 plugs from rate-limiting TUI status polling.

## Context

Both XRLab Shelly plugs return HTTP 429 while the TUI is running. A direct request loop reproduces alternating 401 and 429 responses. `ReqwestHttpClient` first sends an unauthenticated request to inspect the challenge and then calls `diqwest::send_digest_auth`, which performs its own unauthenticated challenge request. Each provider sample reads both `/shelly` and `Switch.GetStatus`, producing up to six requests every two seconds. The initial fix reduced that to three, but review revealed that the TUI recreated the provider every poll and discarded both the cached API generation and digest session.

## Requirements

- Let `diqwest` own the digest challenge flow.
- Cache digest challenge state within a Shelly HTTP client so subsequent requests use preemptive authentication.
- Retain one Shelly provider per TUI device so generation and authentication caches survive polling.
- Poll Shelly power at a slower bounded cadence than system telemetry.
- Preserve the last good power observation across a transient HTTP 429.
- Preserve Basic authentication fallback.
- Never expose credentials in debug output or errors.

## Acceptance criteria

- A local digest-auth regression server observes two requests for the first endpoint and one preemptively authenticated request for the second endpoint.
- Existing Shelly provider tests remain green.
- Full tests, formatting, and strict Clippy pass.
- After restarting the TUI, live Shelly status no longer reports HTTP 429 under normal polling.

## Progress

- [x] Reproduced HTTP 429 against both live XRLab plugs.
- [x] Identified duplicate digest challenge requests in `ReqwestHttpClient`.
- [x] Regression test captures the corrected request count.
- [x] Implementation completed.
- [x] Provider reuse implemented and validated.
- [x] Power cadence and transient 429 handling implemented.
- [x] Final validation completed.

## Validation

- Direct burst probes reproduced HTTP 429 on `134.221.73.51` and `134.221.74.117`.
- `digest_challenge_is_reused_for_subsequent_requests` observes three total requests for two authenticated endpoints.
- `cargo test --quiet --all-targets --all-features` — passed.
- `cargo clippy --quiet --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt --check` — passed.
- Two live `doctor xrlab` runs reported both Shelly providers reachable.
- Six live one-shot power checks at two-second intervals completed with zero failures, but did not exercise persistent TUI provider reuse.
- The persistent TUI retains one provider per device, preserving generation and digest caches across polls.
- A rebuilt persistent TUI still showed intermittent 429 responses, proving one authenticated request every one to two seconds remains too frequent for these Shelly Gen3 units.
- Power observations now have an independent 10-second cache, reducing steady-state traffic to one authenticated request per device every 10 seconds.
- A transient startup 429 preserves the last good observation or remains silent while retrying; a persistent limiter problem is reported after three consecutive checks.
- The final rebuilt TUI was observed for at least 45 seconds with no Shelly warning at startup or during steady state.

## Blockers / decisions needed

None.

## Notes / handoff

The old TUI process was stopped before live verification. Restart the TUI with the rebuilt binary.

`ReqwestHttpClient` lets `diqwest` own digest authentication and caches the digest challenge for subsequent requests. Debug formatting remains redacted. The TUI retains one provider per device and caches power observations for 10 seconds.
