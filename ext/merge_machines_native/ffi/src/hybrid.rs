//! `MergeMachines::HybridClock`: timestamps near physical time that still
//! order.

use std::cell::RefCell;

use bytes::Bytes;
use magnus::scan_args::{get_kwargs, scan_args};
use magnus::{Error, RHash, RModule, RString, Ruby, Value, function, method, prelude::*};
use merge_machines::{HybridClock, ReplicaId};

use crate::data::{stamped, timestamp};
use crate::{codec_error, exhausted, optional_kwarg, required_kwarg, trailing_replica, wall};

/// A clock and the replica whose events it stamps, which the core does not
/// hold.
#[magnus::wrap(class = "MergeMachines::HybridClock", free_immediately, size)]
struct Wrapper {
    clock: RefCell<HybridClock>,
    replica: ReplicaId,
}

impl Wrapper {
    fn new(args: &[Value]) -> Result<Self, Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        Ok(Self {
            clock: RefCell::new(HybridClock::new()),
            replica: required_kwarg(args.keywords, "replica")?,
        })
    }

    /// Reads a blob written by [`dump`](Self::dump).
    fn load(ruby: &Ruby, blob: Bytes) -> Result<Self, Error> {
        let mut clock = HybridClock::new();
        clock
            .decode_into(&blob)
            .map_err(|error| codec_error(ruby, error))?;

        Ok(Self {
            clock: RefCell::new(clock),
            replica: trailing_replica(ruby, &blob, HybridClock::ENCODED_LEN)?,
        })
    }

    const fn replica(&self) -> ReplicaId {
        self.replica
    }

    /// The last timestamp this clock issued.
    fn now(ruby: &Ruby, rb_self: &Self) -> Result<Value, Error> {
        timestamp(ruby, rb_self.clock.borrow().last())
    }

    /// Stamps a local event at `wall_ns:`, defaulting to the realtime clock.
    fn tick(ruby: &Ruby, rb_self: &Self, args: &[Value]) -> Result<Value, Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let wall_ns = match optional_kwarg::<u64>(args.keywords, "wall_ns")? {
            Some(nanoseconds) => nanoseconds,
            None => wall::now_ns(ruby)?,
        };
        let issued = rb_self.clock.borrow_mut().tick(wall_ns);
        timestamp(ruby, exhausted(ruby, issued)?)
    }

    /// Stamps the receipt of the timestamp `wall_ns:` and `counter:` name, at
    /// the realtime clock's reading.
    fn observe(ruby: &Ruby, rb_self: &Self, args: &[Value]) -> Result<Value, Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let (wall_ns, counter) =
            get_kwargs::<_, (u64, u64), (), ()>(args.keywords, &["wall_ns", "counter"], &[])?
                .required;
        let now = wall::now_ns(ruby)?;
        let observed = stamped(wall_ns, counter);
        let issued = rb_self.clock.borrow_mut().observe(now, observed);
        timestamp(ruby, exhausted(ruby, issued)?)
    }

    /// The core frame followed by the replica id, 64-bit little-endian.
    fn dump(ruby: &Ruby, rb_self: &Self) -> RString {
        let mut blob = Vec::from(rb_self.clock.borrow().encoded());
        blob.extend_from_slice(&rb_self.replica.to_le_bytes());
        ruby.str_from_slice(&blob)
    }

    fn inspect(&self) -> String {
        let last = self.clock.borrow().last();
        format!(
            "#<MergeMachines::HybridClock replica={} wall_ns={} counter={}>",
            self.replica, last.wall_ns, last.counter
        )
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = module.define_class("HybridClock", ruby.class_object())?;
    class.define_singleton_method("new", function!(Wrapper::new, -1))?;
    class.define_singleton_method("load", function!(Wrapper::load, 1))?;
    class.define_method("replica", method!(Wrapper::replica, 0))?;
    class.define_method("now", method!(Wrapper::now, 0))?;
    class.define_method("tick", method!(Wrapper::tick, -1))?;
    class.define_method("observe", method!(Wrapper::observe, -1))?;
    class.define_method("dump", method!(Wrapper::dump, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
