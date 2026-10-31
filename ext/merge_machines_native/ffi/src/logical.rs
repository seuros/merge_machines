//! `MergeMachines::LogicalClock`: ordering events with no clock at all.

use std::cell::RefCell;

use bytes::Bytes;
use magnus::scan_args::scan_args;
use magnus::{Error, RHash, RModule, RString, Ruby, Value, function, method, prelude::*};
use merge_machines::{LogicalClock, ReplicaId};

use crate::{codec_error, exhausted, required_kwarg, trailing_replica};

/// A clock and the replica whose events it stamps, which the core does not
/// hold.
#[magnus::wrap(class = "MergeMachines::LogicalClock", free_immediately, size)]
struct Wrapper {
    clock: RefCell<LogicalClock>,
    replica: ReplicaId,
}

impl Wrapper {
    fn new(args: &[Value]) -> Result<Self, Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        Ok(Self {
            clock: RefCell::new(LogicalClock::new()),
            replica: required_kwarg(args.keywords, "replica")?,
        })
    }

    /// Reads a blob written by [`dump`](Self::dump).
    fn load(ruby: &Ruby, blob: Bytes) -> Result<Self, Error> {
        let mut clock = LogicalClock::new();
        clock
            .decode_into(&blob)
            .map_err(|error| codec_error(ruby, error))?;

        Ok(Self {
            clock: RefCell::new(clock),
            replica: trailing_replica(ruby, &blob, LogicalClock::ENCODED_LEN)?,
        })
    }

    const fn replica(&self) -> ReplicaId {
        self.replica
    }

    /// The last stamp this clock issued.
    fn now(&self) -> u64 {
        self.clock.borrow().now()
    }

    /// Stamps a local event.
    fn tick(ruby: &Ruby, rb_self: &Self) -> Result<u64, Error> {
        exhausted(ruby, rb_self.clock.borrow_mut().tick())
    }

    /// Stamps the receipt of `observed`, a stamp from another replica.
    fn observe(ruby: &Ruby, rb_self: &Self, observed: u64) -> Result<u64, Error> {
        exhausted(ruby, rb_self.clock.borrow_mut().observe(observed))
    }

    /// The core frame followed by the replica id, 64-bit little-endian.
    fn dump(ruby: &Ruby, rb_self: &Self) -> RString {
        let mut blob = Vec::from(rb_self.clock.borrow().encoded());
        blob.extend_from_slice(&rb_self.replica.to_le_bytes());
        ruby.str_from_slice(&blob)
    }

    fn inspect(&self) -> String {
        format!(
            "#<MergeMachines::LogicalClock replica={} now={}>",
            self.replica,
            self.now()
        )
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = module.define_class("LogicalClock", ruby.class_object())?;
    class.define_singleton_method("new", function!(Wrapper::new, -1))?;
    class.define_singleton_method("load", function!(Wrapper::load, 1))?;
    class.define_method("replica", method!(Wrapper::replica, 0))?;
    class.define_method("now", method!(Wrapper::now, 0))?;
    class.define_method("tick", method!(Wrapper::tick, 0))?;
    class.define_method("observe", method!(Wrapper::observe, 1))?;
    class.define_method("dump", method!(Wrapper::dump, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
