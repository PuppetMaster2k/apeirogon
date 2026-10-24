# Changelog

## [0.3.0](https://github.com/PuppetMaster2k/apeirogon/compare/v0.2.0...v0.3.0) (2026-10-24)


### Features

* add image clipboard ([#78](https://github.com/PuppetMaster2k/apeirogon/issues/78)) ([99c21ca](https://github.com/PuppetMaster2k/apeirogon/commit/99c21ca4fc16efc06421b14f14fe13b116774ab2))
* **gui:** add egui settings window and tray with pause ([#79](https://github.com/PuppetMaster2k/apeirogon/issues/79)) ([76e696a](https://github.com/PuppetMaster2k/apeirogon/commit/76e696a1970f285a3ceeaf3870b3dfc69ea30ac7))

## 0.2.0 - 2026-10-24

### Features

- feat(platform): add wayland clipboard backend (#68)
- feat(platform): add wayland input capture and injection (#67)
- feat(platform): add macos input capture and injection (#65)
- feat(platform): add macos clipboard backend (#64)
- feat(platform): add windows clipboard backend (#63)
- feat(cli): add a7n clip status command (#62)
- feat(platform): add x11 clipboard backend (#61)
- feat(daemon): fetch lazy clipboard text on paste (#60)
- feat: add daemon logs, autostart and a7n switch, daemon and logs commands (#59)
- feat: add text clipboard generations, exclusion and per-peer policy (#58)
- feat(platform): add windows input capture and injection (#57)
- feat: sync convenience settings across the desk (#56)
- feat: add safety hotkeys for return home, lock and jump (#55)
- feat: add toml config with a7n init and a7n config (#54)
- feat(platform): add x11 input capture and injection (#53)
- feat: route captured input through focus to the driven machine (#52)
- feat: add desk-wide layout with sync and a7n layout editor (#51)
- feat(core): add focus ownership and routing state machine (#50)
- feat(daemon): discover and dial desk members over mdns (#49)
- feat(cli): add status, pair, unpair and connect over the daemon socket (#48)
- feat(ipc): add user-only local socket api (#47)
- feat(daemon): add state-owning daemon loop (#46)
- feat(core): add signed desk membership and revocation (#45)
- feat(core): add spake2 pairing with a 6-digit code (#44)
- feat(core): add link liveness with ping, rtt and drop rules (#43)
- feat(daemon): add pinned quic link and handshake (#42)
- feat(daemon): add device identity and pin store (#41)
- feat(proto): add wire format, framing and limits (#39)

### Fixes

- fix: bind the control socket before input start and clean up wayland review findings (#70)
- fix: correct daemon crate name, licenses and msrv (#37)
