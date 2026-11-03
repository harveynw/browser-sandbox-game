# Stage 4 verification — 2026-11-02

Local implementation; no push or remote deployment. The game is available at http://localhost:4173 with the matching server running from `target/release/server --config config/dev.toml`.

## Functional checks

- 146 frontend tests pass, including deadline seeking/cancellation, independent model transforms with shared GPU assets, stowing while driving/incapacitated, bounded effect saturation/expiry, low quality, and shoulder-camera wall alignment.
- Rust workspace tests pass: 89 server tests, plus existing content/protocol/simulation/integration suites. The new ignored performance fixture was run explicitly and passed.
- TypeScript, ESLint, generated protocol/content checks, full frontend/WASM/native build, and `git diff --check` pass.
- Local connection probe passes complete baseline, StartPlay, nonce-matched WS and RTC receipts, and readiness HTTP 200 after restarting the local server.
- Full `npm run check` stops at pre-existing formatting failures in `level-editor.html` and `previous.html`. Separately, strict Clippy stops at existing `manual_is_multiple_of` and `too_many_arguments` warnings in content baking/vegetation. Neither aggregate check is claimed as passing.
- Node 23.11.0 was used (installed toolchain); repository requests Node 24. Vite retains its existing chunk-size warning.

## Measurements

Host: Apple M1 Pro, 16 GiB RAM. Browser: embedded Chromium 156, 741 × 933 rendering pixels, DPR 1. Browser fixture uses production model/rig/batching/effect classes, a simple ground plane and lights, no city streaming, audio, shadows or live networking. CPU frame duration includes update and WebGL submission, not a GPU timer. rAF includes scheduling. Concurrent builds can affect these measurements; low-quality p95 being higher is not evidence it is intrinsically slower.

| Fixture | Load | CPU frame p50 / p95 | rAF p50 / p95 | Maximum draw calls |
|---|---|---|---|---|
| High quality | 16 machine-gun rigs, 158.4 measured shots/s (160 scheduled) | 0.50 / 0.80 ms | 16.70 / 17.50 ms | 52 |
| Low quality | 16 machine-gun rigs, 160.3 measured shots/s over sampled window | 0.60 / 1.90 ms | 16.70 / 17.50 ms | 51 |

Saturation holds at 32 flashes/tracers, 64 casings, 96 particles; low quality emits no casings and one particle per impact. Pools expire and replace cosmetic work without affecting authoritative state. The audio bus retains its 32-voice bound and 70m distance cutoff.

Release server synthetic fixture: 16 admitted/ready peers, 90 simulation ticks (three seconds), 480 unique accepted shots, **160 shots/s**. Tick p50 **1.334 ms**, p95 **4.496 ms**, maximum **22.295 ms**. Aggregate serialized server JSON output **1,313,610 bytes/s** across all peers; this includes snapshots/status/events but excludes WebSocket, RTC, TLS and packet overhead. Test-only magazine replenishment sustains peak fire without reload pauses. The fixture uses the slice world and fake transport; this is not a public-network or full-city capacity claim.

Reproduce server load:

```sh
cargo test -p server --lib --release --locked weapon_presentation_load_16_players -- --ignored --nocapture
```

Reproduce rendering fixture (development only, excluded from production client entry points):

```sh
node node_modules/vite/bin/vite.js --host 127.0.0.1 --port 5174
# Open http://127.0.0.1:5174/tools/weapon-benchmark.html
```

## Visual evidence and remaining scope

Reviewed the three silhouettes, lowered carry, and all three local shoulder viewpoints in the shared rendering fixture. Automated tests check reload interruption and stowing. Screenshots: [lineup](lineup.png), [pistol shoulder](pistol-shoulder.png), [rifle shoulder](rifle-shoulder.png), [machine gun shoulder](machine-gun-shoulder.png), [high load](load-high.png), [low load](load-low.png).

Impacts classify visible vehicles as metal, characters as neutral dust, ground as earth, and static obstacles as masonry; there is no new authoritative surface-material contract. No predicted flash or hit marker was added, so each accepted event plays once through existing event deduplication. Near-wall muzzle correction and lowering are cosmetic; damage still uses the server's chest-origin ray. Camera kick is omitted for all settings; reduced motion additionally suppresses rig recoil/bobbing and tracers and reduces flash size/duration. Supply stands remain visual-only to preserve navigation clearance.

Stage 5 remains the dedicated 98.css weapon workbench and final integration/handoff. This fixture is a developer verification tool, not that product workbench. Live multiplayer visual review and subjective sound tuning can continue during playtesting; the metrics above are deliberately isolated measurements.

## Remote deployment — 2026-11-02

Deployed the current workspace snapshot (including removal of drops, full starting loadout and centred 50%-width `/weapons` window) to 139.59.181.62. Bundle SHA256: `3bfb6482bc7b894de183c58aefe1bfcd2726b482b487ef344ed86d7b29f8f0ea`. Both referenced generated content packages and matching frontend/WASM were shipped. The Linux native release build completed in 2m29s and systemd restarted successfully. Deployment and SQLite backup: `/opt/port-meridian-rollback-weapons-20261102` on the host. Saves remain in the external database.

Public HTTPS weapon preview was visually verified. A real public diagnostic client completed baseline and StartPlay, then reported `PASS` with 16 nonce-matched WS and 16 RTC receipts; the test guest disconnected afterward. Evidence: `remote-weapons.png` and `remote-transport.png`. Local production build, lint and 145 frontend tests passed before upload. This was a workspace snapshot deployment; no Git commit/push was performed.
