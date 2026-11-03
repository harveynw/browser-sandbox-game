# PvP and aiming deployment — 2026-11-03

Deployed workspace snapshot to 139.59.181.62: 64 connection cap, on-foot player weapon damage, death/respawn, spawn protection, elimination chat, damage cue and crosshair-converged aiming. Traffic recovery is a plan only.

Bundle SHA256: `9528f3500a2a58fd94b88b4b9e6fa0c0e61fc0fb9ddc6867f7fc3e5aa23ccd36`. Production frontend/WASM and both referenced generated content packages included. Remote release build and service restart passed. Rollback source/frontend/binary and SQLite backup: `/opt/port-meridian-rollback-pvp-20261103T215302`. Saved profiles preserved.

Full local production build passed. Latest focused verification: 94 server tests passed, one ignored; 150 frontend tests passed; typechecking and ESLint passed. Existing warnings remain. Real public browser diagnostic completed baseline and StartPlay, reporting PASS with 13 matching WS and RTC receipts. This checks transport, not multiplayer PvP visuals or 64-player capacity. No Git commit or push was performed.
