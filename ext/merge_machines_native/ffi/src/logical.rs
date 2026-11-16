//! `MergeMachines::LogicalClock`: ordering events with no clock at all.

use std::cell::RefCell;

use magnus::typed_data::Obj;
use magnus::{Error, IntoValue, RString, Ruby, Value, prelude::*};
use matryona_machines::ruby::{Class, Registry, integer};
use merge_machines::{LogicalClock, Refusal};

use crate::{frame_bytes, refusal, refused_frame};

/// `MergeMachines::LogicalClock`, pinned when [`define`] creates it.
static CLASS: Registry<Class> = Registry::new("merge_machines");

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
    crate::exact!("MergeMachines::LogicalClock");
    crate::eq!();

    const fn new() -> Self {
        Self(RefCell::new(LogicalClock::new()))
    }

    /// `LogicalClock::decode`.
    fn decode(ruby: &Ruby, frame: Value) -> Result<Self, Error> {
        let frame = frame_bytes(frame)?;
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
    fn observe(ruby: &Ruby, rb_self: Obj<Self>, observed: Value) -> Result<Value, Error> {
        rb_self.check_frozen()?;
        let observed: u64 = integer(observed, "observed")?;
        let outcome = rb_self.0.borrow_mut().observe(observed);
        Ok(stamped(ruby, outcome))
    }

    /// `LogicalClock::encoded`.
    fn encoded(ruby: &Ruby, rb_self: &Self) -> RString {
        ruby.str_from_slice(&rb_self.0.borrow().encoded())
    }

    fn inspect(&self) -> String {
        format!("#<MergeMachines::LogicalClock now={}>", self.now())
    }
}

crate::define!("LogicalClock", new(0) {
    "now" => now(0),
    "tick" => tick(0),
    "observe" => observe(1),
});
