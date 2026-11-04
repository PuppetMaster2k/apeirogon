# Changelog

## [0.15.1](https://github.com/PuppetMaster2k/apeirogon/compare/v0.15.0...v0.15.1) (2026-11-04)


### Bug Fixes

* update workspace lockfile to 0.15.0 ([#145](https://github.com/PuppetMaster2k/apeirogon/issues/145)) ([406a03e](https://github.com/PuppetMaster2k/apeirogon/commit/406a03e2e34b07ce9e2ac07438912ab88774c613))

## [0.15.0](https://github.com/PuppetMaster2k/apeirogon/compare/v0.14.0...v0.15.0) (2026-11-04)


### Features

* add clipboard ask policy ([#99](https://github.com/PuppetMaster2k/apeirogon/issues/99)) ([7641347](https://github.com/PuppetMaster2k/apeirogon/commit/7641347ff53e672d7347de852b0d135eb791f069))
* add daemon logs, autostart and a7n switch, daemon and logs commands ([#59](https://github.com/PuppetMaster2k/apeirogon/issues/59)) ([20ce5ce](https://github.com/PuppetMaster2k/apeirogon/commit/20ce5ce4fc153e19384f43c6cdf817cb9a3b3f94))
* add desk-wide layout with sync and a7n layout editor ([#51](https://github.com/PuppetMaster2k/apeirogon/issues/51)) ([dd294a6](https://github.com/PuppetMaster2k/apeirogon/commit/dd294a6b2130b9c1b3670df8e2c25d336bdcbed2))
* add image clipboard ([#78](https://github.com/PuppetMaster2k/apeirogon/issues/78)) ([99c21ca](https://github.com/PuppetMaster2k/apeirogon/commit/99c21ca4fc16efc06421b14f14fe13b116774ab2))
* add per-peer file policy and receive directory settings ([#131](https://github.com/PuppetMaster2k/apeirogon/issues/131)) ([5e6c92c](https://github.com/PuppetMaster2k/apeirogon/commit/5e6c92c1c32edd3de7635be740cc970524b1bb58))
* add safety hotkeys for return home, lock and jump ([#55](https://github.com/PuppetMaster2k/apeirogon/issues/55)) ([96cda41](https://github.com/PuppetMaster2k/apeirogon/commit/96cda41ad9a0fb845a153ed04b43492afb647f71))
* add text clipboard generations, exclusion and per-peer policy ([#58](https://github.com/PuppetMaster2k/apeirogon/issues/58)) ([9701c1d](https://github.com/PuppetMaster2k/apeirogon/commit/9701c1d9bebd1783a02adbf1d225dad7f6c180c9))
* add toml config with a7n init and a7n config ([#54](https://github.com/PuppetMaster2k/apeirogon/issues/54)) ([4e0f54d](https://github.com/PuppetMaster2k/apeirogon/commit/4e0f54dbdb44e00e8eea3bcefd83cca6f55211e3))
* **cli:** add a7n clip status command ([#62](https://github.com/PuppetMaster2k/apeirogon/issues/62)) ([497f745](https://github.com/PuppetMaster2k/apeirogon/commit/497f745a9920c97b59abfaea102ddfe0bdd11c50))
* **cli:** add status, pair, unpair and connect over the daemon socket ([#48](https://github.com/PuppetMaster2k/apeirogon/issues/48)) ([3530eec](https://github.com/PuppetMaster2k/apeirogon/commit/3530eec6bcf75b70546e2ffbf7ec9c18750a972d))
* **core:** add focus ownership and routing state machine ([#50](https://github.com/PuppetMaster2k/apeirogon/issues/50)) ([a6213c0](https://github.com/PuppetMaster2k/apeirogon/commit/a6213c06adbdee5c93627f20cde788f9a037322c))
* **core:** add link liveness with ping, rtt and drop rules ([#43](https://github.com/PuppetMaster2k/apeirogon/issues/43)) ([ce4f02f](https://github.com/PuppetMaster2k/apeirogon/commit/ce4f02ff0a13b0bcfe10cd5861427fb934775368))
* **core:** add signed desk membership and revocation ([#45](https://github.com/PuppetMaster2k/apeirogon/issues/45)) ([0fbbc37](https://github.com/PuppetMaster2k/apeirogon/commit/0fbbc376ec317f4f725e7e410951beb11db1ebce)), closes [#8](https://github.com/PuppetMaster2k/apeirogon/issues/8)
* **core:** add spake2 pairing with a 6-digit code ([#44](https://github.com/PuppetMaster2k/apeirogon/issues/44)) ([fe9d05b](https://github.com/PuppetMaster2k/apeirogon/commit/fe9d05b5007d2604d6b9e2cf276c34e5f724038a))
* **core:** validate file manifests and plan resumed transfers ([#130](https://github.com/PuppetMaster2k/apeirogon/issues/130)) ([fda0413](https://github.com/PuppetMaster2k/apeirogon/commit/fda04138a214c260de896887743b162088d9b7ac))
* **daemon:** add device identity and pin store ([#41](https://github.com/PuppetMaster2k/apeirogon/issues/41)) ([85de928](https://github.com/PuppetMaster2k/apeirogon/commit/85de9283cfbb94d9c9de6629703fcc29e6899d84))
* **daemon:** add pinned quic link and handshake ([#42](https://github.com/PuppetMaster2k/apeirogon/issues/42)) ([f096b11](https://github.com/PuppetMaster2k/apeirogon/commit/f096b116ea352392ddd4ccb1e1c1680d35ff337b))
* **daemon:** add state-owning daemon loop ([#46](https://github.com/PuppetMaster2k/apeirogon/issues/46)) ([9eac651](https://github.com/PuppetMaster2k/apeirogon/commit/9eac651d0e759038f3328f20a1e919cb9e3af134))
* **daemon:** discover and dial desk members over mdns ([#49](https://github.com/PuppetMaster2k/apeirogon/issues/49)) ([1128d6a](https://github.com/PuppetMaster2k/apeirogon/commit/1128d6afed61036a4338d1477de4f832e31411bb))
* **daemon:** fetch lazy clipboard text on paste ([#60](https://github.com/PuppetMaster2k/apeirogon/issues/60)) ([c108b52](https://github.com/PuppetMaster2k/apeirogon/commit/c108b5231541e2a5b14fc9314fcd8323294b1d7d))
* **daemon:** no edge crossing while a mouse button is held ([#120](https://github.com/PuppetMaster2k/apeirogon/issues/120)) ([7cdc20a](https://github.com/PuppetMaster2k/apeirogon/commit/7cdc20a9a669fc707e97be6601b3a3f66fe5a117))
* **daemon:** notify about large incoming transfers and held file offers ([#140](https://github.com/PuppetMaster2k/apeirogon/issues/140)) ([e46f477](https://github.com/PuppetMaster2k/apeirogon/commit/e46f477e2ce1a5d19f93781fb35829e7059acdbf))
* file clipboard and transfer ([#134](https://github.com/PuppetMaster2k/apeirogon/issues/134)) ([f8d0d07](https://github.com/PuppetMaster2k/apeirogon/commit/f8d0d075560d5d51b31f4165265127fcb0e18b8e))
* **gui:** add design theme, fonts, icons and sidebar shell ([#103](https://github.com/PuppetMaster2k/apeirogon/issues/103)) ([da33aac](https://github.com/PuppetMaster2k/apeirogon/commit/da33aacce21ebbbb69e26dc9be80c38be83b93ba))
* **gui:** add egui settings window and tray with pause ([#79](https://github.com/PuppetMaster2k/apeirogon/issues/79)) ([76e696a](https://github.com/PuppetMaster2k/apeirogon/commit/76e696a1970f285a3ceeaf3870b3dfc69ea30ac7))
* guide macOS permission onboarding ([#106](https://github.com/PuppetMaster2k/apeirogon/issues/106)) ([4c99cfa](https://github.com/PuppetMaster2k/apeirogon/commit/4c99cfa950f7bb59dd4cc2ae090f44b5deacf381))
* **gui:** rebuild screens to the new design ([#104](https://github.com/PuppetMaster2k/apeirogon/issues/104)) ([9251d79](https://github.com/PuppetMaster2k/apeirogon/commit/9251d795cddceddd77ab5b44d4374a5ef3616566))
* **gui:** snap machines to each other and prevent overlap on the layout canvas ([#119](https://github.com/PuppetMaster2k/apeirogon/issues/119)) ([0a77ca9](https://github.com/PuppetMaster2k/apeirogon/commit/0a77ca95f4485347e7d7ff604fd9b0e6e263c902))
* **ipc:** add user-only local socket api ([#47](https://github.com/PuppetMaster2k/apeirogon/issues/47)) ([0c2debe](https://github.com/PuppetMaster2k/apeirogon/commit/0c2debe40ec3d0d3024dd5afe03212212ad45fde))
* **platform:** add macos clipboard backend ([#64](https://github.com/PuppetMaster2k/apeirogon/issues/64)) ([26110b8](https://github.com/PuppetMaster2k/apeirogon/commit/26110b878f19761b5893a8089d6c69ff378faf10))
* **platform:** add macos input capture and injection ([#65](https://github.com/PuppetMaster2k/apeirogon/issues/65)) ([0c381b2](https://github.com/PuppetMaster2k/apeirogon/commit/0c381b2d158a3f4ddba33b6680acd65c3d03563e))
* **platform:** add wayland clipboard backend ([#68](https://github.com/PuppetMaster2k/apeirogon/issues/68)) ([34ced10](https://github.com/PuppetMaster2k/apeirogon/commit/34ced1071752b77273dbf4ab4c1e29c8d6e55791))
* **platform:** add wayland input capture and injection ([#67](https://github.com/PuppetMaster2k/apeirogon/issues/67)) ([10eca16](https://github.com/PuppetMaster2k/apeirogon/commit/10eca16b1b83047e45e36cd6742f01d7fdc09f83))
* **platform:** add windows clipboard backend ([#63](https://github.com/PuppetMaster2k/apeirogon/issues/63)) ([bc7e19e](https://github.com/PuppetMaster2k/apeirogon/commit/bc7e19e5b8cb67ee5b2c2f8de9d9045230ecea5b))
* **platform:** add windows input capture and injection ([#57](https://github.com/PuppetMaster2k/apeirogon/issues/57)) ([450b57d](https://github.com/PuppetMaster2k/apeirogon/commit/450b57d7644ddfca13e2cfb8fcdef7fda0fc0ab4))
* **platform:** add x11 clipboard backend ([#61](https://github.com/PuppetMaster2k/apeirogon/issues/61)) ([b22a30e](https://github.com/PuppetMaster2k/apeirogon/commit/b22a30ecdd97feeb40357e6e5ea7b348c2dae91d))
* **platform:** add x11 input capture and injection ([#53](https://github.com/PuppetMaster2k/apeirogon/issues/53)) ([b8176ad](https://github.com/PuppetMaster2k/apeirogon/commit/b8176ade92ab62700c550bf749ca235812e301ee))
* **platform:** capture unaccelerated Windows pointer deltas via Raw Input ([#86](https://github.com/PuppetMaster2k/apeirogon/issues/86)) ([b40e8c0](https://github.com/PuppetMaster2k/apeirogon/commit/b40e8c0ab6a3aa916917edf4f07e2c231459b18f))
* **proto:** add wire format, framing and limits ([#39](https://github.com/PuppetMaster2k/apeirogon/issues/39)) ([538d00f](https://github.com/PuppetMaster2k/apeirogon/commit/538d00f7e4f187ec14a78b5a8504b9297e64e065))
* refuse file receives without free space, document files ([#138](https://github.com/PuppetMaster2k/apeirogon/issues/138)) ([b7f690f](https://github.com/PuppetMaster2k/apeirogon/commit/b7f690ff22cf4b74ca5bfcaf91689765bbecbe5e))
* route captured input through focus to the driven machine ([#52](https://github.com/PuppetMaster2k/apeirogon/issues/52)) ([9d4c831](https://github.com/PuppetMaster2k/apeirogon/commit/9d4c83178bdcb61b07087ebc0dacbdfc6263cd6e))
* swap Cmd and Ctrl between macOS and other systems by default ([#121](https://github.com/PuppetMaster2k/apeirogon/issues/121)) ([69062b3](https://github.com/PuppetMaster2k/apeirogon/commit/69062b3f39e860bcd93ccb66c75fa6759c286bdd))
* sync convenience settings across the desk ([#56](https://github.com/PuppetMaster2k/apeirogon/issues/56)) ([396f230](https://github.com/PuppetMaster2k/apeirogon/commit/396f2308bbb3422603e1f5f0faacd3508a7dbfa5))
* wake sleeping desk members with wake-on-LAN ([#142](https://github.com/PuppetMaster2k/apeirogon/issues/142)) ([9dca267](https://github.com/PuppetMaster2k/apeirogon/commit/9dca26792293c62948bdbfba0508739aa707fdd2))


### Bug Fixes

* bind the control socket before input start and clean up wayland review findings ([#70](https://github.com/PuppetMaster2k/apeirogon/issues/70)) ([895a76f](https://github.com/PuppetMaster2k/apeirogon/commit/895a76f414aa91c6c2fefa19e41ef3ecfdfbf8fb))
* correct daemon crate name, licenses and msrv ([#37](https://github.com/PuppetMaster2k/apeirogon/issues/37)) ([b1b0a4c](https://github.com/PuppetMaster2k/apeirogon/commit/b1b0a4c722181ad31c61ca2081c1cfc02ae21deb))
* **platform:** stop macOS input freezing when Accessibility is revoked ([#122](https://github.com/PuppetMaster2k/apeirogon/issues/122)) ([da829dc](https://github.com/PuppetMaster2k/apeirogon/commit/da829dc10b3f6531566c0be3b89b5fb78066db99))

## [0.14.0](https://github.com/PuppetMaster2k/apeirogon/compare/v0.13.0...v0.14.0) (2026-11-04)


### Features

* wake sleeping desk members with wake-on-LAN ([#142](https://github.com/PuppetMaster2k/apeirogon/issues/142)) ([9dca267](https://github.com/PuppetMaster2k/apeirogon/commit/9dca26792293c62948bdbfba0508739aa707fdd2))

## [0.13.0](https://github.com/PuppetMaster2k/apeirogon/compare/v0.12.0...v0.13.0) (2026-11-04)


### Features

* **daemon:** notify about large incoming transfers and held file offers ([#140](https://github.com/PuppetMaster2k/apeirogon/issues/140)) ([e46f477](https://github.com/PuppetMaster2k/apeirogon/commit/e46f477e2ce1a5d19f93781fb35829e7059acdbf))

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
