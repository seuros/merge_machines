# Changelog

## [0.2.0](https://github.com/seuros/merge_machines/compare/merge-machines-v0.1.0...merge-machines-v0.2.0) (2026-11-01)


### Features

* CRDTs and the clocks they need ([0cf6b7f](https://github.com/seuros/merge_machines/commit/0cf6b7f8de95fc40b5cdb6f05a1bf4bcafe6e7af))
* Ruby gem, magnus FFI, and a codec for every state ([6be0bda](https://github.com/seuros/merge_machines/commit/6be0bdaea6317b2bc02087db28eb374c0eea88af))


### Bug Fixes

* a crowded reading let the hybrid clock reissue one timestamp ([4c4d139](https://github.com/seuros/merge_machines/commit/4c4d13942d5ccb3b70a5666f0b7306d1487d367b))
* a PNCounter balance was the difference of two clamped totals ([dde63bc](https://github.com/seuros/merge_machines/commit/dde63bca70683ed8ba57deb962f980fab854b776))
* a single observed timestamp stopped a Lamport clock for good ([48f7788](https://github.com/seuros/merge_machines/commit/48f77882dffb7bdd6952c1175612ca60e69c0aae))
* ship the core's test files so the packaged crate builds ([4651644](https://github.com/seuros/merge_machines/commit/4651644a58306aac0b6fd0e890f1f033b89adfa9))
