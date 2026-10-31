# Changelog

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
