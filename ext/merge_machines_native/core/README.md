# merge-machines

Conflict-free replicated data types and the clocks they need. `no_std`, zero
dependencies, no default features: `alloc` adds the `encoded` helpers that
return a boxed frame, and `std` implies `alloc`.

Every type here merges commutatively, associatively and idempotently, so
replicas converge without a coordinator, without ordering their messages and
without caring how many times one arrives. Nothing here talks to a network and
nothing here reads a clock: the caller owns the transport, the replica ids, the
tags and every clock reading.

## Counting what two replicas counted separately

One shared number cannot be merged. Two replicas that both saw 5 and both added
1 have no way to tell 6 from 7. One count per replica can be merged, because no
replica ever writes another's count.

```rust
use merge_machines::{GCounter, Merge};

let mut edge = GCounter::<4>::new();
let mut origin = GCounter::<4>::new();

assert_eq!(edge.increment(1, 3), Ok(()));
assert_eq!(origin.increment(2, 7), Ok(()));

assert_eq!(edge.merge(&origin), Ok(()));
assert_eq!(edge.value(), 10);

// The same state again is absorbed, not added.
assert_eq!(edge.merge(&origin), Ok(()));
assert_eq!(edge.value(), 10);
```

`PNCounter` is two of those, one for what went up and one for what came down,
because a maximum cannot merge a subtraction.

```rust
use merge_machines::{Merge, PNCounter};

let mut left = PNCounter::<4>::new();
let mut right = PNCounter::<4>::new();

assert_eq!(left.increment(1, 10), Ok(()));
assert_eq!(right.decrement(2, 4), Ok(()));

assert_eq!(left.merge(&right), Ok(()));
assert_eq!(left.value(), 6);
```

## Ordering events with no clock at all

Lamport's counter advances on every local event and jumps past any timestamp it
is shown, so a receive is always stamped after the send it answers. A counter
with nothing left answers `Refusal::Exhausted` instead of handing out a
timestamp it already gave away, and stays where it was.

```rust
use merge_machines::{LogicalClock, Refusal};

let mut sender = LogicalClock::new();
let mut receiver = LogicalClock::new();

let sent_at = sender.tick()?;
receiver.tick()?;

assert!(receiver.observe(sent_at)? > sent_at);
assert_eq!(receiver.observe(u64::MAX), Err(Refusal::Exhausted));

Ok::<(), Refusal>(())
```

## Timestamps that also mean something to a human

A Lamport counter orders events but says nothing about when they happened. A
hybrid logical clock pairs a wall-clock reading with a counter: close to
physical time, still totally ordered, and still monotonic when the host's clock
is corrected backwards.

The reading you pass in is physical time two machines can compare, nanoseconds
since the Unix epoch. It is **not** a monotonic reading, and nothing derived
from it is a deadline, a timeout or a budget: measuring elapsed time is the
monotonic clock's job, and this crate has no business with it.

A counter with nothing left answers `Refusal::Exhausted` here too, rather than
handing out a timestamp it already gave away.

```rust
use merge_machines::{HybridClock, HybridTimestamp, Refusal};

const MS: u64 = 1_000_000;
let mut clock = HybridClock::new();

let before = clock.tick(1_000 * MS)?;
let during = clock.tick(1_000 * MS)?;
let after = clock.tick(400 * MS)?;

assert!(during > before);
assert!(after > during);
assert_eq!(after.wall_ns, 1_000 * MS);

let crowded = HybridTimestamp::new(1_000 * MS, u64::MAX);
assert_eq!(clock.observe(400 * MS, crowded), Err(Refusal::Exhausted));
assert_eq!(clock.last(), after);

Ok::<(), Refusal>(())
```

## One value, last writer wins

Stamps order the writes; the replica id breaks the ties. Both sides of a
concurrent write pick the same winner, which is the whole point, and the loser's
value is gone, which is the price. `set` says which happened: `Written`,
`Superseded` for a write that lost (the register working, not a refusal), or
`Duplicate` for the write already held. One stamp and replica naming two
values is `Refusal::Mismatch`, from `set` and from `merge` alike.

```rust
use merge_machines::{LwwRegister, Merge, Refusal, Writing};

let mut left = LwwRegister::new(10_u32, 5_u64, 1);
let right = LwwRegister::new(20_u32, 5_u64, 2);

assert_eq!(left.merge(&right), Ok(()));
assert_eq!(*left.value(), 20);
assert_eq!(left.set(30, 4, 9), Ok(Writing::Superseded));
assert_eq!(left.set(20, 5, 2), Ok(Writing::Duplicate));
assert_eq!(left.set(21, 5, 2), Err(Refusal::Mismatch));
```

## Sets, with and without removal

`GSet` only grows, so merging is the union and there is nothing to resolve.
`OrSet` tags every add and tombstones the tags a remove has seen, so a remove
races a concurrent add and loses, and re-adding afterwards works. The caller
picks the tags: one per add, unique per replica. A tombstone lives on the add
it removes, so a remove never needs a slot and never refuses.

```rust
use merge_machines::{Adding, Merge, OrSet, Removing, Tag};

let mut shared = OrSet::<u16, 8>::new();
assert_eq!(shared.add(1, Tag::new(1, 1)), Ok(Adding::Added));

let mut remover = shared;
let mut adder = shared;
assert_eq!(remover.remove(&1), Removing::Removed);
assert_eq!(adder.add(1, Tag::new(2, 1)), Ok(Adding::Added));

assert_eq!(remover.merge(&adder), Ok(()));
assert!(remover.contains(&1));
```

## Capacity is a type parameter

Every state is sized at compile time: `GCounter<4>` holds four replicas,
`OrSet<T, 8>` eight adds, tombstoned or not. A state with no free slot answers
`Refusal::Full` rather than allocating, which is why the examples above check
what `increment` and `add` returned.

```rust
use merge_machines::{GCounter, Merge, Refusal};

let mut counter = GCounter::<1>::new();

assert_eq!(counter.increment(1, 1), Ok(()));
assert_eq!(counter.increment(1, 1), Ok(()));
assert_eq!(counter.increment(2, 1), Err(Refusal::Full));

// A merge that does not fit lands nothing at all.
let mut other = GCounter::<1>::new();
assert_eq!(other.increment(2, 9), Ok(()));
let before = counter;
assert_eq!(counter.merge(&other), Err(Refusal::Full));
assert_eq!(counter, before);
```

## The contract

- **One `Refusal`.** Every reason a call is not honoured is a variant of
  `Refusal`; `Merge::merge` on this crate's types answers it too. The trait
  keeps an associated `Error` so a sibling crate can implement `Merge` with its
  own refusal.
- **A refusal changes nothing.** A merge is checked whole before anything
  moves: a `merge` lands completely or not at all, with no partial union left
  behind. A refused `increment`, `insert`, `add`, `set`, `tick`, `observe` or
  `decode_into` leaves the value exactly as it was.
- **Repeats are honoured by name.** Inserting an element already in a `GSet`
  is `Inserting::Duplicate`; adding one element under a tag that already added
  it is `Adding::Duplicate` (tombstoned or not); removing an element with no
  live add is `Removing::NotHeld`; setting the write already held is
  `Writing::Duplicate`; merging a state already seen is `Ok(())`. All change
  nothing. A tag or stamp repeated with a different payload is
  `Refusal::Mismatch`.
- **Nobody issues handles here.** Replica ids and `OrSet` tags are the
  caller's. The clocks issue stamps, and those are protocol numbers every peer
  must agree on and compare, not handles to state this crate holds: they carry
  no scope and are bare `u64`s.
- **Time.** No call takes a monotonic `now`, so no answer records one.
  `HybridTimestamp::wall_ns` is an event-time stamp (nanoseconds since the
  Unix epoch) and the logical counters are logical stamps; both stay `u64`. The
  hybrid clock folds the caller's `wall_ns` into each stamp it issues; a
  refused stamp folds nothing.

| Refusal | Meaning |
| --- | --- |
| `Full` | The call needs a slot and the fixed capacity has none left. |
| `Overflow` | A replica's count would pass `u64::MAX`. |
| `Exhausted` | A clock has issued the last stamp its counter can hold. |
| `Mismatch` | One tag, or one stamp and replica, already names another value. |
| `Truncated` | The buffer is shorter than the frame. |
| `WrongStructure` | The frame's leading byte names another structure. |
| `WrongShape` | The frame was written at another capacity, replica count or stamp kind. |
| `Malformed` | The frame describes a state no sequence of calls produces. |

## Putting state on a wire

A state that cannot be sent cannot replicate. Each one encodes to a fixed-length
frame of its own `ENCODED_LEN` bytes, led by a tag naming the structure and,
where there is one, a field naming the const generic.

```rust
use merge_machines::{GCounter, Merge};

let mut local = GCounter::<4>::new();
assert_eq!(local.increment(1, 5), Ok(()));

let mut frame = [0_u8; GCounter::<4>::ENCODED_LEN];
assert_eq!(local.encode(&mut frame), Ok(GCounter::<4>::ENCODED_LEN));

// `decode_into` reads a frame over a state the receiver already holds.
let mut remote = GCounter::<4>::new();
assert_eq!(remote.decode_into(&frame), Ok(()));
assert_eq!(remote.value(), 5);
```

`encode` writes into a caller's buffer and `encoded` allocates one, which needs
the `alloc` feature. Decoding refuses a frame shorter than `ENCODED_LEN`
(`Truncated`), one written by another structure (`WrongStructure`), one written
at another shape (`WrongShape`), and one no sequence of calls produces
(`Malformed`: more records than slots, a replica, element or tag held twice, a
zero count, a tombstone for an add the frame does not hold).

```rust
use merge_machines::{GCounter, Refusal};

let mut wide = [0_u8; GCounter::<8>::ENCODED_LEN];
assert_eq!(GCounter::<8>::new().encode(&mut wide), Ok(wide.len()));

assert_eq!(GCounter::<4>::decode(&wide), Err(Refusal::WrongShape));
```

Elements, values and stamps on a frame are 64-bit: the sets encode
`GSet<u64, N>` and `OrSet<u64, N>`, and the register encodes
`LwwRegister<u64, S>` for a stamp `S` of either `u64` or `HybridTimestamp`.

## License

MIT.
