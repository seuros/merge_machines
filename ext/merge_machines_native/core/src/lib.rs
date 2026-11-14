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
//! slot left answers [`Refusal::Full`] rather than allocating. A clock whose
//! counter has run out answers [`Refusal::Exhausted`] rather than reissuing
//! the timestamp it last gave away.
//!
//! # The contract
//!
//! Every reason a call is not honoured is a variant of the one [`Refusal`],
//! and a refused call changes nothing: a merge lands whole or not at all. A
//! call whose effect already holds is honoured, says so by name
//! ([`Inserting::Duplicate`], [`Adding::Duplicate`], [`Removing::NotHeld`],
//! [`Writing::Duplicate`]) and changes nothing either. A write that loses to a
//! later one is honoured too ([`Writing::Superseded`]): that is the register
//! working.
//!
//! No call takes a monotonic `now`, so no answer records one. The hybrid
//! clock's `tick` and `observe` fold the caller's `wall_ns` into the stamp
//! they issue: that is what a hybrid stamp is, and a refused stamp folds
//! nothing.
//!
//! # Numbers this crate issues
//!
//! The clocks issue stamps. They are protocol numbers every peer must agree
//! on and compare, not handles to something this crate holds, so they carry
//! no scope and are bare integers. Replica ids and tags are the caller's. A
//! `wall_ns` is an event-time stamp, not an instant on a monotonic clock.
//!
//! # Putting state on a wire
//!
//! Every state encodes to a fixed-length frame of its own
//! `ENCODED_LEN` bytes, led by a tag naming the structure and, where there is
//! one, a field naming the const generic. `decode` and `decode_into` refuse a
//! short buffer, another structure's tag, a frame written at another shape and
//! a frame describing a state no sequence of calls produces.

#[cfg(feature = "alloc")]
extern crate alloc;

/// Compiles and runs the README's examples as doctests.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

/// Defines a state's `encode` and `encoded` over its private `write_frame`,
/// which fills a frame of exactly `ENCODED_LEN` bytes. `$noun` names the
/// state in the docs.
macro_rules! encoders {
    ($noun:literal) => {
        #[doc = concat!("Writes the ", $noun, " into `out` and returns the bytes written.")]
        ///
        /// # Errors
        ///
        /// [`Refusal::Truncated`] when `out` is shorter than
        /// [`ENCODED_LEN`](Self::ENCODED_LEN).
        pub fn encode(&self, out: &mut [u8]) -> Result<usize, Refusal> {
            $crate::write_into(out, Self::ENCODED_LEN, |frame| self.write_frame(frame))
        }

        #[doc = concat!("The ", $noun, "'s frame, [`ENCODED_LEN`](Self::ENCODED_LEN) bytes long.")]
        #[cfg(feature = "alloc")]
        #[must_use]
        pub fn encoded(&self) -> alloc::boxed::Box<[u8]> {
            $crate::boxed_frame(Self::ENCODED_LEN, |frame| self.write_frame(frame))
        }
    };
}

pub mod gcounter;
pub mod gset;
pub mod hybrid;
pub mod logical;
pub mod lww;
pub mod orset;
pub mod pncounter;

pub use gcounter::{Entry, GCounter};
pub use gset::{GSet, Inserting};
pub use hybrid::{HybridClock, HybridTimestamp};
pub use logical::LogicalClock;
pub use lww::{LwwRegister, STAMP_HYBRID, STAMP_LOGICAL, Stamp, Writing};
pub use orset::{Adding, OrSet, Removing, Tag, Tagged};
pub use pncounter::PNCounter;

/// Which replica an update came from. Chosen by the caller, who keeps it
/// unique and durable; the crate issues none.
pub type ReplicaId = u64;

/// A state that can absorb another replica's state.
pub trait Merge {
    /// Why this state could not absorb the other one. Every type in this crate
    /// answers [`Refusal`]; a sibling crate implementing the trait carries its
    /// own.
    type Error;

    /// Folds `other` into this state: afterwards this state has seen
    /// everything `other` had. Folding in a state already seen is honoured
    /// and changes nothing.
    ///
    /// # Errors
    ///
    /// The implementor's refusal, and then this state is exactly what it was:
    /// a merge lands whole or not at all.
    fn merge(&mut self, other: &Self) -> Result<(), Self::Error>;
}

/// Why a call was not honoured. A refused call changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The call needs a slot and the fixed capacity has none left.
    Full,
    /// A replica's count would pass [`u64::MAX`].
    Overflow,
    /// A clock has issued the last timestamp its counter can hold.
    Exhausted,
    /// The same tag, or the same stamp and replica, already names something
    /// else: two writes that must be one disagree.
    Mismatch,
    /// The buffer is shorter than the frame.
    Truncated,
    /// The frame's leading byte names another structure.
    WrongStructure,
    /// The frame was written at another shape: another capacity, replica
    /// count or kind of stamp.
    WrongShape,
    /// The frame has the right shape and describes a state no sequence of
    /// calls produces.
    Malformed,
}

impl core::fmt::Display for Refusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Full => "no free slot",
            Self::Overflow => "the count would pass u64::MAX",
            Self::Exhausted => "no timestamp left",
            Self::Mismatch => "the same tag or stamp already names something else",
            Self::Truncated => "buffer too short",
            Self::WrongStructure => "the frame holds another structure",
            Self::WrongShape => "the frame was written at another shape",
            Self::Malformed => "the frame describes an impossible state",
        })
    }
}

impl core::error::Error for Refusal {}

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

/// Writes a frame of `len` bytes at the front of `out` through `fill`,
/// returning the number of bytes written.
pub(crate) fn write_into(
    out: &mut [u8],
    len: usize,
    fill: impl FnOnce(&mut [u8]),
) -> Result<usize, Refusal> {
    let Some(frame) = out.get_mut(..len) else {
        return Err(Refusal::Truncated);
    };
    fill(frame);
    Ok(len)
}

/// A frame of exactly `len` bytes, filled by `fill`.
#[cfg(feature = "alloc")]
pub(crate) fn boxed_frame(len: usize, fill: impl FnOnce(&mut [u8])) -> alloc::boxed::Box<[u8]> {
    let mut frame = alloc::vec![0_u8; len].into_boxed_slice();
    fill(&mut frame);
    frame
}

/// The `len`-byte frame `bytes` leads with, which `tag` has to name.
pub(crate) fn tagged_frame(bytes: &[u8], len: usize, tag: u8) -> Result<&[u8], Refusal> {
    let Some(frame) = bytes.get(..len) else {
        return Err(Refusal::Truncated);
    };
    if frame[TAG] != tag {
        return Err(Refusal::WrongStructure);
    }
    Ok(frame)
}

/// Whether the `incoming` slots `unseen` marks fit in the free slots of
/// `slots`.
pub(crate) fn fits_free<T>(slots: &[Option<T>], unseen: &[bool]) -> bool {
    let free = slots.iter().filter(|slot| slot.is_none()).count();
    unseen.iter().filter(|unseen| **unseen).count() <= free
}

/// Copies every `incoming` slot `unseen` marks into a free slot of `slots`,
/// once [`fits_free`] has said they all land.
pub(crate) fn fill_free<T: Copy>(slots: &mut [Option<T>], incoming: &[Option<T>], unseen: &[bool]) {
    let incoming = incoming
        .iter()
        .zip(unseen)
        .filter_map(|(slot, unseen)| slot.filter(|_| *unseen));
    let free = slots.iter_mut().filter(|slot| slot.is_none());
    for (slot, held) in free.zip(incoming) {
        *slot = Some(held);
    }
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

#[cfg(test)]
mod tests;
