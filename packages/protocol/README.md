# Shared protocol package

Rust in [crates/protocol](../../crates/protocol) owns the wire types. This package exports generated TypeScript declarations and a thin `@msgpack/msgpack` codec. It starts no connections.

```ts
import { encodeClient, decodeServer } from "@port-meridian/protocol";
import type { ClientEnvelope } from "@port-meridian/protocol";
```

`encodeClient` writes one MessagePack `Uint8Array`; `decodeServer(bytes, delivery)` decodes a server envelope. `decodeClient` and `encodeServer` support tests/tools. Encoders check the resulting packet size, without decoding their output again. Trusted callers construct typed plain data; no recursive JavaScript object sanitizer runs.

## Trust boundary

- Rust decodes directly into Serde DTOs using rmp-serde, with frame/depth limits and a trailing-data check. Scalar deserializers retain finite/range checks; input batches contain 1–3 unique ascending ticks.
- Browser decoding checks frame size, library string/array/map limits and `v: 3`, then trusts the server payload as its generated TypeScript type. This is intentionally a type assertion, not runtime schema validation.
- Server session/world code owns tickets, scope, phase, epochs, ownership, input windows, rates, cooldowns, missions and money. Client session/replica code still owns baseline and visibility ordering. Parsing does not authorize an action.
- Server-message gameplay relationships are tested in their owning systems, rather than rechecked inside both codecs. There is no protocol-level duplicate/special-key scanner or exhaustive semantic validator.
- Senders use named maps and serialize absent `Option` values as null. Serde defaults accept omitted optional fields and ignore unknown fields. Unknown Rust enum variants still fail. Integer fields use ordinary MessagePack integer encoding; no JSON numeric-normalization stage remains.

Protocol v3 explicitly marks this acceptance-policy change; old v2 clients must reload. Canonical message field names/tagged shapes are unchanged. The independent local WASM/content versions remain 1; their schema validation still exists where owned by those boundaries.

## Generation and verification

```sh
npm run protocol:generate
npm run protocol:check
npm run test:unit
cargo test -p protocol --locked
```

Generation writes TS declarations, schema metadata and limits under `src/generated`, plus a compact shared fixture manifest. Schemas describe data for tooling/documentation; they are not loaded by the game packet codecs. Do not edit generated files. The root build/check commands verify drift.

Fourteen representative positive fixtures cover handshake, batched inputs, nested actions, characters/vehicles, snapshots/status and maximum realtime packets. Four malformed/version examples cover shared decoder failures. Vitest sends TS-encoded examples to the actual Rust codec via `xtask verify-wire`; safe integer IDs/ticks compare exactly and floats allow f32 roundoff. Rust tests cover client types/ranges, malformed/truncated/deep/oversized frames, small ordered batches and intended Serde defaults. The full server, WASM prediction and transport suites exercise gameplay and lifecycle behavior.

See [the current protocol contract](../../docs/05-protocol.md) and [the simplification record](../../docs/34-protocol-simplification.md).
