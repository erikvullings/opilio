# 0010 — Generic service health

**Status:** open  
**Depends on:** 0004  
**Spec:** `docs/OPILIO_V1_SPEC.md`

## Goal

Add configurable generic service status/health/info probes without hard-coding vLLM/SGLang.

## Context

This is a resumable Opilio v1 task. Read the product spec and `AGENTS.md` before implementation. Do not reopen settled product decisions unless implementation exposes a contradiction. Keep this file current while working.

## Requirements

- Implement generic service definitions using configured SSH command and/or HTTP endpoints.
- Support status, health, and info probes with timeouts.
- Allow configured extraction of useful JSON fields such as model name while keeping the domain generic.
- Represent stopped/loading/ready/error/unknown states where evidence supports them.
- Poll service health less frequently than system telemetry by default.
- Do not add Docker/systemd/vLLM/SGLang management APIs; named actions remain the control mechanism.

## Acceptance criteria

- Mock HTTP/SSH fixtures demonstrate ready, loading, stopped, error, malformed, timeout states.
- A vLLM-compatible `/v1/models` response can yield a model name through generic configuration.
- No provider-specific service code is required for the example.

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
