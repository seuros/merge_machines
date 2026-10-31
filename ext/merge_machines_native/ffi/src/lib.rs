//! Ruby bindings for the `merge-machines` replicated states.
//!
//! Replica counts and set capacities are const generics, so every class
//! dispatches the number it is given to a fixed set of monomorphised variants
//! and raises `ArgumentError` for any other number.
//!
//! Wall-clock readings are nanoseconds on both sides of this boundary.
//! `NS_PER_SECOND` is exposed for callers holding seconds.

#![allow(clippy::needless_pass_by_value)]

mod data;
mod gcounter;
mod gset;
mod hash;
mod hybrid;
mod logical;
mod lww;
mod names;
mod orset;
mod pncounter;
mod shape;
mod wall;

use std::sync::OnceLock;

use magnus::scan_args::get_kwargs;
use magnus::value::Opaque;
use magnus::{Error, ExceptionClass, RHash, RModule, Ruby, TryConvert, prelude::*};
use merge_machines::{CapacityError, ClockExhausted, CodecError, ReplicaId};

/// `MergeMachines::Full`, defined in [`init`].
static FULL: OnceLock<Opaque<ExceptionClass>> = OnceLock::new();

/// `MergeMachines::Exhausted`, defined in [`init`].
static EXHAUSTED: OnceLock<Opaque<ExceptionClass>> = OnceLock::new();

/// Nanoseconds in a second.
pub(crate) const NS_PER_SECOND: u32 = 1_000_000_000;

/// Replica counts the counters are monomorphised for.
pub(crate) const REPLICAS: [usize; 4] = [4, 8, 16, 32];

/// Capacities the sets are monomorphised for.
pub(crate) const CAPACITIES: [usize; 3] = [64, 256, 1024];

/// Where a core frame names the shape it was written at.
pub(crate) const SHAPE: core::ops::Range<usize> = 1..3;

/// Bytes a 64-bit field occupies in a blob.
pub(crate) const FIELD_LEN: usize = 8;

/// The low [`FIELD_LEN`] bytes of `value`, little-endian.
pub(crate) fn field(value: usize) -> [u8; FIELD_LEN] {
    let mut bytes = [0_u8; FIELD_LEN];
    for (slot, byte) in bytes.iter_mut().zip(value.to_le_bytes()) {
        *slot = byte;
    }
    bytes
}

/// The one required keyword of a constructor.
pub(crate) fn required_kwarg<T: TryConvert>(keywords: RHash, name: &str) -> Result<T, Error> {
    Ok(get_kwargs::<_, (T,), (), ()>(keywords, &[name], &[])?
        .required
        .0)
}

/// A keyword with a default the caller may leave out.
pub(crate) fn optional_kwarg<T: TryConvert>(
    keywords: RHash,
    name: &str,
) -> Result<Option<T>, Error> {
    Ok(
        get_kwargs::<_, (), (Option<T>,), ()>(keywords, &[], &[name])?
            .optional
            .0,
    )
}

/// The shape a blob was written at, read from its core frame.
pub(crate) fn blob_shape(ruby: &Ruby, blob: &[u8]) -> Result<usize, Error> {
    let Some(field) = blob.get(SHAPE) else {
        return Err(codec_error(ruby, CodecError::Truncated));
    };
    Ok(usize::from(u16::from_le_bytes([field[0], field[1]])))
}

/// The replica id a clock's blob carries after its core frame.
pub(crate) fn trailing_replica(
    ruby: &Ruby,
    blob: &[u8],
    frame_len: usize,
) -> Result<ReplicaId, Error> {
    let Some((field, _)) = blob
        .get(frame_len..)
        .and_then(<[u8]>::split_first_chunk::<FIELD_LEN>)
    else {
        return Err(codec_error(ruby, CodecError::Truncated));
    };
    Ok(u64::from_le_bytes(*field))
}

pub(crate) fn unsupported(ruby: &Ruby, name: &str, given: usize, supported: &[usize]) -> Error {
    Error::new(
        ruby.exception_arg_error(),
        format!("{name} {given} is not supported, expected one of {supported:?}"),
    )
}

pub(crate) fn unsupported_blob(ruby: &Ruby, name: &str, supported: &[usize]) -> Error {
    Error::new(
        ruby.exception_arg_error(),
        format!("blob was written with a {name} outside {supported:?}"),
    )
}

pub(crate) fn codec_error(ruby: &Ruby, error: CodecError) -> Error {
    Error::new(ruby.exception_arg_error(), format!("codec: {error}"))
}

pub(crate) fn mismatch(ruby: &Ruby, name: &str) -> Error {
    Error::new(
        ruby.exception_arg_error(),
        format!("the other {name} has a different shape"),
    )
}

pub(crate) fn uninitialised(ruby: &Ruby) -> Error {
    Error::new(
        ruby.exception_runtime_error(),
        "the merge_machines extension is not initialised",
    )
}

pub(crate) fn defined_twice(ruby: &Ruby, name: &str) -> Error {
    Error::new(
        ruby.exception_runtime_error(),
        format!("MergeMachines::{name} was initialised twice"),
    )
}

/// Raises `MergeMachines::Full` for a state that ran out of slots.
pub(crate) fn capacity(ruby: &Ruby, outcome: Result<(), CapacityError>) -> Result<(), Error> {
    outcome.map_err(|error| raised(ruby, &FULL, error.to_string()))
}

/// Raises `MergeMachines::Exhausted` for a clock that ran out of timestamps.
pub(crate) fn exhausted<T>(ruby: &Ruby, outcome: Result<T, ClockExhausted>) -> Result<T, Error> {
    outcome.map_err(|error| raised(ruby, &EXHAUSTED, error.to_string()))
}

/// Raises `MergeMachines::Exhausted` for a set that ran out of tags.
pub(crate) fn tags_exhausted(ruby: &Ruby) -> Error {
    raised(ruby, &EXHAUSTED, "no tag left".to_owned())
}

fn raised(ruby: &Ruby, class: &OnceLock<Opaque<ExceptionClass>>, message: String) -> Error {
    class.get().map_or_else(
        || uninitialised(ruby),
        |class| Error::new(ruby.get_inner(*class), message),
    )
}

fn define_errors(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let base = module.define_error("Error", ruby.exception_standard_error())?;
    let full = module.define_error("Full", base)?;
    FULL.set(full.into())
        .map_err(|_| defined_twice(ruby, "Full"))?;
    let exhausted = module.define_error("Exhausted", base)?;
    EXHAUSTED
        .set(exhausted.into())
        .map_err(|_| defined_twice(ruby, "Exhausted"))
}

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("MergeMachines")?;
    module.const_set("NS_PER_SECOND", NS_PER_SECOND)?;
    define_errors(ruby, module)?;
    wall::define(ruby)?;
    data::define(ruby, module)?;
    logical::define(ruby, module)?;
    hybrid::define(ruby, module)?;
    gcounter::define(ruby, module)?;
    pncounter::define(ruby, module)?;
    lww::define(ruby, module)?;
    gset::define(ruby, module)?;
    orset::define(ruby, module)
}
