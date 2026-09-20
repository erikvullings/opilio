# 0001 — Bootstrap Rust application

**Status:** done  
**Depends on:** none  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Create the cross-platform Rust project skeleton and shared library-first architecture.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Initialize a Rust application/library named `opilio` with one executable.
- Establish `src/lib.rs` as the shared application surface used by CLI/TUI/scheduled invocations.
- Add initial CLI parsing with `opilio --help`, `opilio --version`, and no-argument TUI placeholder behavior.
- Establish module boundaries from the v1 spec without prematurely splitting into a workspace.
- Add formatting, clippy, unit-test, and cross-platform CI foundations.
- Do not implement product features yet; this task proves the executable/library/CI seam.

## Acceptance criteria

- `cargo build`, `cargo test`, `cargo fmt --check`, and strict clippy pass.
- `opilio --help` works and identifies the application.
- No-argument invocation reaches a clearly isolated TUI entry point (placeholder is acceptable).
- CI matrix covers Linux, macOS, and Windows.

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

- `cargo build --all-targets --all-features` — passed.
- `cargo test --all-targets --all-features` — 2 passed.
- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `cargo run --quiet -- --help`, `--version`, and no arguments — expected output.

## Blockers / decisions needed

None currently.

## Notes / handoff

- Added the library-first module skeleton under `src/`, with `src/main.rs` limited to CLI parsing, application dispatch, and exit-code handling.
- The no-argument path calls the isolated `tui::run` placeholder. Future commands belong in `cli`, with behavior dispatched through `app`.
- Added `.github/workflows/ci.yml` for Linux, macOS, and Windows.
