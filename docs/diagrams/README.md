# Current module diagrams

- [Frontend](frontend.mmd): browser modules, shared packages, graphics, prediction and networking.
- [Backend](backend.mmd): server modules, threads, admission, authoritative simulation and replication.

These Mermaid sources describe the implemented architecture through **T19**, rather than the proposed full game. Open them in a Mermaid-compatible viewer to render them.

Both diagrams use the same colour groups: purple for orchestration/input, blue for networking/sessions/replication, green for simulation/collision, orange for graphics/camera, yellow for content, and grey for diagnostics/external systems. Arrows show the main calls and data flows; dashed arrows show secondary relationships, including diagnostic tooling and queries. Groups identify source locations; they do not imply extra runtime threads. Backend thread ownership is labelled explicitly.

## Maintenance

Update the affected diagram in the same change that adds, removes or renames a module, changes ownership or thread boundaries, or changes a significant runtime interaction. Preserve matching colour meanings across both diagrams, advance the milestone annotation when appropriate, and describe only implemented behavior. Reconcile changes against actual imports and call sites; keep future plans in the design documents. Mention diagram updates in the implementation log when recording an architecture change.
