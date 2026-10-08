# 0026 — Publish Windows-ready release

**Status:** in-progress
**Depends on:** 0025
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Publish the Windows build fix in a GitHub release with binaries for all
supported targets.

## Context

The Release workflow already builds four target archives on `v*` tags and
publishes them as GitHub release assets. Confirm the Windows CI build before
tagging the fix.

## Decisions

- Do not overwrite an existing release; use the package version if its tag is
  available.

## Progress

- [x] Verify CI for the Windows fix.
- [ ] Publish and verify the GitHub release and its four archives.

## Validation

- [CI run 37811993774](https://github.com/erikvullings/opilio/actions/runs/37811993774)
  passed release builds and full checks on Linux, macOS, and Windows.

## Blockers / decisions needed

None.

## Notes / handoff

The release must contain the `Mutex` fix.
