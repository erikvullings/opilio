# AGENTS.md

## Project

Opilio is a cross-platform Rust CLI/TUI for agentless management of a small flock of machines. Read `docs/OPILIO_V1_SPEC.md` before substantial work.

## Task tracking

One Markdown file per task lives in `TASKS/`. The files are the resumable source of truth for humans and coding agents; git is the history.

Before substantial work:

1. Read `README.md` for the current task dashboard.
2. Search `TASKS/` for the matching task.
3. Pick the lowest-numbered `open` task whose dependencies are all `done`, unless the user explicitly names another task.
4. Mark it `in-progress` before changing code.
5. Keep its Context, Decisions, Progress, Validation, and Notes current as work proceeds.
6. Run the task's validation before marking it `done`.
7. Update the task-status table in `README.md` whenever a task status changes.

Do not silently redesign settled v1 requirements. If implementation reveals a real contradiction or missing product decision, record it in the active task under `## Blockers / decisions needed` and ask the user rather than inventing a new requirement.

## New tasks

If new work is discovered and does not fit the active task, create `NNNN-short-kebab-title.md`, where `NNNN` is one greater than the highest existing task number. Keep the title under 60 characters. Declare dependencies explicitly.

## Status vocabulary

Use exactly: `open`, `in-progress`, `blocked`, `done`.

## Engineering approach

Prefer vertical/tracer-bullet tasks that leave a demonstrable capability rather than layer-only work. Keep CLI, TUI, scheduler, and tests behind shared library APIs. Test stable seams. Never log or export resolved secrets. Treat JSON output as a public compatibility surface.
