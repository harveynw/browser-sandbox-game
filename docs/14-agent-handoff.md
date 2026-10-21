# 14 — Agent handoff

[Index](../README.md) · Tasks: [Implementation plan](13-implementation-plan.md)

## How to use this package

Give the implementation agent one task at a time. Start at T01, then use the continuation prompt. The design already selects the stack, scope, protocol, and localhost deployment; the agent should not spend each turn redesigning them.

The authoritative constraints are:

1. Rust authoritative server using the pinned `harveynw/net-crate`; no substitute networking library.
2. TypeScript/Three.js browser client and shared Rust/WASM movement.
3. Rust-owned DTOs generated to TS and JSON Schema, MessagePack named maps.
4. Original compact city and staged gameplay scope; first finish the greybox courier slice.
5. Localhost only. Public-hosting work in document 16 is deferred.
6. No trusting client positions, damage, money, seat grants, or mission completion.
7. One bounded task, actual verification, honest status. Do not mark stubs complete.

## First implementation prompt

Copy this into the implementation agent:

```text
Implement T01 from docs/13-implementation-plan.md in this repository.

First read README.md, docs/03-architecture.md,
docs/12-deployment-and-operations.md, and the T01 section.
Inspect the existing files before editing and preserve any user changes.

This is an original GTA-inspired browser sandbox with an authoritative
Rust server using harveynw/net-crate, a TypeScript/Three.js client,
Rust-owned MessagePack protocol types, and shared Rust/WASM movement.
Localhost deployment is sufficient; do not set up public hosting.

Implement T01 fully, create the implementation log, run its checks,
and stop at that task's boundary. Do not implement later gameplay early.
If a toolchain/dependency is unavailable, report the precise blocker
and leave the affected acceptance criterion incomplete.

Finish with files changed, commands and results, remaining limitations,
and the next task ID. Do not describe tests as passing unless run.
```

## Continuation prompt

```text
Continue this project by completing the next incomplete task in
docs/13-implementation-plan.md. Read README.md and
docs/implementation-log.md, inspect actual repository state, then read
the owning design documents linked by that task.

Check that prerequisite tasks actually meet their acceptance criteria.
Fix a prerequisite regression before adding the next feature.
Use the established interfaces and generated shared types.

Keep docs/diagrams/frontend.mmd and docs/diagrams/backend.mmd current
when modules, ownership, threads or significant runtime interactions
change. Preserve matching colour groups and show implemented behavior.

Implement only this task and its necessary integration.
Keep native/WASM movement behavior shared and keep authority on the
Rust server. Exercise actual net-crate transport where the task calls
for it. Do not replace RTC with a mock or a WebSocket-only shortcut.
Localhost is the deployment target; public hosting is deferred.

Run the task's verification, update the implementation log with evidence,
and stop at the task boundary. If a design contradiction blocks work,
make the smallest explicit correction to the owning document and log
the reasoning; do not silently invent a new architecture.

Report the completed task ID, important changes, commands/results,
known limitations, and next task.
```

## Targeted repair prompt

```text
Repair the failing acceptance criterion for TASK_ID:
PASTE_THE_FAILURE_HERE.

Read its owning specification and inspect the failing code/test.
Reproduce the failure before changing behavior. Make the smallest
complete fix, preserve server authority and the documented wire contract,
then rerun the regression plus directly affected checks.

Do not expand scope, weaken the assertion to make it pass, replace real
transport with a mock, or mark the task complete while another required
criterion is failing. Record the evidence in docs/implementation-log.md.
```

## Required implementation-log entry

```markdown
## Txx — Task name
Status: not started | in progress | blocked | complete
Prerequisites verified:
Files/contracts changed:
Design corrections (if any):
Commands run:
- command — pass/fail — concise result
Manual scenario evidence:
Measured budgets (if relevant):
Known limitations:
Next action:
```

Statuses describe actual work, not intent. Preserve prior evidence; add a regression note if a later change invalidates it. Do not paste massive logs or secrets.

## Review before accepting an agent’s task

- Can the advertised behavior be reproduced with the listed commands?
- Are the server and browser using the same generated DTOs?
- Was the real networking crate used when required?
- Are code paths bounded and failures explicit?
- Does the client send intent rather than authoritative outcomes?
- Are local prediction, remote interpolation, and authoritative state separate?
- Did the task respect its scope and update fixtures when contracts changed?
- Are unfinished features visible as unfinished instead of fake success?

These are implementation acceptance checks, not permission requests. Routine reversible coding/testing is already part of the implementation task.
