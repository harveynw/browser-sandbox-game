# 34 — Protocol simplification

The user approved a smaller, more trusting game codec after T19. This follow-up replaces the original scanner/JSON/schema/semantic pipeline with direct Serde/MessagePack. Network revision is **3**, with matching build ID `protocol-simple-local`. This is a deliberate acceptance-policy change; reload old browsers. SQLite saves, game message fields, content hashes and local WASM version 1 remain intact.

## Current path

Rust receives bytes → checks frame size → deserializes into its DTOs with rmp-serde and a depth limit → checks trailing bytes and the small input-batch invariant. Scalar deserializers retain numeric/text bounds. Encoding uses `to_vec_named`, checks output size and returns; it does not decode its own output.

The browser receives bytes → checks frame size → calls `@msgpack/msgpack` with its string/array/map limits → checks revision 3 → trusts the result as the generated server type. TypeScript declarations are compile-time checks; this cast intentionally does not prove the server object's runtime shape. Encoders trust our typed callers to supply plain DTOs.

Session/world code continues to validate tickets, actual world/session IDs, phase, epochs, tick windows, ownership, rates, distance, cooldowns, prices, credits and mission completion. Client session/replica code continues to handle baseline/visibility ordering. Those rules are not delegated to serialization.

Removed:

- Both handwritten MessagePack scanners and their duplicate/special-key/total-node policies.
- Runtime JSON Schema validators, Rust JSON intermediates and numeric normalization.
- Encode-then-decode validation and recursive JavaScript object sanitization.
- Duplicated server-message relationship checks and unused protocol session-context API.
- Custom required-null deserializers and unknown-field rejection on shared DTOs.
- The protocol package's Ajv dependency and native jsonschema dependency. Content/WASM still use their own schema validation.

Normal Serde behavior now applies: unknown fields are ignored, missing Option fields become None, unknown enum variants fail and supported positional input representations are accepted. Canonical output remains named maps with explicit nulls. Integer fields use normal integer encoding; the old JSON float-to-integer normalization is gone. Native decoders and the trusted browser decoder intentionally provide different levels of checking.

`Empty` also follows Serde defaults. HTTP control-plane JSON requests keep their separately owned strict `EmptyBody` contract, preserving the existing HTTP admission tests.

## Size and fixtures

Source under `crates/protocol/src/*.rs` and `packages/protocol/src/*.ts` fell from **2,504 to 1,067 lines** (about 57%), excluding generated artifacts/tests. The Rust codec is 58 lines, its remaining batch check 15, and the browser codec 69. DTO declarations make up most of what remains. `fixtures.rs` fell from **800 to 264 lines**, with fourteen representative positive and four malformed/version cases.

The corpus checks real Rust↔TS interoperability, nested variants, characters/vehicles, status and worst-case RTC envelope sizes. Tests explicitly verify that invalid client types, versions, nonfinite/range-invalid controls and malformed batches still fail on the Rust boundary. Tests also record that server relationship inconsistencies are now trusted by the codec. Authority and lifecycle coverage stays in the owning gameplay/prediction/transport suites. The total test count is lower because the exhaustive codec guard corpus was intentionally retired.

Generated schemas remain metadata for tools/documentation. Three local WASM schema artifacts changed to reflect the shared DTO field policy; the independently owned bridge codec, its limits and movement algorithms were not replaced. Content drift verification retained hash `a322579fa1fedb5e3c058552fa96bcc8e22e0b21a36098b883b6cc35741c9939`.

The production session JS chunk changed from **173.32 kB / 47.73 kB gzip** to **31.96 kB / 10.31 kB gzip**. This is a measured chunk-size comparison, not a total download or frame-time benchmark. The separate Three.js chunk advisory remains.

## Verification and open transport issue

- `npm run check`: **110 TypeScript / 132 Rust tests pass**, plus strict typing, lint/formatting, generated protocol/content/bridge drift, release WASM and Clippy with warnings denied. The actual socket tests are explicitly ignored in the default native runner and run separately.
- `npm run build`: passes.
- The separately run healthy RTC/blackout/backpressure/reconnect test passes: five recycled-ID kills/rejoins, stale-generation isolation and a sixth-peer send-buffer rejection at 65,500 outstanding bytes under its 65,536-byte limit. [Captured healthy transport run](evidence/protocol-simple-healthy-rtc.txt).
- Production browser using real WS/RTC: joined the v3 SQLite-backed server, retained 150 credits/one completed job, walked to Quay, accepted and cancelled courier, entered/drove/braked/exited a compact and completed Resync world. Epoch advanced to 4, both paths had 75 receipts, correction p95 was 0.000 m and pending/fallback chunks were zero. [Live v3 evidence](evidence/protocol-simple-v3-live.png). No live actor/economic state injection was used.

The deliberately lossy native RTC test **does not currently pass its progress threshold**. It also fails with the committed pre-simplification v2 codec in a temporary checkout, using the same paced diagnostic probe. Within eight seconds, all 50 requests were sent but v3 observed two unique replies and the old codec observed five; both fail the unchanged requirement of more than fifteen. The original short-window v3 probe also failed (nine receipts). These observations isolate an existing loss-path/test problem; they do not identify its precise dependency or harness cause.

The actual transport test is split so this loss failure cannot prevent independent healthy-path, blackout recovery, backpressure and rapid reconnect checks from running:

```sh
cargo test -p server --test actual_transport real_rtc_backpressure_and_rapid_kill_reconnect --locked -- --ignored --nocapture
cargo test -p server --test actual_transport real_rtc_loss_and_blackout_recovery --locked -- --ignored --nocapture
```

The second command remains an open **T19** regression investigation, with likely owners `server/tests/support/rtc.rs`, the UDP relay and the pinned RTC dependency. First inspect datagram direction/SCTP acknowledgement/abandonment progress; do not lower the threshold or hide the failure. T19 is reopened as incomplete and T20 remains unstarted. Earlier T19 passing evidence is historical, not a current all-loss-path pass claim. [v3 failing probe](evidence/protocol-simple-loss-v3.txt), [old-codec comparison](evidence/protocol-simple-loss-baseline.txt).
