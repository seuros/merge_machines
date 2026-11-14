//! `MergeMachines::HybridClock`: timestamps near physical time that still
//! order. The caller passes every wall-clock reading; the binding reads none.

use std::cell::RefCell;

use magnus::typed_data::Obj;
use magnus::{Error, IntoValue, RModule, RString, Ruby, Value, function, method, prelude::*};
use merge_machines::{HybridClock, HybridTimestamp, Refusal};

use crate::data::{self, classes, timestamp_from};
use crate::{Whole, on_copy, refusal, refused_frame};

#[magnus::wrap(class = "MergeMachines::HybridClock", free_immediately, size)]
struct Wrapper(RefCell<HybridClock>);

/// The `Timestamp` issued, or the refusal's symbol. `classes` was looked up
/// before the core was called, and the clock is a copy [`on_copy`] keeps only
/// once the value is built.
fn stamped(
    ruby: &Ruby,
    classes: data::Classes,
    outcome: Result<HybridTimestamp, Refusal>,
) -> Result<Value, Error> {
    match outcome {
        Ok(stamp) => data::timestamp(classes, stamp),
        Err(refused) => Ok(refusal(ruby, refused).into_value_with(ruby)),
    }
}

impl Wrapper {
    const fn new() -> Self {
        Self(RefCell::new(HybridClock::new()))
    }

    /// `HybridClock::decode`.
    fn decode(ruby: &Ruby, frame: RString) -> Result<Self, Error> {
        let frame = frame.to_bytes();
        HybridClock::decode(&frame)
            .map(|clock| Self(RefCell::new(clock)))
            .map_err(|refused| refused_frame(ruby, refused))
    }

    /// The last timestamp this clock issued.
    fn last(ruby: &Ruby, rb_self: &Self) -> Result<Value, Error> {
        data::timestamp(classes(ruby)?, rb_self.0.borrow().last())
    }

    /// Stamps a local event at `wall_ns`: the `Timestamp` issued, or
    /// `:exhausted`.
    fn tick(ruby: &Ruby, rb_self: Obj<Self>, Whole(wall_ns): Whole<u64>) -> Result<Value, Error> {
        rb_self.check_frozen()?;
        let classes = classes(ruby)?;
        on_copy(ruby, rb_self, &rb_self.0, |clock| {
            stamped(ruby, classes, clock.tick(wall_ns))
        })
    }

    /// Stamps the receipt of `observed` at `wall_ns`: the `Timestamp` issued,
    /// or `:exhausted`.
    fn observe(
        ruby: &Ruby,
        rb_self: Obj<Self>,
        Whole(wall_ns): Whole<u64>,
        observed: Value,
    ) -> Result<Value, Error> {
        rb_self.check_frozen()?;
        let classes = classes(ruby)?;
        let observed = timestamp_from(ruby, observed)?;
        on_copy(ruby, rb_self, &rb_self.0, |clock| {
            stamped(ruby, classes, clock.observe(wall_ns, observed))
        })
    }

    fn eq(&self, other: Value) -> bool {
        <&Self>::try_convert(other).is_ok_and(|other| *self.0.borrow() == *other.0.borrow())
    }

    /// `HybridClock::encoded`.
    fn encoded(ruby: &Ruby, rb_self: &Self) -> RString {
        ruby.str_from_slice(&rb_self.0.borrow().encoded())
    }

    fn inspect(&self) -> String {
        let last = self.0.borrow().last();
        format!(
            "#<MergeMachines::HybridClock wall_ns={} counter={}>",
            last.wall_ns, last.counter
        )
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = module.define_class("HybridClock", ruby.class_object())?;
    class.undef_default_alloc_func();
    class.define_singleton_method("new", function!(Wrapper::new, 0))?;
    class.define_singleton_method("decode", function!(Wrapper::decode, 1))?;
    class.define_method("last", method!(Wrapper::last, 0))?;
    class.define_method("tick", method!(Wrapper::tick, 1))?;
    class.define_method("observe", method!(Wrapper::observe, 2))?;
    class.define_method("==", method!(Wrapper::eq, 1))?;
    class.define_method("encoded", method!(Wrapper::encoded, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
