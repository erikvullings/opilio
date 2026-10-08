# 0026 — Publish Windows-ready release

**Status:** done
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
- [x] Publish and verify the GitHub release and its four archives.
- [x] Correct the publish job's missing checkout and normalize Windows
  checksum line endings for future tagged releases.

## Validation

- [CI run 37811993774](https://github.com/erikvullings/opilio/actions/runs/37811993774)
  passed release builds and full checks on Linux, macOS, and Windows.
- Final [CI run 37814635751](https://github.com/erikvullings/opilio/actions/runs/37814635751)
  passed on the merged branch commit.
- The [v1.0.0 release](https://github.com/erikvullings/opilio/releases/tag/v1.0.0)
  contains four archives and four checksums; all four SHA-256 digests match.

## Blockers / decisions needed

None.

## Notes / handoff

The release tag points to the verified main merge containing the `Mutex` and
TUI contrast fixes. The tagged workflow built all four archives but its
publish job lacked a checkout, so publication was completed using the
checksum-verified workflow artifacts and the personal GitHub keychain login.
The release workflow now includes the checkout for subsequent tags.
