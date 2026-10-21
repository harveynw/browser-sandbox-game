# 22 — Browser walkthrough

[Index](../README.md) · Implemented in T08, 21 October 2026.

This document records the T08 scene/input/camera checkpoint. T09 replaces its direct owner-sample display and input scheduling with prediction/reconciliation and remote interpolation; [23 — Prediction and replication](23-prediction-and-replication.md) owns the current networking behavior. The camera framing and drag-look controls described here remain current.

Run `npm run dev` and `cargo run -p server --locked` in separate terminals. At localhost:5173 choose Play, wait for admission, then Resume / mouse look. WASD/arrows move, Shift sprints, Space jumps, mouse look rotates, Esc pauses. When pointer lock is declined, the canvas uses explicitly focused keyboard controls and left-drag orbit; no desktop automation permission is needed. Blur, menu focus, pointer-lock loss and hidden tabs neutralize input. The online world continues while paused. Returning from a hidden tab requests a fresh baseline/epoch.

The root route is the game. `/diagnostic` retains the T07 scripted transport and movement diagnostic; `/map` and `/simulation` remain server-independent. Menu settings include mouse sensitivity, Y inversion, reduced motion and Low/Medium pixel-ratio caps (1 / 1.5). Settings are per mounted app; control remapping, persistence and audio remain T18.

## Ownership and timing

`game/app.ts` owns phases, one rAF loop, one 30 Hz input timer and the mounted input listener lifetime. `net/session.ts` owns guest/ticket admission, scope validation, baseline readiness, epochs, pings and one WASM query world. `net/diagnostic.ts` preserves the diagnostic import. The app receives typed asset/owner/status/phase callbacks; render modules do not mutate authority.

`input/input.ts` samples bounded axes and a single jump edge. Each send derives a future tick from the latest server sample plus three ticks of lead, and skips ticks already sent. There is no accumulated send catch-up. Stale owner samples trigger resync; missing inputs brake on the server. Input redundancy, latency-adaptive lead, replay rings and owner reconciliation remain T09. A tab that stays hidden long enough may be disconnected by existing server deadlines and must retry.

The only displayed character pose is the latest authoritative CharacterMotor, received at 10 Hz. Consequently movement has visible latency/stepping. There is no TS movement implementation or unacknowledged local movement. The loaded WASM motor is reset on StartPlay and provides shared static camera queries. Remote players are admitted by the server but are not rendered/motion-replicated until T09. Parking anchors are reservations, not interactive cars.

## Camera and resources

`render/camera.ts` orbits/sweeps from feet + 1.45 m, looking at feet + 1.8 m to place the head near the centre cross and the body below it, at 4.5 m distance with no horizontal offset, centred on the player, FOV 65°. At yaw zero, forward movement is −Z and the camera is on +Z; negative camera pitch puts the camera above the target. Mouse-right decreases yaw, rotating the view to the right. A green screen-centre debug cross appears while controls are active and hides in menus. Camera pitch and wire aim pitch have opposite signs. Pitch stays within −1.1 to +0.65 radians.

A native Rapier sphere sweep and the generated WASM bridge use radius 0.2 m, returning the sphere center's first-contact distance. The camera subtracts another 0.1 m margin, shortens immediately, and eases outward over 0.2 s. Reduced motion uses the unobstructed distance immediately. The character fades below 0.8 m and hides below 0.15 m. The cast includes both static physical and vision blockers, including the movement-only ground; dynamic car geometry is T10.

One renderer shares primitive geometry/materials across the four verified chunks and a capsule character. Leave/error frees graphics and the session's WASM allocation, closes transport, and aborts asset requests. Retry creates fresh resources; app teardown additionally cancels rAF, clears the input timer and removes all mounted listeners. WebGL context loss pauses controls, attempts renderer recreation on restoration and resyncs. An eight-second recovery deadline exposes Reload page. This recovery path is implemented but has not been forced in the live browser.

## Verification and limits

See the implementation log for commands and exact live measurements. Automated tests cover control focus/blur/hidden-state handling and listener disposal, orbit conventions, immediate camera shortening/outward easing, native wall/corner/awning sphere clearance and real release-WASM sphere queries. The in-app browser demonstrated actual server walking, jumping, drag orbit, pause blocking, low quality/reduced motion, camera shortening and repeated join/leave with zero/one server session counts.

The in-app browser declines pointer lock; drag-look was verified there. Successful native pointer lock has not been verified in this chunk, and desktop browser permissions were not pursued after the user's preference to use the existing browser. No multiplayer synchronization, driving, jobs, frame-rate/capacity benchmark or full WebGL recovery test is claimed.
