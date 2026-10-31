# merge-machines

Conflict-free replicated data types and the clocks they need. `no_std`, zero
dependencies.

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
is shown, so a receive is always stamped after the send it answers.

```rust
use merge_machines::LogicalClock;

let mut sender = LogicalClock::new();
let mut receiver = LogicalClock::new();

let sent_at = sender.tick();
receiver.tick();

assert!(receiver.observe(sent_at) > sent_at);
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

```rust
use merge_machines::HybridClock;

const MS: u64 = 1_000_000;
let mut clock = HybridClock::new();

let before = clock.tick(1_000 * MS);
let during = clock.tick(1_000 * MS);
let after = clock.tick(400 * MS);

assert!(during > before);
assert!(after > during);
assert_eq!(after.wall_ns, 1_000 * MS);
```

## One value, last writer wins

Stamps order the writes; the replica id breaks the ties. Both sides of a
concurrent write pick the same winner, which is the whole point, and the loser's
value is gone, which is the price.

```rust
use merge_machines::{LwwRegister, Merge};

let mut left = LwwRegister::new(10_u32, 5_u64, 1);
let right = LwwRegister::new(20_u32, 5_u64, 2);

assert_eq!(left.merge(&right), Ok(()));
assert_eq!(*left.value(), 20);
```

## Sets, with and without removal

`GSet` only grows, so merging is the union and there is nothing to resolve.
`OrSet` tags every add and tombstones the tags a remove has seen, so a remove
races a concurrent add and loses, and re-adding afterwards works.

```rust
use merge_machines::{Merge, OrSet, Tag};

let mut shared = OrSet::<u16, 8>::new();
assert_eq!(shared.add(1, Tag::new(1, 1)), Ok(()));

let mut remover = shared;
let mut adder = shared;
assert_eq!(remover.remove(&1), Ok(()));
assert_eq!(adder.add(1, Tag::new(2, 1)), Ok(()));

assert_eq!(remover.merge(&adder), Ok(()));
assert!(remover.contains(&1));
```

## Capacity is a type parameter

Every state is sized at compile time: `GCounter<4>` holds four replicas,
`OrSet<T, 8>` eight adds and eight tombstones. A state with no free slot reports
`CapacityError` rather than allocating, which is why the examples above check
what `increment` and `add` returned.

```rust
use merge_machines::{CapacityError, GCounter};

let mut counter = GCounter::<1>::new();

assert_eq!(counter.increment(1, 1), Ok(()));
assert_eq!(counter.increment(1, 1), Ok(()));
assert_eq!(counter.increment(2, 1), Err(CapacityError));
```

## License

MIT.
