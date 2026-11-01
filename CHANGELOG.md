# Changelog

## [0.2.0](https://github.com/seuros/merge_machines/compare/merge_machines-v0.1.0...merge_machines/v0.2.0) (2026-11-01)


### ⚠ BREAKING CHANGES

* hybrid timestamps cross the boundary as nanoseconds, not Float seconds

### Features

* CRDTs and the clocks they need ([0cf6b7f](https://github.com/seuros/merge_machines/commit/0cf6b7f8de95fc40b5cdb6f05a1bf4bcafe6e7af))
* pass a rubygems OTP through the environment ([6834db9](https://github.com/seuros/merge_machines/commit/6834db97472a0028a6fa837d2cb3b845620d97e6))
* Ruby gem, magnus FFI, and a codec for every state ([6be0bda](https://github.com/seuros/merge_machines/commit/6be0bdaea6317b2bc02087db28eb374c0eea88af))


### Bug Fixes

* a crowded reading let the hybrid clock reissue one timestamp ([4c4d139](https://github.com/seuros/merge_machines/commit/4c4d13942d5ccb3b70a5666f0b7306d1487d367b))
* a merge that overran capacity kept the names of the keys it turned down ([fe5ac2a](https://github.com/seuros/merge_machines/commit/fe5ac2aaec8bf9db35e2131fb67951dcaf5c4c1b))
* a PNCounter balance was the difference of two clamped totals ([dde63bc](https://github.com/seuros/merge_machines/commit/dde63bca70683ed8ba57deb962f980fab854b776))
* a register that got its value by merging took a stale write ([ae393ef](https://github.com/seuros/merge_machines/commit/ae393efc1fdaf8bf3980d70e7ef0508a3296c36d))
* a single observed timestamp stopped a Lamport clock for good ([48f7788](https://github.com/seuros/merge_machines/commit/48f77882dffb7bdd6952c1175612ca60e69c0aae))
* a source install can build, and a failed gem push fails the release ([bf37071](https://github.com/seuros/merge_machines/commit/bf37071c1398ecc81d4185f24d89cd89e8a8abe0))
* a truncated register blob loaded as an empty one ([1aa9391](https://github.com/seuros/merge_machines/commit/1aa939148ba7f5161158b033a0a08c658ad3d9fa))
* an OrSet add vanished when a replica's tag counter ran out ([8f81ce8](https://github.com/seuros/merge_machines/commit/8f81ce8240ce9070bb50f685bd6d54750674089b))
* collect the built gems whatever depth the download puts them at ([29ccbdd](https://github.com/seuros/merge_machines/commit/29ccbddfbaee366919c2833925cfa989a5fb2a00))
* hybrid timestamps cross the boundary as nanoseconds, not Float seconds ([01c9d86](https://github.com/seuros/merge_machines/commit/01c9d8602d07be6bdfcb659ae90b8f42706130e4))
* keep the name table consistent when a merge overruns capacity ([3f34aa5](https://github.com/seuros/merge_machines/commit/3f34aa5c24bae838a1a3421f64ae061c5a6afb5b))
* refuse a fractional stamp instead of truncating it into a tie ([6d3be65](https://github.com/seuros/merge_machines/commit/6d3be65af48918f890544e8547cfeb0eb03b45a2))
* ship the core's test files so the packaged crate builds ([4651644](https://github.com/seuros/merge_machines/commit/4651644a58306aac0b6fd0e890f1f033b89adfa9))
* the hash let a short key collide with a longer zero-padded one ([0a53206](https://github.com/seuros/merge_machines/commit/0a532065246def302a6510f082779f7981a61ff3))
* the release tag carries the gem name, and the gate uses the aggregate output ([d048d8c](https://github.com/seuros/merge_machines/commit/d048d8cf92bf29b7a284a2f475c4e6f94280563d))
* two unrescuable panics, a non-commutative merge, and a lost first write ([6951483](https://github.com/seuros/merge_machines/commit/6951483b917a731359ac2b4d94c0b326439f0882))

## 0.1.0

- Initial release.
- `MergeMachines::GCounter` and `PNCounter`: one count per replica, summed for
  a value that only grows, and two of those for one that moves both ways.
- `MergeMachines::LwwRegister`: one value, held by whichever write was stamped
  last, with the replica id as the tie-break, stamped by either clock.
- `MergeMachines::GSet` and `OrSet`: a set nothing leaves, and one where a
  remove only takes out the adds it has seen, so a concurrent add survives it.
- `MergeMachines::LogicalClock` and `HybridClock`: Lamport's counter, and a
  wall-clock reading paired with a counter that breaks its ties. The hybrid
  clock reads `Process::CLOCK_REALTIME`, which may step backwards, and no
  deadline may be derived from it.
- Keys and register values are Ruby strings, carried as 64-bit hashes with a
  side table that `dump` and `load` round-trip.
- Replica counts and set capacities are const generics in the crate, so each
  size is a separately compiled type and anything off the menu raises
  `ArgumentError`.
- `merge-machines` core crate: the states and the clocks in `no_std` Rust with
  zero dependencies, plus a fixed-length codec per state. Builds and lints
  clean on bare metal.
- RBS signatures for the whole public API.
