# MergeMachines

Conflict-free replicated data types for Ruby, and the clocks that stamp them.

Two processes that both edited the same thing have to agree on the result
without asking each other. These states merge commutatively, associatively and
idempotently, so a replica converges on the same value whatever order the
updates arrived in and however many times one of them arrived.

```ruby
require 'merge_machines'

edge = MergeMachines::GCounter.new(replicas: 8)
origin = MergeMachines::GCounter.new(replicas: 8)

edge.increment(replica: 1, by: 3)
origin.increment(replica: 2, by: 7)

edge.merge(origin).value   # => 10
edge.merge(origin).value   # => 10, the same state again is absorbed
```

Nothing here talks to a network. The caller owns the transport, the replica ids
and every clock reading.

## The native extension is required

There is no pure-Ruby fallback and no `DISABLE_*` switch. The gem is a binding
to the `merge-machines` crate in `ext/merge_machines_native/core`, and it needs
Ruby 4.0 and a Rust toolchain (or a platform gem). A second implementation
would mean two sets of convergence tests and two byte formats to keep in step.

## Sizes are a fixed menu

The crate sizes every state by a const generic, so each size is a separately
compiled type. The extension carries these and raises `ArgumentError` for
anything else:

| Class         | Keyword     | Values           |
| ------------- | ----------- | ---------------- |
| `GCounter`    | `replicas:` | 4, 8, 16, 32     |
| `PNCounter`   | `replicas:` | 4, 8, 16, 32     |
| `GSet`        | `capacity:` | 64, 256, 1024    |
| `OrSet`       | `capacity:` | 64, 256, 1024    |
| `LwwRegister` | `stamp:`    | `:logical`, `:hybrid` |

```ruby
MergeMachines::GSet.new(capacity: 100)
# ArgumentError: capacity 100 is not supported, expected one of [64, 256, 1024]
```

A state with no free slot left raises `MergeMachines::Full` rather than
growing. That is the deal a fixed size buys: a replica's memory is known at
construction time.

## Counters

```ruby
counter = MergeMachines::GCounter.new(replicas: 8)
counter.increment(replica: 1, by: 3)
counter.increment(replica: 1)        # by: defaults to 1
counter.count(replica: 1)            # => 4
counter.value                        # => 4, every replica's count
counter.merge(other)                 # exact union, idempotent
```

One shared number cannot be merged: two replicas that both saw 5 and both added
1 have no way to tell 6 from 7. One count per replica can, because no replica
ever writes another's count — which is why `increment` needs a `replica:`.

`PNCounter` is two of those, one for what went up and one for what came down,
so its value may be negative.

```ruby
counter = MergeMachines::PNCounter.new(replicas: 8)
counter.increment(replica: 1, by: 10)
counter.decrement(replica: 2, by: 4)
counter.value     # => 6
counter.added     # => 10
counter.removed   # => 4
```

## Sets

Keys are strings and must be UTF-8. The crate's elements are `Copy + Eq`, so a
key crosses as a 64-bit hash of its bytes and the extension keeps the string
each hash came from; `dump` carries that table along, so `load` hands the real
strings back.

```ruby
set = MergeMachines::GSet.new(capacity: 64)
set.add('user-1')
set.include?('user-1')   # => true
set.members              # => ['user-1']
set.merge(other)         # union
```

`GSet` only grows, so merging is the union and there is nothing to resolve.
`OrSet` tags every add and tombstones the tags a remove has seen, so a remove
races a concurrent add and loses, and re-adding afterwards works.

```ruby
set = MergeMachines::OrSet.new(capacity: 64)
set.add('user-1', replica: 1)
set.remove('user-1')
set.include?('user-1')   # => false

set.add('user-1', replica: 1)
set.include?('user-1')   # => true
```

`capacity:` bounds the adds and the tombstones separately. A removed key keeps
its add slot, so a set that churns runs out.

## One value, last writer wins

Stamps order the writes and the replica id breaks the ties. Both sides of a
concurrent write pick the same winner, which is the whole point, and the
loser's value is gone, which is the price.

```ruby
register = MergeMachines::LwwRegister.new
register.set('http://origin', replica: 1, stamp: 7)
register.value      # => 'http://origin'
register.stamp      # => 7
register.replica    # => 1
register.merge(other)
```

A register nobody has written holds `nil`. `stamp:` is whichever clock you
picked at construction: an Integer for `:logical`, a `Timestamp` for
`:hybrid`.

## Which clock

`LogicalClock` is Lamport's counter. It advances on every local event and jumps
past any stamp it is shown, so a receive is always stamped after the send it
answers. No clock is read.

```ruby
clock = MergeMachines::LogicalClock.new(replica: 1)
sent_at = clock.tick       # => 1
clock.observe(42)          # => 43, past both the local history and the send
clock.now                  # => 43
```

A counter with nothing left raises `MergeMachines::Exhausted` rather than
handing out a stamp it already gave away, and the clock stays where it was.

`HybridClock` pairs a wall-clock reading with a counter: close to physical
time, still totally ordered, and still monotonic when the host's clock is
corrected backwards.

```ruby
clock = MergeMachines::HybridClock.new(replica: 1)
clock.tick                              # Timestamp, reading from the realtime clock
clock.tick(wall: 1_700_000_000.5)       # or a reading you supply, in seconds
clock.observe(wall: 1.5, counter: 3)    # a timestamp that arrived
clock.now                               # the last one issued
```

### The reading it wants is not a monotonic one

`wall:` is physical time two machines can compare, and defaults to
`Process.clock_gettime(Process::CLOCK_REALTIME, :nanosecond)`. That clock
**steps backwards** when the host's is corrected, which is exactly why the
counter exists.

**No deadline, timeout or budget may be derived from it.** Measuring how long
something took belongs to the monotonic clock, which this gem neither reads nor
models. `deadline_machines` owns that.

A reading crosses this boundary as seconds in a Float, so a realtime reading
round-trips to about 256 nanoseconds. Two timestamps that land inside the same
window are separated by the counter, which is what it is for.

## Putting state on a wire

`dump` returns a binary string and `load` reads one back.

```ruby
blob = set.dump                    # String, Encoding::BINARY
MergeMachines::GSet.load(blob)
```

A blob is the crate's own fixed-length frame, led by a tag byte naming the
structure and a field naming the shape, followed by whatever the Ruby surface
needs that the frame does not hold: the key table for the sets and the
register, the replica id for the clocks. `load` raises `ArgumentError` on a
truncated blob, on one written by another structure, and on one written at a
shape this build does not carry.

```ruby
MergeMachines::GSet.load(MergeMachines::OrSet.new(capacity: 64).dump)
# ArgumentError: codec: wrong structure tag
```

## What this gem is not

- It is not a transport. There is no gossip, no anti-entropy loop, no peer
  list. You ship the blobs.
- It is not a timer. The hybrid clock reads a wall clock, never a monotonic
  one, and nothing here measures elapsed time.
- It is not a state machine, and it does not retry, back off, break a circuit
  or shed load. Other machines in this family do those.

## License

MIT.
