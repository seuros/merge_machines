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
mod init;
mod logical;
mod lww;
mod orset;
mod pncounter;
mod shape;

use magnus::{Error, Ruby, Symbol, Value, prelude::*};
use matryona_machines::ruby::{Symbols, symbol};
use merge_machines::Refusal;

/// Where a core frame names the shape it was written at.
pub(crate) const SHAPE: core::ops::Range<usize> = 1..3;

matryona_machines::symbols! {
    /// The symbol each refusal comes back to Ruby as.
    pub(crate) Refusals for Refusal as "a refusal" {
        Full => "full",
        Overflow => "overflow",
        Exhausted => "exhausted",
        Mismatch => "mismatch",
        Truncated => "truncated",
        WrongStructure => "wrong_structure",
        WrongShape => "wrong_shape",
        Malformed => "malformed",
    }
}

pub(crate) fn refusal(ruby: &Ruby, refusal: Refusal) -> Symbol {
    Refusals::symbol(ruby, refusal)
}

/// `honoured` for an honoured unit answer, the refusal's symbol otherwise.
pub(crate) fn unit(ruby: &Ruby, outcome: Result<(), Refusal>, honoured: &str) -> Symbol {
    match outcome {
        Ok(()) => symbol(ruby, honoured),
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

/// The `RangeError` for a frame `decode` turned down.
pub(crate) fn refused_frame(ruby: &Ruby, refused: Refusal) -> Error {
    Error::new(
        ruby.exception_range_error(),
        format!("the frame is refused: {}", Refusals::name(refused)),
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
    Error::new(ruby.exception_type_error(), expected_message(what, given))
}

/// What [`expected`] says, for a reader that builds its own `TypeError`.
pub(crate) fn expected_message(what: &str, given: Value) -> String {
    format!("expected {what}, got {}", given.inspect())
}
