//! `MergeMachines::LwwRegister`: one unsigned 64-bit value, held by whichever
//! write was stamped last.
//!
//! The core has one register type per kind of stamp. Ruby has one class: a
//! register built with an Integer stamp is `LwwRegister<u64, u64>`, one built
//! with a `Timestamp` is `LwwRegister<u64, HybridTimestamp>`, and a stamp or a
//! register of the other kind is a `TypeError`.

use std::cell::RefCell;

use magnus::typed_data::Obj;
use magnus::{Error, Integer, IntoValue, RString, Ruby, Symbol, Value, prelude::*};
use matryona_machines::ruby::{Class, Registry, integer};
use merge_machines::{
    HybridTimestamp, LwwRegister, Merge, Refusal, ReplicaId, STAMP_HYBRID, STAMP_LOGICAL, Writing,
};

use crate::data::{self, classes, timestamp_from};
use crate::{expected, frame_bytes, refusal, refused_frame, shape_field};

/// A register stamped by one of the two clocks.
#[derive(Clone, PartialEq, Eq)]
enum Register {
    Logical(LwwRegister<u64, u64>),
    Hybrid(LwwRegister<u64, HybridTimestamp>),
}

impl Register {
    /// Folds `other` in, or `None` when it is stamped by the other clock.
    fn absorb(&mut self, other: &Self) -> Option<Result<(), Refusal>> {
        match (self, other) {
            (Self::Logical(held), Self::Logical(theirs)) => Some(held.merge(theirs)),
            (Self::Hybrid(held), Self::Hybrid(theirs)) => Some(held.merge(theirs)),
            (Self::Logical(_), Self::Hybrid(_)) | (Self::Hybrid(_), Self::Logical(_)) => None,
        }
    }
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
    crate::exact!("MergeMachines::LwwRegister");
    crate::eq!();
    crate::merge!(
        "MergeMachines::LwwRegister",
        "a register of the same kind of stamp"
    );

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
        let frame = frame_bytes(frame)?;
        let kind = shape_field(&frame);
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

crate::define!("LwwRegister", new(3) {
    "set" => set(3),
    "value" => value(0),
    "stamp" => stamp(0),
    "replica" => replica(0),
    "merge" => merge(1),
});
