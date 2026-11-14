//! Last writer wins, with the replica id as the tie-break.

use crate::{
    FIELD_LEN, Merge, Refusal, ReplicaId, SHAPE_LEN, TAG, TAG_LEN, TAG_LWW_REGISTER, read_small,
    read_u64, small_field, tagged_frame,
};

/// A stamp naming one 64-bit counter, as [`LogicalClock`](crate::LogicalClock)
/// issues.
pub const STAMP_LOGICAL: u8 = 0x01;

/// A stamp naming a wall-clock reading and its counter, as
/// [`HybridClock`](crate::HybridClock) issues.
pub const STAMP_HYBRID: u8 = 0x02;

/// A stamp a register's frame can carry.
pub trait Stamp: Copy + Default + Ord {
    /// Byte naming this stamp in a register frame.
    const KIND: u8;

    /// Bytes this stamp occupies in a register frame.
    const ENCODED_LEN: usize;

    /// Writes this stamp into `field`, which is
    /// [`ENCODED_LEN`](Self::ENCODED_LEN) long.
    fn write(self, field: &mut [u8]);

    /// Reads a stamp from `field`, which is
    /// [`ENCODED_LEN`](Self::ENCODED_LEN) long.
    fn read(field: &[u8]) -> Self;
}

impl Stamp for u64 {
    const KIND: u8 = STAMP_LOGICAL;
    const ENCODED_LEN: usize = FIELD_LEN;

    fn write(self, field: &mut [u8]) {
        field.copy_from_slice(&self.to_le_bytes());
    }

    fn read(field: &[u8]) -> Self {
        read_u64(field)
    }
}

/// Where the stamp kind sits in a frame.
const STAMP_KIND: core::ops::Range<usize> = TAG_LEN..TAG_LEN + SHAPE_LEN;

/// Where the value sits in a frame.
const VALUE: core::ops::Range<usize> = STAMP_KIND.end..STAMP_KIND.end + FIELD_LEN;

/// Where the writing replica sits in a frame.
const REPLICA: core::ops::Range<usize> = VALUE.end..VALUE.end + FIELD_LEN;

/// What an honoured [`LwwRegister::set`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Writing {
    /// The write beat the one held and is held now.
    Written,
    /// This very write, the same value under the same stamp and replica, is
    /// already held; nothing changed.
    Duplicate,
    /// A later write is held, so this one lost and nothing changed. That is
    /// the register working, not a refusal.
    Superseded,
}

/// One value, held by whichever write was stamped last.
///
/// ```
/// use merge_machines::{LwwRegister, Merge, Refusal, Writing};
///
/// let mut left = LwwRegister::new(10_u32, 4_u64, 1);
/// let right = LwwRegister::new(20, 9, 2);
///
/// assert_eq!(left.merge(&right), Ok(()));
/// assert_eq!(*left.value(), 20);
///
/// // A stamp from before the current one loses, and says so.
/// assert_eq!(left.set(30, 1, 1), Ok(Writing::Superseded));
/// assert_eq!(*left.value(), 20);
///
/// // The same stamp and replica can only ever name one value.
/// assert_eq!(left.set(20, 9, 2), Ok(Writing::Duplicate));
/// assert_eq!(left.set(21, 9, 2), Err(Refusal::Mismatch));
///
/// // Equal stamps go to the higher replica id, from either side.
/// let mut low = LwwRegister::new(1_u32, 5_u64, 1);
/// let high = LwwRegister::new(2, 5, 2);
/// assert_eq!(low.merge(&high), Ok(()));
/// assert_eq!(*low.value(), 2);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LwwRegister<T, S> {
    value: T,
    stamp: S,
    replica: ReplicaId,
}

impl<T: Clone + PartialEq, S: Ord + Copy> LwwRegister<T, S> {
    /// A register holding `value`, written by `replica` at `stamp`.
    #[must_use]
    pub const fn new(value: T, stamp: S, replica: ReplicaId) -> Self {
        Self {
            value,
            stamp,
            replica,
        }
    }

    /// Writes `value` if `(stamp, replica)` beats the write already held.
    /// An older write is honoured as [`Writing::Superseded`], the write
    /// already held as [`Writing::Duplicate`]; neither changes anything.
    ///
    /// # Errors
    ///
    /// [`Refusal::Mismatch`] when the held write has the same stamp and
    /// replica and another value; the register is unchanged.
    pub fn set(&mut self, value: T, stamp: S, replica: ReplicaId) -> Result<Writing, Refusal> {
        match (stamp, replica).cmp(&(self.stamp, self.replica)) {
            core::cmp::Ordering::Less => Ok(Writing::Superseded),
            core::cmp::Ordering::Equal if value == self.value => Ok(Writing::Duplicate),
            core::cmp::Ordering::Equal => Err(Refusal::Mismatch),
            core::cmp::Ordering::Greater => {
                self.value = value;
                self.stamp = stamp;
                self.replica = replica;
                Ok(Writing::Written)
            }
        }
    }

    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// The stamp of the write being held.
    #[must_use]
    pub const fn stamp(&self) -> &S {
        &self.stamp
    }

    /// The replica that wrote what is being held.
    #[must_use]
    pub const fn replica(&self) -> ReplicaId {
        self.replica
    }
}

impl<S: Stamp> LwwRegister<u64, S> {
    /// Length of the [`encode`](Self::encode) frame.
    pub const ENCODED_LEN: usize = REPLICA.end + S::ENCODED_LEN;

    /// Where the stamp sits in a frame.
    const STAMP: core::ops::Range<usize> = REPLICA.end..Self::ENCODED_LEN;

    /// A register holding nothing, for [`decode`](Self::decode) to read a frame
    /// over.
    fn empty() -> Self {
        Self {
            value: 0,
            stamp: S::default(),
            replica: 0,
        }
    }

    encoders!("register");

    /// Fills a frame of exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    fn write_frame(&self, frame: &mut [u8]) {
        frame[TAG] = TAG_LWW_REGISTER;
        frame[STAMP_KIND].copy_from_slice(&small_field(usize::from(S::KIND)));
        frame[VALUE].copy_from_slice(&self.value.to_le_bytes());
        frame[REPLICA].copy_from_slice(&self.replica.to_le_bytes());
        self.stamp.write(&mut frame[Self::STAMP]);
    }

    /// Reads a register written by [`encode`](Self::encode).
    ///
    /// # Errors
    ///
    /// [`Refusal::Truncated`] when `bytes` is too short,
    /// [`Refusal::WrongStructure`] when it holds another structure,
    /// [`Refusal::WrongShape`] when it was written with another kind of stamp.
    pub fn decode(bytes: &[u8]) -> Result<Self, Refusal> {
        let mut register = Self::empty();
        register.decode_into(bytes)?;
        Ok(register)
    }

    /// Reads a frame over a register the caller already holds.
    ///
    /// The register is untouched when the frame is rejected.
    ///
    /// # Errors
    ///
    /// The same as [`decode`](Self::decode).
    pub fn decode_into(&mut self, bytes: &[u8]) -> Result<(), Refusal> {
        let frame = tagged_frame(bytes, Self::ENCODED_LEN, TAG_LWW_REGISTER)?;
        if read_small(&frame[STAMP_KIND]) != usize::from(S::KIND) {
            return Err(Refusal::WrongShape);
        }

        self.value = read_u64(&frame[VALUE]);
        self.replica = read_u64(&frame[REPLICA]);
        self.stamp = S::read(&frame[Self::STAMP]);
        Ok(())
    }
}

impl<T: Clone + PartialEq, S: Ord + Copy> Merge for LwwRegister<T, S> {
    type Error = Refusal;

    /// Keeps whichever write was stamped last.
    ///
    /// # Errors
    ///
    /// [`Refusal::Mismatch`] when both hold a write with the same stamp and
    /// replica and different values; the register is unchanged.
    fn merge(&mut self, other: &Self) -> Result<(), Self::Error> {
        match (other.stamp, other.replica).cmp(&(self.stamp, self.replica)) {
            core::cmp::Ordering::Less => Ok(()),
            core::cmp::Ordering::Equal if other.value == self.value => Ok(()),
            core::cmp::Ordering::Equal => Err(Refusal::Mismatch),
            core::cmp::Ordering::Greater => {
                self.value.clone_from(&other.value);
                self.stamp = other.stamp;
                self.replica = other.replica;
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests;
