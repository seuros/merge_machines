//! `MergeMachines::LogicalClock`: ordering events with no clock at all.

use std::cell::RefCell;

use magnus::typed_data::Obj;
use magnus::{Error, IntoValue, RModule, RString, Ruby, Value, function, method, prelude::*};
use merge_machines::{LogicalClock, Refusal};

use crate::{Whole, refusal, refused_frame};

#[magnus::wrap(class = "MergeMachines::LogicalClock", free_immediately, size)]
struct Wrapper(RefCell<LogicalClock>);

/// The stamp issued, or the refusal's symbol.
fn stamped(ruby: &Ruby, outcome: Result<u64, Refusal>) -> Value {
    match outcome {
        Ok(stamp) => stamp.into_value_with(ruby),
        Err(refused) => refusal(ruby, refused).into_value_with(ruby),
    }
}

impl Wrapper {
    const fn new() -> Self {
        Self(RefCell::new(LogicalClock::new()))
    }

    /// `LogicalClock::decode`.
    fn decode(ruby: &Ruby, frame: RString) -> Result<Self, Error> {
        let frame = frame.to_bytes();
        LogicalClock::decode(&frame)
            .map(|clock| Self(RefCell::new(clock)))
            .map_err(|refused| refused_frame(ruby, refused))
    }

    /// The last stamp this clock issued.
    fn now(&self) -> u64 {
        self.0.borrow().now()
    }

    /// The stamp issued, or `:exhausted`.
    fn tick(ruby: &Ruby, rb_self: Obj<Self>) -> Result<Value, Error> {
        rb_self.check_frozen()?;
        let outcome = rb_self.0.borrow_mut().tick();
        Ok(stamped(ruby, outcome))
    }

    /// The stamp issued, or `:exhausted`.
    fn observe(
        ruby: &Ruby,
        rb_self: Obj<Self>,
        Whole(observed): Whole<u64>,
    ) -> Result<Value, Error> {
        rb_self.check_frozen()?;
        let outcome = rb_self.0.borrow_mut().observe(observed);
        Ok(stamped(ruby, outcome))
    }

    fn eq(&self, other: Value) -> bool {
        <&Self>::try_convert(other).is_ok_and(|other| *self.0.borrow() == *other.0.borrow())
    }

    /// `LogicalClock::encoded`.
    fn encoded(ruby: &Ruby, rb_self: &Self) -> RString {
        ruby.str_from_slice(&rb_self.0.borrow().encoded())
    }

    fn inspect(&self) -> String {
        format!("#<MergeMachines::LogicalClock now={}>", self.now())
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = module.define_class("LogicalClock", ruby.class_object())?;
    class.undef_default_alloc_func();
    class.define_singleton_method("new", function!(Wrapper::new, 0))?;
    class.define_singleton_method("decode", function!(Wrapper::decode, 1))?;
    class.define_method("now", method!(Wrapper::now, 0))?;
    class.define_method("tick", method!(Wrapper::tick, 0))?;
    class.define_method("observe", method!(Wrapper::observe, 1))?;
    class.define_method("==", method!(Wrapper::eq, 1))?;
    class.define_method("encoded", method!(Wrapper::encoded, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
