# MergeMachines 🔀

> **Two ships, one log, no argument.** The tender that lets separated crews write
> their own records and still agree when the fleet regroups.

Conflict-free replicated data types for Ruby, and the clocks that stamp them.

Two processes that both edited the same thing have to agree on the result
without asking each other. These states merge commutatively, associatively and
idempotently, so a replica converges on the same value whatever order the
updates arrived in and however many times one of them arrived. **AMS Lamport**
runs under comms blackout by design: it never calls home, it reconciles on
contact.

---

## 🚀 Launch Sequence

```ruby
require 'merge_machines'

edge = MergeMachines::GCounter.new(8)
origin = MergeMachines::GCounter.new(8)

edge.increment(1, 3)     # => :incremented
origin.increment(2, 7)   # => :incremented

edge.merge(origin)       # => :merged
edge.value               # => 10
edge.merge(origin)       # => :merged, the same state again is absorbed
edge.value               # => 10
```

Nothing here talks to a network and nothing here reads a clock. The caller owns
the transport, the replica ids, the tags and every clock reading.

---

## 🔩 The Native Extension Is Required

There is no pure-Ruby fallback and no `DISABLE_*` switch. The gem is a thin
binding to the `merge-machines` crate in `ext/merge_machines_native/core`, and
it needs Ruby 4.0 and a Rust toolchain (or a platform gem). Every class and
method is named after the crate's; every value is what the crate holds:
elements, values, replica ids, tags and stamps are unsigned 64-bit Integers. A
non-Integer raises `TypeError`, an Integer outside `0..2**64-1` raises
`RangeError`. Want strings in a set? Hash them yourself, and keep your own
table if you need them back.

---

## 📐 Sizes Are a Fixed Menu

The crate sizes every state by a const generic, so each size is a separately
compiled type. The extension carries these and raises `RangeError` for anything
else:

| Class       | Argument   | Values        |
| ----------- | ---------- | ------------- |
| `GCounter`  | `replicas` | 4, 8, 16, 32  |
| `PNCounter` | `replicas` | 4, 8, 16, 32  |
| `GSet`      | `capacity` | 64, 256, 1024 |
| `OrSet`     | `capacity` | 64, 256, 1024 |

```ruby
MergeMachines::GSet.new(100)
# RangeError: capacity 100 is not supported, expected one of [64, 256, 1024]
```

A state with no free slot left answers `:full` rather than growing. That is
the deal a fixed size buys: a replica's memory is known at construction time.
Two states of different sizes are different types: merging them is a
`TypeError`.

---

## 🔢 Counters

```ruby
counter = MergeMachines::GCounter.new(8)
counter.increment(1, 3)   # => :incremented
counter.count(1)          # => 3
counter.value             # => 3, every replica's count
counter.entries           # => [#<data MergeMachines::Entry replica=1, count=3>]
counter.merge(other)      # => :merged, or :full and nothing moved
```

One shared number cannot be merged: two replicas that both saw 5 and both added
1 have no way to tell 6 from 7. One count per replica can, because no replica
ever writes another's count, which is why `increment` needs a replica. A count
that would pass `2**64 - 1` is `:overflow`; adding zero is honoured and claims
no slot.

`PNCounter` is two of those, one for what went up and one for what came down,
so its value may be negative.

```ruby
counter = MergeMachines::PNCounter.new(8)
counter.increment(1, 10)   # => :incremented
counter.decrement(2, 4)    # => :decremented
counter.value              # => 6
counter.added              # => 10
counter.removed            # => 4
```

---

## 🗂️ Sets

```ruby
set = MergeMachines::GSet.new(64)
set.insert(42)        # => :inserted
set.insert(42)        # => :duplicate, nothing changed
set.include?(42)      # => true
set.to_a              # => [42]
set.merge(other)      # => :merged, or :full and nothing moved
```

`GSet` only grows, so merging is the union and there is nothing to resolve.
`OrSet` tags every add and tombstones the tags a remove has seen, so a remove
races a concurrent add and loses, and re-adding afterwards works. You pick the
tags: the replica making the add and a counter of its own that it never reuses.

```ruby
include MergeMachines

set = OrSet.new(64)
set.add(42, Tag.new(1, 1))    # => :added
set.add(42, Tag.new(1, 1))    # => :duplicate
set.add(7, Tag.new(1, 1))     # => :mismatch, that tag already added 42
set.remove(42)                # => :removed
set.remove(42)                # => :not_held
set.add(42, Tag.new(1, 2))    # => :added, back again
set.adds                      # => every Tagged add seen, tombstoned or not
set.removes                   # => [#<data MergeMachines::Tag replica=1, counter=1>]
```

A tombstone lives on the add it removes, so `capacity` bounds the adds and a
remove never needs a slot. A removed element keeps its add slot, so a set that
churns runs out.

---

## 🏁 One Value, Last Writer Wins

Stamps order the writes and the replica id breaks the ties. Both sides of a
concurrent write pick the same winner, which is the whole point, and the
loser's value is gone, which is the price.

```ruby
register = MergeMachines::LwwRegister.new(1, 7, 1)   # value, stamp, replica
register.set(2, 9, 1)     # => :written
register.set(3, 8, 4)     # => :superseded, a later write is held
register.set(2, 9, 1)     # => :duplicate
register.set(5, 9, 1)     # => :mismatch, one stamp and replica, two values
register.value            # => 2
register.merge(other)     # => :merged, or :mismatch and nothing moved
```

`:superseded` is the register working, not a refusal. The stamp's class picks
the crate's register: an Integer for a logical stamp, a
`MergeMachines::Timestamp` for a hybrid one. A stamp or register of the other
kind is a `TypeError`.

---

## 🕰️ Which Chronometer

`LogicalClock` is Lamport's counter. It advances on every local event and jumps
past any stamp it is shown, so a receive is always stamped after the send it
answers. No clock is read.

```ruby
clock = MergeMachines::LogicalClock.new
sent_at = clock.tick       # => 1
clock.observe(42)          # => 43, past both the local history and the send
clock.now                  # => 43
```

A counter with nothing left answers `:exhausted` rather than handing out a stamp
it already gave away, and the clock stays where it was.

`HybridClock` pairs a wall-clock reading with a counter: close to physical
time, still totally ordered, and still monotonic when the host's clock is
corrected backwards. You pass the reading; the gem reads no clock.

```ruby
now = Process.clock_gettime(Process::CLOCK_REALTIME, :nanosecond)
clock = MergeMachines::HybridClock.new
clock.tick(now)                                          # => Timestamp
clock.observe(now, MergeMachines::Timestamp.new(now, 3)) # a timestamp that arrived
clock.last                                               # the last one issued
```

It answers `:exhausted` too, when the reading has not advanced and the counter
that separates two events at one reading has nothing left. A reading that
really did advance resets that counter.

Neither clock knows a replica: a stamp is a protocol number every peer
compares, not a handle, and which replica made it is yours to carry.

### ⚠️ The Reading It Wants Is Not a Monotonic One

`wall_ns` is physical time two machines can compare, in Integer nanoseconds
since the Unix epoch:
`Process.clock_gettime(Process::CLOCK_REALTIME, :nanosecond)`. That clock
**steps backwards** when the host's is corrected, which is exactly why the
counter exists. It is an event-time stamp, so it stays an Integer, never a
span or an instant on the monotonic clock.

**No deadline, timeout or budget may be derived from it.** Measuring how long
something took belongs to the monotonic clock, which this gem neither reads nor
models. `deadline_machines` owns that, and **AMS Layland** is the ship you hail
for it.

---

## 📦 Putting State on a Wire

`encoded` returns the crate's own fixed-length frame as a binary string and
`decode` reads one back, at the shape the frame names.

```ruby
frame = set.encoded                # String, Encoding::BINARY
MergeMachines::GSet.decode(frame)
```

A frame is led by a tag byte naming the structure and a field naming the shape.
`decode` raises `RangeError` for a frame the crate refuses, and the message
names the refusal: `truncated`, `wrong_structure`, `wrong_shape`, or
`malformed` for a frame no sequence of calls produces (more records than slots,
a replica, element or tag held twice, a zero count, a tombstone for an add the
frame does not hold). A frame at a shape this build does not carry is a
`RangeError` too.

```ruby
MergeMachines::GSet.decode(MergeMachines::OrSet.new(64).encoded)
# RangeError: the frame is refused: wrong_structure
```

---

## 📜 The Contract

- **A refusal is a symbol and changes nothing.** Never an exception. A merge is
  checked whole before anything moves, so it lands completely or not at all.
- **A repeat is honoured by name and changes nothing:** `:duplicate` from
  `GSet#insert`, `OrSet#add` and `LwwRegister#set`, `:not_held` from
  `OrSet#remove`, `:merged` for a state already seen. A repeat with a different
  payload is `:mismatch`.
- **Malformed input raises:** `TypeError` for the wrong class, `RangeError` for
  an Integer out of range, an unsupported size, or a frame `decode` refuses.
  `ArgumentError` only for the wrong number of arguments.
- **Values are checked at construction.** `Timestamp`, `Entry`, `Tag` and
  `Tagged` go through the crate's constructors whether you build them or the
  gem does. They are frozen, sealed `Data`: subclassing one raises `TypeError`
  and `allocate` is undefined. Sealing stops accidents, not reflection (a named
  subclass's constant is bound before the raise, `Marshal.load` skips
  `initialize`), so the binding is the authority: every value it reads must be
  an instance of exactly its class and is checked again, raising `TypeError` or
  `RangeError` before the crate sees it.
- **A frozen state refuses to change.** Every mutating method raises
  `FrozenError` on a frozen receiver, before anything moves.
- **The binding never loses work.** Every argument is read before the crate is
  called. `HybridClock#tick` and `#observe` stamp a copy of the clock and keep
  it only once the `Timestamp` is built: a build that raises leaves the clock
  unmoved, one that freezes the clock raises `FrozenError`, and one that changes
  the clock meanwhile (a redefined `Timestamp.new` calling back in) raises
  `RuntimeError`; in each case the outer stamp is not applied.
- **No scopes, no restarts to resume.** Nothing here issues handles. Replica ids
  and tags are yours; clock stamps are protocol numbers. `encoded`/`decode` is
  the whole of persistence.
- **No answer records a monotonic clock**, because none is taken. The hybrid
  clock folds your `wall_ns` into the stamp it issues; a refused stamp folds
  nothing.

### Every answer

| Call | Honoured | Refusals |
| --- | --- | --- |
| `GCounter#increment(replica, by)` | `:incremented` | `:full`, `:overflow` |
| `GCounter#merge(other)` | `:merged` | `:full` |
| `PNCounter#increment(replica, by)` | `:incremented` | `:full`, `:overflow` |
| `PNCounter#decrement(replica, by)` | `:decremented` | `:full`, `:overflow` |
| `PNCounter#merge(other)` | `:merged` | `:full` |
| `GSet#insert(element)` | `:inserted`, `:duplicate` (*repeat*) | `:full` |
| `GSet#merge(other)` | `:merged` | `:full` |
| `OrSet#add(element, tag)` | `:added`, `:duplicate` (*repeat*) | `:full`, `:mismatch` |
| `OrSet#remove(element)` | `:removed`, `:not_held` (*repeat*) | none |
| `OrSet#merge(other)` | `:merged` | `:full`, `:mismatch` |
| `LwwRegister#set(value, stamp, replica)` | `:written`, `:superseded`, `:duplicate` (*repeat*) | `:mismatch` |
| `LwwRegister#merge(other)` | `:merged` | `:mismatch` |
| `LogicalClock#tick` | the stamp issued (Integer) | `:exhausted` |
| `LogicalClock#observe(observed)` | the stamp issued (Integer) | `:exhausted` |
| `HybridClock#tick(wall_ns)` | the `Timestamp` issued | `:exhausted` |
| `HybridClock#observe(wall_ns, observed)` | the `Timestamp` issued | `:exhausted` |
| `.decode(frame)` on every class | a new state | raises `RangeError` |

Which answers record the clock: none takes a monotonic `now`. Only
`HybridClock#tick` and `#observe` take a reading, and their honoured answer is
the stamp built from it; `:exhausted` records nothing.

| Refusal | Meaning |
| --- | --- |
| `:full` | The call needs a slot and the fixed size has none left. |
| `:overflow` | A replica's count would pass `2**64 - 1`. |
| `:exhausted` | A clock has issued the last stamp its counter can hold. |
| `:mismatch` | One tag, or one stamp and replica, already names another value. |
| `:truncated`, `:wrong_structure`, `:wrong_shape`, `:malformed` | Only in a `decode` `RangeError` message: the frame is short, holds another structure, was written at another shape, or describes a state no calls produce. |

---

## 🚫 Rules of Engagement

- It is not a transport. There is no gossip, no anti-entropy loop, no peer
  list. You ship the blobs.
- It is not a timer. The hybrid clock takes a wall-clock reading you pass,
  never a monotonic one, and nothing here measures elapsed time.
- It is not a state machine, and it does not retry, back off, break a circuit
  or shed load. Other machines in this family do those.

---

## 📡 Credits

**AMS Lamport**, NCC-1178, *Lamport*-class tender.

Named for Leslie Lamport's *Time, Clocks, and the Ordering of Events in a
Distributed System* (1978), which showed that order between separated machines
is something you stamp, not something you read off a clock.

---

## License

MIT.
