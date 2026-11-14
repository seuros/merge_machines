//! `MergeMachines::LwwRegister`: one unsigned 64-bit value, held by whichever
//! write was stamped last.
//!
//! The core has one register type per kind of stamp. Ruby has one class: a
//! register built with an Integer stamp is `LwwRegister<u64, u64>`, one built
//! with a `Timestamp` is `LwwRegister<u64, HybridTimestamp>`, and a stamp or a
//! register of the other kind is a `TypeError`.

use std::cell::RefCell;

use magnus::typed_data::Obj;
use magnus::{
    Error, Integer, IntoValue, RModule, RString, Ruby, Symbol, Value, function, method, prelude::*,
};
use matryona_machines::ruby::{Class, Message, Registry, bytes, exact_typed_with, integer};
use merge_machines::{
    HybridTimestamp, LwwRegister, Merge, Refusal, ReplicaId, STAMP_HYBRID, STAMP_LOGICAL, Writing,
};

use crate::data::{self, classes, timestamp_from};
use crate::{SHAPE, expected, expected_class, merged, refusal, refused_frame};

/// A register stamped by one of the two clocks.
#[derive(Clone, PartialEq, Eq)]
enum Register {
    Logical(LwwRegister<u64, u64>),
    Hybrid(LwwRegister<u64, HybridTimestamp>),
}

/// A stamp of one of the two kinds, read from Ruby.
enum Stamped {
    Logical(u64),
    Hybrid(HybridTimestamp),
}

/// An Integer stamp or a `MergeMachines::Timestamp`.
fn stamp_from(ruby: &Ruby, value: Value) -> Result<Stamped, Error> {
    if Integer::from_value(value).is_some() {
        return integer(value, "stamp").map(Stamped::Logical);
    }
    timestamp_from(ruby, value).map(Stamped::Hybrid)
}

/// `MergeMachines::LwwRegister`, pinned when [`define`] creates it.
static CLASS: Registry<Class> = Registry::new("merge_machines");

#[magnus::wrap(class = "MergeMachines::LwwRegister", free_immediately, size)]
struct Wrapper(RefCell<Register>);

impl Wrapper {
    /// `value` as exactly a `MergeMachines::LwwRegister`: `TypeError` with `message`
    /// for anything else, a subclass instance included.
    fn exact(ruby: &Ruby, value: Value, message: impl Message) -> Result<Obj<Self>, Error> {
        exact_typed_with(ruby, value, CLASS.get(ruby)?.get(ruby), message)
    }

    /// `LwwRegister::new(value, stamp, replica)`, of the kind `stamp` is.
    fn new(ruby: &Ruby, value: Value, stamp: Value, replica: Value) -> Result<Self, Error> {
        let value: u64 = integer(value, "value")?;
        let replica: ReplicaId = integer(replica, "replica")?;
        let register = match stamp_from(ruby, stamp)? {
            Stamped::Logical(stamp) => Register::Logical(LwwRegister::new(value, stamp, replica)),
            Stamped::Hybrid(stamp) => Register::Hybrid(LwwRegister::new(value, stamp, replica)),
        };
        Ok(Self(RefCell::new(register)))
    }

    /// `LwwRegister::decode`, of the kind of stamp the frame names.
    fn decode(ruby: &Ruby, frame: Value) -> Result<Self, Error> {
        let frame = bytes(frame, "frame")?;
        let kind = frame.get(SHAPE).map(|field| [field[0], field[1]]);
        let decoded = match kind {
            None => Err(Refusal::Truncated),
            Some([STAMP_LOGICAL, 0]) => LwwRegister::decode(&frame).map(Register::Logical),
            Some([STAMP_HYBRID, 0]) => LwwRegister::decode(&frame).map(Register::Hybrid),
            Some(_) => Err(Refusal::WrongShape),
        };
        decoded
            .map(|register| Self(RefCell::new(register)))
            .map_err(|refused| refused_frame(ruby, refused))
    }

    /// `:written`, `:duplicate`, `:superseded` or `:mismatch`. Every argument
    /// is read before the core is called; a stamp of the other kind is raised
    /// on once the register's borrow has ended, as naming it runs `inspect`.
    fn set(
        ruby: &Ruby,
        rb_self: Obj<Self>,
        value: Value,
        stamp: Value,
        replica: Value,
    ) -> Result<Symbol, Error> {
        rb_self.check_frozen()?;
        let value: u64 = integer(value, "value")?;
        let replica: ReplicaId = integer(replica, "replica")?;
        let given = stamp_from(ruby, stamp)?;
        let outcome = match (&mut *rb_self.0.borrow_mut(), given) {
            (Register::Logical(held), Stamped::Logical(stamp)) => {
                Ok(held.set(value, stamp, replica))
            }
            (Register::Hybrid(held), Stamped::Hybrid(stamp)) => Ok(held.set(value, stamp, replica)),
            (Register::Logical(_), Stamped::Hybrid(_)) => Err("an Integer stamp"),
            (Register::Hybrid(_), Stamped::Logical(_)) => Err("a MergeMachines::Timestamp"),
        };
        let outcome = outcome.map_err(|what| expected(ruby, what, stamp))?;
        Ok(match outcome {
            Ok(Writing::Written) => ruby.to_symbol("written"),
            Ok(Writing::Duplicate) => ruby.to_symbol("duplicate"),
            Ok(Writing::Superseded) => ruby.to_symbol("superseded"),
            Err(refused) => refusal(ruby, refused),
        })
    }

    fn value(&self) -> u64 {
        match &*self.0.borrow() {
            Register::Logical(register) => *register.value(),
            Register::Hybrid(register) => *register.value(),
        }
    }

    /// The stamp of the write held: an Integer or a `Timestamp`.
    fn stamp(ruby: &Ruby, rb_self: &Self) -> Result<Value, Error> {
        match &*rb_self.0.borrow() {
            Register::Logical(register) => Ok(register.stamp().into_value_with(ruby)),
            Register::Hybrid(register) => data::timestamp(ruby, classes(ruby)?, *register.stamp()),
        }
    }

    fn replica(&self) -> ReplicaId {
        match &*self.0.borrow() {
            Register::Logical(register) => register.replica(),
            Register::Hybrid(register) => register.replica(),
        }
    }

    /// `:merged` or `:mismatch`. A register of the other kind is raised on
    /// once this one's borrow has ended: naming it runs `other.inspect`.
    fn merge(ruby: &Ruby, rb_self: Obj<Self>, other: Value) -> Result<Symbol, Error> {
        rb_self.check_frozen()?;
        let source = Self::exact(ruby, other, || {
            expected_class("a MergeMachines::LwwRegister", other)
        })?
        .0
        .borrow()
        .clone();
        let outcome = match (&mut *rb_self.0.borrow_mut(), &source) {
            (Register::Logical(held), Register::Logical(theirs)) => Some(held.merge(theirs)),
            (Register::Hybrid(held), Register::Hybrid(theirs)) => Some(held.merge(theirs)),
            (Register::Logical(_), Register::Hybrid(_))
            | (Register::Hybrid(_), Register::Logical(_)) => None,
        };
        merged(ruby, outcome, other, "a register of the same kind of stamp")
    }

    fn eq(ruby: &Ruby, rb_self: &Self, other: Value) -> bool {
        Self::exact(ruby, other, || "").is_ok_and(|other| *rb_self.0.borrow() == *other.0.borrow())
    }

    /// `LwwRegister::encoded`.
    fn encoded(ruby: &Ruby, rb_self: &Self) -> RString {
        let frame = match &*rb_self.0.borrow() {
            Register::Logical(register) => register.encoded(),
            Register::Hybrid(register) => register.encoded(),
        };
        ruby.str_from_slice(&frame)
    }

    fn inspect(&self) -> String {
        format!(
            "#<MergeMachines::LwwRegister replica={} value={}>",
            self.replica(),
            self.value()
        )
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = module.define_class("LwwRegister", ruby.class_object())?;
    CLASS.set(ruby, Class::pin(ruby, class))?;
    class.undef_default_alloc_func();
    class.define_singleton_method("new", function!(Wrapper::new, 3))?;
    class.define_singleton_method("decode", function!(Wrapper::decode, 1))?;
    class.define_method("set", method!(Wrapper::set, 3))?;
    class.define_method("value", method!(Wrapper::value, 0))?;
    class.define_method("stamp", method!(Wrapper::stamp, 0))?;
    class.define_method("replica", method!(Wrapper::replica, 0))?;
    class.define_method("merge", method!(Wrapper::merge, 1))?;
    class.define_method("==", method!(Wrapper::eq, 1))?;
    class.define_method("encoded", method!(Wrapper::encoded, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
