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
is shown, so a receive is always stamped after the send it answers. A counter
with nothing left reports `ClockExhausted` instead of handing out a timestamp it
already gave away.

```rust
use merge_machines::{ClockExhausted, LogicalClock};

let mut sender = LogicalClock::new();
let mut receiver = LogicalClock::new();

let sent_at = sender.tick()?;
receiver.tick()?;

assert!(receiver.observe(sent_at)? > sent_at);
assert_eq!(receiver.observe(u64::MAX), Err(ClockExhausted));

Ok::<(), ClockExhausted>(())
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

A counter with nothing left reports `ClockExhausted` here too, rather than
handing out a timestamp it already gave away.

```rust
use merge_machines::{ClockExhausted, HybridClock, HybridTimestamp};

const MS: u64 = 1_000_000;
let mut clock = HybridClock::new();

let before = clock.tick(1_000 * MS)?;
let during = clock.tick(1_000 * MS)?;
let after = clock.tick(400 * MS)?;

assert!(during > before);
assert!(after > during);
assert_eq!(after.wall_ns, 1_000 * MS);

let crowded = HybridTimestamp { wall_ns: 1_000 * MS, counter: u64::MAX };
assert_eq!(clock.observe(400 * MS, crowded), Err(ClockExhausted));
assert_eq!(clock.last(), after);

Ok::<(), ClockExhausted>(())
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

## Putting state on a wire

A state that cannot be sent cannot replicate. Each one encodes to a fixed-length
frame of its own `ENCODED_LEN` bytes, led by a tag naming the structure and,
where there is one, a field naming the const generic.

```rust
use merge_machines::{GCounter, Merge};

let mut local = GCounter::<4>::new();
assert_eq!(local.increment(1, 5), Ok(()));

let frame = local.encoded();
assert_eq!(frame.len(), GCounter::<4>::ENCODED_LEN);

// `decode_into` reads a frame over a state the receiver already holds.
let mut remote = GCounter::<4>::new();
assert_eq!(remote.decode_into(&frame), Ok(()));
assert_eq!(remote.value(), 5);
```

`encode` writes into a caller's buffer and `encoded` allocates one, which needs
the `alloc` feature. Decoding refuses a frame shorter than `ENCODED_LEN`, one
written by another structure, and one written at another shape.

```rust
use merge_machines::{CodecError, GCounter};

let wide = GCounter::<8>::new().encoded();

assert_eq!(GCounter::<4>::decode(&wide), Err(CodecError::Parameters));
```

Elements, values and stamps on a frame are 64-bit: the sets encode
`GSet<u64, N>` and `OrSet<u64, N>`, and the register encodes
`LwwRegister<u64, S>` for a stamp `S` of either `u64` or `HybridTimestamp`.

## License

MIT.
