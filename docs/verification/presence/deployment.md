# Chat presence and nametag deployment — 2026-11-03

Deployed the current workspace snapshot to 139.59.181.62, including server-generated join/leave chat notices with bold names and remote-player nametags. Labels use white text on translucent black, remain 22 CSS pixels tall with a 6-pixel gap above the rendered head, and hide beyond 40m. They retain depth occlusion and release textures/materials when players leave visibility.

Bundle SHA256: `25092e85fbc2bd1eb20f0e358317e422794b555a2f6fb29f57e99d484acb1e40`. Matching frontend/WASM and both referenced generated content packages were included. Linux release build completed in 2m53s; the systemd service restarted successfully and its bundle marker matches. Rollback source/frontend/binary and SQLite backup remain at `/opt/port-meridian-rollback-presence-20261103` on the VPS; the live save database remains external.

Validation: full local production build, TypeScript, ESLint and 147 frontend tests pass. The server suite previously passed 89 tests, including broadcast-once, loading/resync suppression and chat replay; focused nametag tests cover resource cleanup, projected size/gap across distance/FOV/viewport sizes, the 40m cutoff and behind-camera visibility. Existing build warnings remain.

Public HTTPS liveness returned HTTP 200 with `live: true` and `ready: true` during the browser probe. A real public client completed baseline and StartPlay and reported PASS with 17 matching WS and 17 RTC receipts. The test diagnostic guest disconnected afterward. Evidence: `remote-transport.png`. This verifies the exercised connection, not a multiplayer capacity soak. No Git commit or push was performed.

The public game also reached Ready. Its chat DOM contained join/leave notices with the display name inside a `strong` element; the smoke-test player left the game afterward. Remote multiplayer nametag projection was covered by the rendering tests rather than a two-player public visual session.
