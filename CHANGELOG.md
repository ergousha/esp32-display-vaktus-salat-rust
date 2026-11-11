# Changelog

## [0.9.0](https://github.com/ergousha/esp32-display-vaktus-salat-rust/compare/v0.8.0...v0.9.0) (2026-11-11)


### Features

* move the cloud link onto the platform device SDK; no secrets in images ([49ef1d5](https://github.com/ergousha/esp32-display-vaktus-salat-rust/commit/49ef1d5055edc2a29fa4da06404b957679fc9ca4))


### Bug Fixes

* device SDK with the connection fix and p256 0.14 ([5745e9d](https://github.com/ergousha/esp32-display-vaktus-salat-rust/commit/5745e9d29d7533e2b9c308079386d2bb61504e09))
* device SDK without the empty-publish crash ([4522df9](https://github.com/ergousha/esp32-display-vaktus-salat-rust/commit/4522df9b8d3eb341877eb6c438f23fe9bf4bcc0f))
* SDK session and heap fixes; the release image carries the tag version ([e206df9](https://github.com/ergousha/esp32-display-vaktus-salat-rust/commit/e206df955c36537d879ea766cb6a3f5500f65141))

## [0.8.0](https://github.com/ergousha/esp32-display-vaktus-salat-rust/compare/v0.7.0...v0.8.0) (2026-11-09)


### ⚠ BREAKING CHANGES

* the Arabic language option is no longer available.

### Features

* AWS IoT link with the ATECC608B, OTA updates with rollback ([64a0669](https://github.com/ergousha/esp32-display-vaktus-salat-rust/commit/64a06691cc0096ced71509e285e0cd00e87f7003))
* remove the Arabic UI language to free flash for OTA ([6ca4f3a](https://github.com/ergousha/esp32-display-vaktus-salat-rust/commit/6ca4f3a482563a8b73bbe7bc40a9e9c12dc149c2))


### Bug Fixes

* review findings and an OTA memory failure found on the bench ([0a51359](https://github.com/ergousha/esp32-display-vaktus-salat-rust/commit/0a513598c55e1bb19109dc6340de023fd40a3359))
