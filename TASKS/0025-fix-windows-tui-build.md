# 0025 — Fix Windows TUI build

**Status:** done
**Depends on:** 0022
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Restore Windows compilation after the Shelly power-observation cache change.

## Context

A Windows source build fails with E0425/E0433 at `src/tui/runtime.rs` because
`Mutex` is imported only under `#[cfg(not(windows))]`, although
`CachedPowerProvider` uses it on every platform.

## Decisions

- Import `Mutex` alongside the unconditional `Arc` import. No change to power
  behavior is needed.
- Extend the existing Linux/macOS/Windows CI matrix to build an optimized
  release binary on every platform; its Windows job is the platform regression
  check. A local macOS cross-check requires an unavailable Windows C toolchain
  for `aws-lc-sys`.

## Progress

- [x] Identified the platform-guard mismatch from the Windows compiler output.
- [x] Correct the import and validate locally.
- [x] Address Windows-only dead-code and newer CI Clippy diagnostics.
- [x] Correct the transfer CLI test fixture to override `USERPROFILE` on
  Windows instead of `HOME`, matching production path resolution.
- [x] Verify the Windows CI release build.

## Validation

- `cargo check --all-targets --all-features --locked --quiet` — passed.
- `cargo test --test tui --locked --quiet` — 15 tests passed.
- `cargo fmt --check` and strict `cargo clippy` — passed.
- `cargo check --target x86_64-pc-windows-msvc --locked --quiet` cannot complete
  on macOS: `aws-lc-sys` needs the Windows C toolchain.
- [CI run 37811993774](https://github.com/erikvullings/opilio/actions/runs/37811993774)
  passed on Linux, macOS, and Windows, including strict Clippy, full tests,
  and optimized release builds.

## Blockers / decisions needed

None.

## Notes / handoff

No configuration or runtime behavior changes are intended. The existing Release
workflow already packages Linux, macOS Intel/Apple silicon, and Windows. The
Windows transfer test fixture now overrides `USERPROFILE` to match the
production SSH path lookup.
