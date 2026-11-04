# Changelog

## [0.12.0](https://github.com/PuppetMaster2k/apeirogon/compare/v0.11.0...v0.12.0) (2026-11-04)


### Features

* refuse file receives without free space, document files ([#138](https://github.com/PuppetMaster2k/apeirogon/issues/138)) ([b7f690f](https://github.com/PuppetMaster2k/apeirogon/commit/b7f690ff22cf4b74ca5bfcaf91689765bbecbe5e))

## [0.11.0](https://github.com/PuppetMaster2k/apeirogon/compare/v0.10.0...v0.11.0) (2026-11-03)


### Features

* file clipboard and transfer ([#134](https://github.com/PuppetMaster2k/apeirogon/issues/134)) ([f8d0d07](https://github.com/PuppetMaster2k/apeirogon/commit/f8d0d075560d5d51b31f4165265127fcb0e18b8e))

## [0.10.0](https://github.com/PuppetMaster2k/apeirogon/compare/v0.9.0...v0.10.0) (2026-11-03)


### Features

* add per-peer file policy and receive directory settings ([#131](https://github.com/PuppetMaster2k/apeirogon/issues/131)) ([5e6c92c](https://github.com/PuppetMaster2k/apeirogon/commit/5e6c92c1c32edd3de7635be740cc970524b1bb58))

## [0.9.0](https://github.com/PuppetMaster2k/apeirogon/compare/v0.8.0...v0.9.0) (2026-11-03)


### Features

* **core:** validate file manifests and plan resumed transfers ([#130](https://github.com/PuppetMaster2k/apeirogon/issues/130)) ([fda0413](https://github.com/PuppetMaster2k/apeirogon/commit/fda04138a214c260de896887743b162088d9b7ac))

## [0.8.0](https://github.com/PuppetMaster2k/apeirogon/compare/v0.7.0...v0.8.0) (2026-11-03)


### Features

* **daemon:** no edge crossing while a mouse button is held ([#120](https://github.com/PuppetMaster2k/apeirogon/issues/120)) ([7cdc20a](https://github.com/PuppetMaster2k/apeirogon/commit/7cdc20a9a669fc707e97be6601b3a3f66fe5a117))
* **gui:** snap machines to each other and prevent overlap on the layout canvas ([#119](https://github.com/PuppetMaster2k/apeirogon/issues/119)) ([0a77ca9](https://github.com/PuppetMaster2k/apeirogon/commit/0a77ca95f4485347e7d7ff604fd9b0e6e263c902))
* swap Cmd and Ctrl between macOS and other systems by default ([#121](https://github.com/PuppetMaster2k/apeirogon/issues/121)) ([69062b3](https://github.com/PuppetMaster2k/apeirogon/commit/69062b3f39e860bcd93ccb66c75fa6759c286bdd))


### Bug Fixes

* **platform:** stop macOS input freezing when Accessibility is revoked ([#122](https://github.com/PuppetMaster2k/apeirogon/issues/122)) ([da829dc](https://github.com/PuppetMaster2k/apeirogon/commit/da829dc10b3f6531566c0be3b89b5fb78066db99))

## [0.7.0](https://github.com/PuppetMaster2k/apeirogon/compare/v0.6.0...v0.7.0) (2026-10-30)


### Features

* guide macOS permission onboarding ([#106](https://github.com/PuppetMaster2k/apeirogon/issues/106)) ([4c99cfa](https://github.com/PuppetMaster2k/apeirogon/commit/4c99cfa950f7bb59dd4cc2ae090f44b5deacf381))

## [0.6.0](https://github.com/PuppetMaster2k/apeirogon/compare/v0.5.0...v0.6.0) (2026-10-30)


### Features

* **gui:** add design theme, fonts, icons and sidebar shell ([#103](https://github.com/PuppetMaster2k/apeirogon/issues/103)) ([da33aac](https://github.com/PuppetMaster2k/apeirogon/commit/da33aacce21ebbbb69e26dc9be80c38be83b93ba))
* **gui:** rebuild screens to the new design ([#104](https://github.com/PuppetMaster2k/apeirogon/issues/104)) ([9251d79](https://github.com/PuppetMaster2k/apeirogon/commit/9251d795cddceddd77ab5b44d4374a5ef3616566))

## [0.5.0](https://github.com/PuppetMaster2k/apeirogon/compare/v0.4.0...v0.5.0) (2026-10-30)


### Features

* add clipboard ask policy ([#99](https://github.com/PuppetMaster2k/apeirogon/issues/99)) ([7641347](https://github.com/PuppetMaster2k/apeirogon/commit/7641347ff53e672d7347de852b0d135eb791f069))

## [0.4.0](https://github.com/PuppetMaster2k/apeirogon/compare/v0.3.0...v0.4.0) (2026-10-26)


### Features

* **platform:** capture unaccelerated Windows pointer deltas via Raw Input ([#86](https://github.com/PuppetMaster2k/apeirogon/issues/86)) ([b40e8c0](https://github.com/PuppetMaster2k/apeirogon/commit/b40e8c0ab6a3aa916917edf4f07e2c231459b18f))

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
