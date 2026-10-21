# Shared protocol package

Rust in [crates/protocol](../../crates/protocol) owns all wire fields, tagged variants, scalar bounds, and schemas. This package exports generated TypeScript declarations and browser-safe codecs. It starts no network connections.

## Public API

    import {
      encodeClient, decodeServer, validateClientContext,
      protocolLimits, ProtocolError,
    } from "@port-meridian/protocol";
    import type { ClientEnvelope, ClientContext } from "@port-meridian/protocol";

Use encodeClient for client intent; it returns one MessagePack Uint8Array. Use decodeServer(bytes, "reliable" | "unreliable") for received binary frames; it returns a validated ServerEnvelope or throws ProtocolError. Both APIs are synchronous and do not partially update application state. decodeClient and encodeServer are also available for tests and local tools.

Transport text signalling does not belong in this codec. Validate envelope world/session IDs against the actual session before applying messages. validateClientContext checks phase, epoch, baseline, and scheduled input window using the generated ClientContext type; it cannot authenticate a ticket or inspect gameplay ownership, distance, cooldowns, and money.

On the Rust server, decode_client(bytes) validates structure and stateless coherence. Then call validate_client_context with the current session at application time. Build outgoing DTOs and call encode_server with an explicit Delivery. A protocol success is not authorization to execute an action.

## Generation and verification

    npm run protocol:generate
    npm run protocol:check
    npm run test:unit
    cargo test -p protocol -p xtask --locked

Generation writes types, client/server JSON Schemas, and limits under src/generated plus the shared fixture manifest under tests/fixtures/protocol. Do not edit those files. check-generated regenerates in a temporary directory, compares the complete file set and bytes, and leaves existing files unchanged on drift. The root check command includes it; the root build command regenerates.

Vitest decodes Rust-written fixtures and sends TypeScript-encoded cases to the real Rust codec via xtask verify-wire. Float32 roundoff is tolerated for floats; safe integer IDs/ticks are compared exactly. Rust tests cover context admission, scanner limits, randomized bounded bytes/truncations, and schema enum coverage. No network mock is used or needed for this serialization milestone.

## Validation details

- Both decoders preflight byte size, depth, node count, collection lengths, UTF-8, integer safety, duplicate/reserved map keys, extensions/binary values, nonfinite floats, and trailing values before decoding.
- Generated Schema 2020-12 requires named objects, every declared property including nullable fields, exact tuples, known fields/variants, and numeric/string bounds.
- Additional checks reject unsorted inputs, conflicting owner/driver state, descriptor/state mismatch, duplicate snapshot IDs, invalid status/result coherence, and unreliable control changes.
- Integers with mathematically integral float markers are accepted consistently. Rust normalizes safe whole numbers before typed deserialization; fractional integer fields fail.
- Outbound JS values must be plain, acyclic, dense wire data; undefined, accessors, classes, BigInt, binary buffers, symbols, and nonfinite numbers are rejected.
- Rust encoding uses the same named-map serializer mode as rmp-serde to_vec_named with a bounded writer, then validates its output. Error strings omit packet contents.

The Rust crate defaults to its native codec feature, including a cached schema validator with remote HTTP/file resolution disabled. Shared simulation/WASM consumers should select default-features = false for DTOs and pure validation without the native validator dependency. That mode has been compiled for wasm32-unknown-unknown.

Full wire semantics are in [document 05](../../docs/05-protocol.md). Rate limiting, action-result caching, relevance, gameplay authority, networking, and session storage belong to T03/T07 and later tasks.
