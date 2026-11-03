# Firefox audio fix — 2026-11-03

The reported disconnect was caused by assuming AudioListener.positionX exists. Added feature-detected position/orientation updates using legacy setPosition/setOrientation when AudioParams are absent, including panner position updates. Unsupported spatial APIs degrade without throwing.

Typechecking, ESLint, five audio tests and the production frontend build passed. Tests cover the legacy listener shape reported by Firefox; no real Firefox browser session was available for verification.

Published frontend and matching audio source files to the existing remote service without restarting it. Previous frontend retained at /opt/port-meridian-firefox-previous-dist-20261103. The base deployment marker remains the PvP bundle; DEPLOYED_FRONTEND_PATCH records the additional archive hash.
