# 0005 — Named actions and aliases

**Status:** open  
**Depends on:** 0003, 0004  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Run deterministic configured remote actions and one-operation aliases across targets.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Implement `action ls` and `action run <name> <target>`.
- Execute either `command` or structured `exec`; no runtime parameters/templates.
- Honor cwd, timeout, default/group/device override resolution.
- Use conservative default parallelism for disruptive actions; support `--parallel N`.
- Implement aliases as exactly one existing Opilio operation plus target/options.
- Do not add workflows, loops, conditions, arbitrary group exec, or DAG semantics.
- Provide per-device human/JSON results and partial-success exit code.

## Acceptance criteria

- Default and override action implementations are exercised in tests.
- Conflicting group override never resolves by order.
- Alias expansion is deterministic and cannot recurse into a workflow language.
- JSON results remain stable.

## Implementation notes

- Prefer a vertical, demonstrable slice through shared library APIs rather than code that only a later task can exercise.
- Keep platform/hardware/process/network dependencies behind test seams.
- Treat JSON output and secret redaction as compatibility/security surfaces where applicable.
- Add dependencies only when they are maintained and materially reduce complexity.

## Progress

- [ ] Task picked up; status changed to `in-progress` in this file and root README.
- [ ] Implementation completed.
- [ ] Focused tests pass.
- [ ] Full relevant test suite/lints pass.
- [ ] Documentation/examples updated if behavior is user-visible.
- [ ] Status changed to `done` and root README dashboard updated.

## Validation

Record commands/tests and concise results here before marking done.

## Blockers / decisions needed

None currently.

## Notes / handoff

Add anything a fresh agent needs to resume this task.
