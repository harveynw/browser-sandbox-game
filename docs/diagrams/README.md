# Module diagrams

- [Frontend](frontend.mmd): browser modules, shared packages, graphics, prediction and networking.
- [Backend](backend.mmd): server modules, threads, admission, authoritative simulation and replication.

These Mermaid sources describe the implemented [architecture](../architecture.md). Open them in a Mermaid-compatible viewer to render them.

Both use purple for orchestration/input, blue for networking/sessions/replication, green for simulation/collision, orange for graphics/camera, yellow for content and grey for diagnostics/external systems. Arrows show main calls/data flows; dashed arrows show secondary relationships. Groups identify source locations, not extra runtime threads. Backend thread ownership is explicit.

Update the affected diagram alongside module, ownership, thread-boundary or significant runtime-interaction changes. Preserve matching colour meanings and reconcile arrows against actual imports and call sites.
