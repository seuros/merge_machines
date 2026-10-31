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
//! slot left reports [`CapacityError`] rather than allocating.

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
pub use lww::LwwRegister;
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

impl core::fmt::Display for CapacityError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("no free slot")
    }
}

impl core::error::Error for CapacityError {}

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
