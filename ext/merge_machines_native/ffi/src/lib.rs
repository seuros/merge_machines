//! Ruby bindings for the `merge-machines` replicated states: a thin mirror.
//!
//! Replica counts and set capacities are const generics, so every class
//! dispatches the number it is given to a fixed set of monomorphised variants
//! and raises `RangeError` for any other number. Elements, values, replica ids,
//! tags and stamps are unsigned 64-bit Integers, as on a core frame.
//!
//! Every refusal comes back as the `snake_case` symbol of its `Refusal` variant
//! and never raises. A frame `decode` refuses raises `RangeError`: it is saved
//! state a restoring constructor turned down.

mod data;
mod gcounter;
mod gset;
mod hybrid;
mod logical;
mod lww;
mod orset;
mod pncounter;
mod shape;

use std::cell::RefCell;

use magnus::rb_sys::AsRawValue;
use magnus::value::ReprValue;
use magnus::{Error, Integer, RClass, Ruby, Symbol, TryConvert, Value};
use merge_machines::Refusal;

/// Where a core frame names the shape it was written at.
pub(crate) const SHAPE: core::ops::Range<usize> = 1..3;

/// A whole number of things: a replica, an element, a stamp, a capacity, a
/// count, a nanosecond reading.
///
/// magnus reads an integer through Ruby's `NUM2*` family, which truncates a
/// `Float` towards zero instead of refusing it, so an argument that means an
/// integer is read as one of these and anything but an `Integer` is a
/// `TypeError`. An `Integer` outside the target width, negative ones included,
/// is the `RangeError` magnus raises for it.
pub(crate) struct Whole<T>(pub(crate) T);

impl<T: TryConvert> TryConvert for Whole<T> {
    fn try_convert(value: Value) -> Result<Self, Error> {
        if Integer::from_value(value).is_none() {
            let ruby = Ruby::get_with(value);
            return Err(Error::new(
                ruby.exception_type_error(),
                format!(
                    "no implicit conversion of {} into Integer: {}",
                    value.class(),
                    value.inspect()
                ),
            ));
        }
        T::try_convert(value).map(Self)
    }
}

/// The symbol a refusal comes back to Ruby as.
pub(crate) const fn refusal_name(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::Full => "full",
        Refusal::Overflow => "overflow",
        Refusal::Exhausted => "exhausted",
        Refusal::Mismatch => "mismatch",
        Refusal::Truncated => "truncated",
        Refusal::WrongStructure => "wrong_structure",
        Refusal::WrongShape => "wrong_shape",
        Refusal::Malformed => "malformed",
    }
}

pub(crate) fn refusal(ruby: &Ruby, refusal: Refusal) -> Symbol {
    ruby.to_symbol(refusal_name(refusal))
}

/// `honoured` for an honoured unit answer, the refusal's symbol otherwise.
pub(crate) fn unit(ruby: &Ruby, outcome: Result<(), Refusal>, honoured: &str) -> Symbol {
    match outcome {
        Ok(()) => ruby.to_symbol(honoured),
        Err(refused) => refusal(ruby, refused),
    }
}

/// `:merged` or the refusal's symbol; the `TypeError` naming `what` when
/// `other` held another shape and nothing was folded.
pub(crate) fn merged(
    ruby: &Ruby,
    outcome: Option<Result<(), Refusal>>,
    other: Value,
    what: &str,
) -> Result<Symbol, Error> {
    outcome
        .map(|outcome| unit(ruby, outcome, "merged"))
        .ok_or_else(|| expected(ruby, what, other))
}

/// Runs `act` on a copy of the state in `cell` and keeps the copy only once
/// `act`, which builds the answer and may run Ruby, has returned. A raise
/// while building leaves the state as it was, so no work is lost. Right
/// before the store the receiver is checked again: frozen meanwhile (a
/// redefined `new` freezing it) raises `FrozenError`, changed meanwhile (a
/// redefined `new` calling back in) raises `RuntimeError`, and in both cases
/// the copy is not applied.
pub(crate) fn on_copy<T: Clone + PartialEq, R>(
    ruby: &Ruby,
    receiver: impl ReprValue,
    cell: &RefCell<T>,
    act: impl FnOnce(&mut T) -> Result<R, Error>,
) -> Result<R, Error> {
    receiver.check_frozen()?;
    let before = cell.borrow().clone();
    let mut after = before.clone();
    let answer = act(&mut after)?;
    receiver.check_frozen()?;
    let mut held = cell.borrow_mut();
    if *held != before {
        return Err(Error::new(
            ruby.exception_runtime_error(),
            "the state changed while the answer was built; the call was not applied",
        ));
    }
    *held = after;
    Ok(answer)
}

/// Whether `value` is an instance of exactly `class`: the classes compared by
/// identity, which no Ruby method can be redefined to fake. A subclass (whose
/// constant leaks past the seal) is refused, and so is a value given a
/// singleton class.
pub(crate) fn instance_of(value: Value, class: RClass) -> bool {
    value.class().as_raw() == class.as_raw()
}

/// The `RangeError` for a frame `decode` turned down.
pub(crate) fn refused_frame(ruby: &Ruby, refused: Refusal) -> Error {
    Error::new(
        ruby.exception_range_error(),
        format!("the frame is refused: {}", refusal_name(refused)),
    )
}

/// The shape a frame was written at, read from its header.
pub(crate) fn frame_shape(ruby: &Ruby, frame: &[u8]) -> Result<usize, Error> {
    let Some(field) = frame.get(SHAPE) else {
        return Err(refused_frame(ruby, Refusal::Truncated));
    };
    Ok(usize::from(u16::from_le_bytes([field[0], field[1]])))
}

pub(crate) fn unsupported(ruby: &Ruby, name: &str, given: usize, supported: &[usize]) -> Error {
    Error::new(
        ruby.exception_range_error(),
        format!("{name} {given} is not supported, expected one of {supported:?}"),
    )
}

/// The `TypeError` for an argument of the wrong class.
pub(crate) fn expected(ruby: &Ruby, what: &str, given: Value) -> Error {
    Error::new(
        ruby.exception_type_error(),
        format!("expected {what}, got {}", given.inspect()),
    )
}

pub(crate) fn uninitialised(ruby: &Ruby) -> Error {
    Error::new(
        ruby.exception_load_error(),
        "the merge_machines extension is not initialised",
    )
}

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("MergeMachines")?;
    data::define(ruby, module)?;
    logical::define(ruby, module)?;
    hybrid::define(ruby, module)?;
    gcounter::define(ruby, module)?;
    pncounter::define(ruby, module)?;
    lww::define(ruby, module)?;
    gset::define(ruby, module)?;
    orset::define(ruby, module)
}
