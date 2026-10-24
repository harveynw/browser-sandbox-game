# 15 — Decisions, risks, and sources

[Index](../README.md)

## Decisions

| ID | Decision | Rationale | Revisit only if |
|---|---|---|---|
| D01 | Localhost is the required deployment | User explicitly selected this scope | Public hosting is requested |
| D02 | 16-player single authoritative Rust shard | Small, testable multiplayer model | Measured demand requires more |
| D03 | Keep net-crate, pin source revision | User’s networking crate is a requirement | A tracked bug fix requires a newer/fork revision |
| D04 | TypeScript + Three.js browser presentation | Direct browser integration with a small rendering layer | The user selects a different frontend |
| D05 | Rust DTOs → generated TS/Schema | One source of DTO shape; schemas are metadata for game packets | Codegen compatibility fails the early fixture spike |
| D06 | MessagePack named maps | Explicit readable fields and straightforward cross-language compatibility | Actual bandwidth profiling justifies a versioned packed format |
| D07 | Shared native/WASM movement | Avoid duplicate Rust/TS movement implementations | Measured WASM overhead remains unacceptable after boundary tuning |
| D08 | Arcade kinematic cars, flat roads | Tractable prediction, collision, and implementation scope | A later milestone explicitly needs full vehicle dynamics |
| D09 | 30 Hz simulation/input, 10 Hz snapshots | Modest server cost and bounded browser networking | Measured gameplay/performance shows a better tradeoff |
| D10 | Independent full state records, no delta baseline | Loss recovery and debugging stay simple | Measured replication cannot meet budgets after AOI/rate tuning |
| D11 | No PvP or rewind shooting in alpha | Avoid competitive lag compensation and griefing rules | A separate PvP design exists |
| D12 | Complete static collision, streamed visuals | Prediction never depends on an unloaded visual chunk | Collision package exceeds measured memory/load budgets |
| D13 | SQLite guest profiles; no world persistence | Useful local progress with a small transactional boundary | Requirements add durable world state |
| D14 | Public-hosting notes remain optional | Preserve transport research without blocking localhost | Public deployment enters scope |
| D15 | Direct Serde/MessagePack game codec; browser trusts server DTOs | User approved simpler protocol after T19; retain frame/input bounds and authority | Concrete failures justify another boundary check |

## Early experiments and exit criteria

These validate chosen designs rather than reopening every decision:

| Risk | Task | Experiment and decision |
|---|---|---|
| Crate’s actual browser readiness/signalling | T03 | Real WS+RTC round trip; fix adapter or reproduce crate bug before gameplay |
| Generated Rust/TS/Schema mismatch | T02 | Every variant cross-decodes; simplify unsupported attributes instead of duplicating DTOs |
| Large OwnerState/input maps | T02 | Encode maximum values; must fit 1,100 bytes or explicitly revise schema |
| Native/WASM motion divergence | T06 | Shared recorded traces; inspect version/features/order before adding smoothing |
| Tight-corner vehicle collision | T10 | Fast wall/corner tests; reduce cast step or add conservative rotation checks |
| AI pathfinding work | T13/T20 | Bounded requests and entity counts; measure before replacing graph search |
| Renderer overload | T18/T20 | Reference laptop profile; reduce art/draw calls before changing renderer |
| Crate queues fail normal local load | T19/T20 | Minimal saturation reproducer, scoped crate fix, pinned new revision |
| Recycled peer ID race | T03/T19 | Rapid open/close/kill stress; a true internal race needs a crate-level fix |
| Public network reachability | Deferred | No local task depends on this experiment |

The remaining product defaults—city theme, controls, map size, player cap, stylized look, mission mix—are design assumptions selected to make the request concrete. They can be tuned later without redesigning the transport. They were not supplied as user requirements.

## Verified networking sources

Inspected source at immutable commit `77b401b1376b5c8bcb98da2884efcb1e63082ba4` on 20 October 2026:

- [Public exports](https://github.com/harveynw/net-crate/blob/77b401b1376b5c8bcb98da2884efcb1e63082ba4/src/lib.rs)
- [Server handle and actor](https://github.com/harveynw/net-crate/blob/77b401b1376b5c8bcb98da2884efcb1e63082ba4/src/server/mod.rs)
- [Event payloads](https://github.com/harveynw/net-crate/blob/77b401b1376b5c8bcb98da2884efcb1e63082ba4/src/event.rs)
- [Event queue](https://github.com/harveynw/net-crate/blob/77b401b1376b5c8bcb98da2884efcb1e63082ba4/src/queue.rs)
- [Connection handling](https://github.com/harveynw/net-crate/blob/77b401b1376b5c8bcb98da2884efcb1e63082ba4/src/server/connection.rs)
- [RTC channel settings](https://github.com/harveynw/net-crate/blob/77b401b1376b5c8bcb98da2884efcb1e63082ba4/src/server/webrtc/mod.rs)
- [RTC binding/configuration](https://github.com/harveynw/net-crate/blob/77b401b1376b5c8bcb98da2884efcb1e63082ba4/src/server/webrtc/config.rs)
- [Signalling parser](https://github.com/harveynw/net-crate/blob/77b401b1376b5c8bcb98da2884efcb1e63082ba4/src/server/webrtc/signal.rs)
- [Browser example](https://github.com/harveynw/net-crate/blob/77b401b1376b5c8bcb98da2884efcb1e63082ba4/example_clients/javascript/client.js)
- [Readiness tests](https://github.com/harveynw/net-crate/blob/77b401b1376b5c8bcb98da2884efcb1e63082ba4/tests/connection_readiness.rs)
- [End-to-end transport tests](https://github.com/harveynw/net-crate/blob/77b401b1376b5c8bcb98da2884efcb1e63082ba4/tests/end_to_end.rs)

Source inspection verifies code structure and intended behavior, not runtime reliability. The source tests were read, not executed during design. The old search-cached README used Vec bytes and different example paths; pinned source takes precedence.

## Library references

These support the selected building blocks; gameplay architecture and tuning values are original design choices.

- [Three.js WebGLRenderer](https://threejs.org/docs/pages/WebGLRenderer): WebGL2 rendering requirement and renderer diagnostics.
- [ts-rs](https://docs.rs/ts-rs/latest/ts_rs/): Rust-derived TypeScript declarations, export APIs, and supported Serde attributes.
- [rmp-serde named-map encoding](https://docs.rs/rmp-serde/latest/rmp_serde/encode/fn.to_vec_named.html): use field-name maps rather than positional struct encoding.
- [MessagePack JavaScript implementation](https://github.com/msgpack/msgpack-javascript): TypeScript-compatible encode/decode and configurable decoding limits.
- [Schemars](https://docs.rs/schemars/latest/schemars/): generate JSON Schema from Rust types; explicitly configure and pin schema generation.
- [Ajv schema language guide](https://ajv.js.org/guide/schema-language): use the matching 2020-12 validator export.
- [Rapier documentation](https://www.rapier.rs/docs/): Rust physics/query engine and supported bindings.
- [Rapier character controller concepts](https://rapier.rs/docs/user_guides/javascript/character_controller/): collision-aware kinematic movement; our Rust adapter wraps the chosen Rust API.
- [Rapier serialization/determinism notes](https://www.rapier.rs/docs/user_guides/templates/serialization/): determinism has requirements; shared code alone is not a world synchronization guarantee.
- [wasm-bindgen guide](https://rustwasm.github.io/docs/wasm-bindgen/print.html) and [wasm-pack build documentation](https://wasm-bindgen.github.io/wasm-pack/book/commands/build.html): exposing Rust to the browser and generating web-target packages. Resolve tool versions during setup.

Do not interpret mutable `latest` documentation URLs as a dependency pin. T01 records actual selected versions; the compatibility tests, lockfiles, and source revision determine what is shipped.
