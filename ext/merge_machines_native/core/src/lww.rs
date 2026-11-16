//! Last writer wins, with the replica id as the tie-break.

use crate::{
    FIELD_LEN, Merge, Reader, Refusal, ReplicaId, SHAPE_LEN, TAG_LEN, TAG_LWW_REGISTER, Writer,
    tagged_frame,
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

    /// Writes this stamp into the front of `field`.
    ///
    /// # Errors
    ///
    /// [`Refusal::Truncated`] when `field` is shorter than
    /// [`ENCODED_LEN`](Self::ENCODED_LEN).
    fn write(self, field: &mut [u8]) -> Result<(), Refusal>;

    /// Reads a stamp from the front of `field`.
    ///
    /// # Errors
    ///
    /// [`Refusal::Truncated`] when `field` is shorter than
    /// [`ENCODED_LEN`](Self::ENCODED_LEN).
    fn read(field: &[u8]) -> Result<Self, Refusal>;
}

impl Stamp for u64 {
    const KIND: u8 = STAMP_LOGICAL;
    const ENCODED_LEN: usize = FIELD_LEN;

    fn write(self, field: &mut [u8]) -> Result<(), Refusal> {
        Writer::new(field).u64(self)
    }

    fn read(field: &[u8]) -> Result<Self, Refusal> {
        Reader::new(field).u64()
    }
}

/// Bytes a frame spends ahead of its stamp: the tag, the stamp kind, the
/// value and the writing replica, in that order.
const HEAD_LEN: usize = TAG_LEN + SHAPE_LEN + FIELD_LEN + FIELD_LEN;

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

    /// The value of the write held.
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
    pub const ENCODED_LEN: usize = HEAD_LEN + S::ENCODED_LEN;

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
    fn write_frame(&self, frame: &mut Writer<'_>) -> Result<(), Refusal> {
        frame.byte(TAG_LWW_REGISTER)?;
        frame.small(usize::from(S::KIND))?;
        frame.u64(self.value)?;
        frame.u64(self.replica)?;
        self.stamp.write(frame.take(S::ENCODED_LEN)?)
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
        let mut frame = tagged_frame(bytes, Self::ENCODED_LEN, TAG_LWW_REGISTER)?;
        if frame.small()? != usize::from(S::KIND) {
            return Err(Refusal::WrongShape);
        }

        let value = frame.u64()?;
        let replica = frame.u64()?;
        let stamp = S::read(frame.take(S::ENCODED_LEN)?)?;
        self.value = value;
        self.replica = replica;
        self.stamp = stamp;
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
