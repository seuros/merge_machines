#![cfg_attr(not(feature = "std"), no_std)]

//! Conflict-free replicated data types and the clocks they need.
//!
//! Every type here has a [`Merge::merge`] that is commutative, associative and
//! idempotent, so replicas that have seen the same updates hold the same state
//! no matter what order the updates arrived in, or how many times.
//!
//! Nothing here talks to a network and nothing here reads a clock. The caller
//! owns the transport, the replica ids, the tags, and every clock reading.
//!
//! # Which clock
//!
//! [`LogicalClock`] counts events. [`HybridClock`] pairs a wall-clock reading
//! with a counter, where a wall-clock reading means physical time that two
//! machines can compare: nanoseconds since the Unix epoch, which jumps
//! backwards when the host's clock is corrected. That is not the monotonic
//! reading you measure elapsed time with, and no deadline, timeout or budget
//! may be derived from it. Those belong to a monotonic clock, which this crate
//! does not have and does not want.
//!
//! State is fixed-size: capacities are const generics, and a state with no free
//! slot left reports [`CapacityError`] rather than allocating. A clock whose
//! counter has run out reports [`ClockExhausted`] rather than reissuing the
//! timestamp it last gave away.
//!
//! # Putting state on a wire
//!
//! Every state encodes to a fixed-length frame of its own
//! `ENCODED_LEN` bytes, led by a tag naming the structure and, where there is
//! one, a field naming the const generic. `decode` and `decode_into` reject a
//! short buffer, another structure's tag, and a frame written at another shape.

#[cfg(feature = "alloc")]
extern crate alloc;

/// Compiles and runs the README's examples as doctests.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub mod gcounter;
pub mod gset;
pub mod hybrid;
pub mod logical;
pub mod lww;
pub mod orset;
pub mod pncounter;

pub use gcounter::GCounter;
pub use gset::GSet;
pub use hybrid::{HybridClock, HybridTimestamp};
pub use logical::LogicalClock;
pub use lww::{LwwRegister, STAMP_HYBRID, STAMP_LOGICAL, Stamp};
pub use orset::{OrSet, Tag, Tagged};
pub use pncounter::PNCounter;

/// Which replica an update came from.
pub type ReplicaId = u64;

/// A state that can absorb another replica's state.
pub trait Merge {
    /// Why this state could not absorb the other one.
    type Error;

    /// Folds `other` into this state.
    ///
    /// # Errors
    ///
    /// Returns [`CapacityError`] when the union needs more slots than this
    /// state has. Nothing is rolled back: whatever fit stayed, and merging
    /// again after raising the capacity completes the job.
    fn merge(&mut self, other: &Self) -> Result<(), Self::Error>;
}

/// The union needed a slot and there was none left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapacityError;

/// A clock has issued the last timestamp its counter can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockExhausted;

impl core::fmt::Display for ClockExhausted {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("no timestamp left")
    }
}

impl core::error::Error for ClockExhausted {}

impl core::fmt::Display for CapacityError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("no free slot")
    }
}

impl core::error::Error for CapacityError {}

/// Why a state could not be read from or written to a byte buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecError {
    /// The buffer is shorter than the encoded form needs.
    Truncated,
    /// The leading byte names a different structure.
    Tag,
    /// The encoded shape does not match the target type's.
    Parameters,
}

impl core::fmt::Display for CodecError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Truncated => "buffer too short",
            Self::Tag => "wrong structure tag",
            Self::Parameters => "shape mismatch",
        })
    }
}

impl core::error::Error for CodecError {}

pub(crate) const TAG_LOGICAL_CLOCK: u8 = 0x51;
pub(crate) const TAG_HYBRID_CLOCK: u8 = 0x52;
pub(crate) const TAG_GCOUNTER: u8 = 0x61;
pub(crate) const TAG_PNCOUNTER: u8 = 0x62;
pub(crate) const TAG_LWW_REGISTER: u8 = 0x71;
pub(crate) const TAG_GSET: u8 = 0x81;
pub(crate) const TAG_ORSET: u8 = 0x82;

/// Where a frame's leading structure tag sits.
pub(crate) const TAG: usize = 0;

/// Bytes a frame spends on its leading structure tag.
pub(crate) const TAG_LEN: usize = 1;

/// Bytes a frame spends naming the shape it was written at.
pub(crate) const SHAPE_LEN: usize = 2;

/// Bytes a frame spends saying how many of its slot records carry a slot.
pub(crate) const COUNT_LEN: usize = 2;

/// Bytes a frame spends on one 64-bit field.
pub(crate) const FIELD_LEN: usize = 8;

/// A frame's 64-bit field, read from `field`, which is [`FIELD_LEN`] long.
pub(crate) const fn read_u64(field: &[u8]) -> u64 {
    let mut bytes = [0_u8; FIELD_LEN];
    bytes.copy_from_slice(field);
    u64::from_le_bytes(bytes)
}

/// The low two bytes of `value`, as a frame's shape or count field.
pub(crate) const fn small_field(value: usize) -> [u8; SHAPE_LEN] {
    let [low, high, ..] = value.to_le_bytes();
    [low, high]
}

/// A frame's shape or count field, read from `field`, which is [`SHAPE_LEN`]
/// long.
pub(crate) fn read_small(field: &[u8]) -> usize {
    let mut bytes = [0_u8; SHAPE_LEN];
    bytes.copy_from_slice(field);
    usize::from(u16::from_le_bytes(bytes))
}

/// Whether two slot arrays hold the same elements, ignoring where in the array
/// each one sits.
pub(crate) fn same_elements<T: PartialEq>(left: &[Option<T>], right: &[Option<T>]) -> bool {
    let contained =
        |haystack: &[Option<T>], needle: &T| haystack.iter().flatten().any(|held| held == needle);

    left.iter()
        .flatten()
        .all(|element| contained(right, element))
        && right
            .iter()
            .flatten()
            .all(|element| contained(left, element))
}

/// Index of the first slot nothing has claimed yet.
pub(crate) fn free_slot<T>(slots: &[Option<T>]) -> Option<usize> {
    slots.iter().position(Option::is_none)
}

#[cfg(test)]
mod tests;
