//! Last writer wins, with the replica id as the tie-break.

use crate::{
    CodecError, FIELD_LEN, Merge, ReplicaId, SHAPE_LEN, TAG, TAG_LEN, TAG_LWW_REGISTER, read_small,
    read_u64, small_field,
};
use core::convert::Infallible;

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

/// One value, held by whichever write was stamped last.
///
/// ```
/// use merge_machines::{LwwRegister, Merge};
///
/// let mut left = LwwRegister::new(10_u32, 4_u64, 1);
/// let right = LwwRegister::new(20, 9, 2);
///
/// assert_eq!(left.merge(&right), Ok(()));
/// assert_eq!(*left.value(), 20);
///
/// // A stamp from before the current one changes nothing.
/// left.set(30, 1, 1);
/// assert_eq!(*left.value(), 20);
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

impl<T: Clone, S: Ord + Copy> LwwRegister<T, S> {
    #[must_use]
    pub const fn new(value: T, stamp: S, replica: ReplicaId) -> Self {
        Self {
            value,
            stamp,
            replica,
        }
    }

    /// Writes `value` if `(stamp, replica)` beats the write already held.
    pub fn set(&mut self, value: T, stamp: S, replica: ReplicaId) {
        if (stamp, replica) > (self.stamp, self.replica) {
            self.value = value;
            self.stamp = stamp;
            self.replica = replica;
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

    /// Writes the register into `out`, returning the number of bytes written.
    ///
    /// # Errors
    ///
    /// [`CodecError::Truncated`] when `out` is shorter than
    /// [`ENCODED_LEN`](Self::ENCODED_LEN).
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let Some(frame) = out.get_mut(..Self::ENCODED_LEN) else {
            return Err(CodecError::Truncated);
        };
        self.write_frame(frame);
        Ok(Self::ENCODED_LEN)
    }

    /// The register's frame, exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    #[cfg(feature = "alloc")]
    #[must_use]
    pub fn encoded(&self) -> alloc::boxed::Box<[u8]> {
        let mut frame = alloc::vec![0_u8; Self::ENCODED_LEN].into_boxed_slice();
        self.write_frame(&mut frame);
        frame
    }

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
    /// [`CodecError::Truncated`] when `bytes` is too short, [`CodecError::Tag`]
    /// when it holds another structure, [`CodecError::Parameters`] when it was
    /// written with another kind of stamp.
    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
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
    pub fn decode_into(&mut self, bytes: &[u8]) -> Result<(), CodecError> {
        let Some(frame) = bytes.get(..Self::ENCODED_LEN) else {
            return Err(CodecError::Truncated);
        };
        if frame[TAG] != TAG_LWW_REGISTER {
            return Err(CodecError::Tag);
        }
        if read_small(&frame[STAMP_KIND]) != usize::from(S::KIND) {
            return Err(CodecError::Parameters);
        }

        self.value = read_u64(&frame[VALUE]);
        self.replica = read_u64(&frame[REPLICA]);
        self.stamp = S::read(&frame[Self::STAMP]);
        Ok(())
    }
}

impl<T: Clone, S: Ord + Copy> Merge for LwwRegister<T, S> {
    type Error = Infallible;

    fn merge(&mut self, other: &Self) -> Result<(), Self::Error> {
        if (other.stamp, other.replica) > (self.stamp, self.replica) {
            self.value.clone_from(&other.value);
            self.stamp = other.stamp;
            self.replica = other.replica;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
