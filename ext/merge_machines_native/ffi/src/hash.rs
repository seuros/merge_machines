//! The binding's only hash. The core's elements are `Copy + Eq`, so a Ruby
//! string crosses as a 64-bit hash of its UTF-8 bytes.

use crate::{FIELD_LEN, field};

const PRIME: u64 = 0x9E37_79B9_7F4A_7C15;

/// `splitmix64`'s finalizer: a bijection on [`u64`].
const fn mix64(value: u64) -> u64 {
    let mut z = value.wrapping_add(PRIME);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Hashes bytes, folding [`FIELD_LEN`] at a time through [`mix64`], then the
/// remainder and the length separately.
pub fn hash64(bytes: &[u8]) -> u64 {
    let (lanes, tail) = bytes.as_chunks::<FIELD_LEN>();
    let mut accumulator = PRIME;

    for lane in lanes {
        accumulator = (accumulator ^ mix64(u64::from_le_bytes(*lane)))
            .rotate_left(27)
            .wrapping_mul(PRIME);
    }

    let mut last = [0_u8; FIELD_LEN];
    last[..tail.len()].copy_from_slice(tail);
    let length = u64::from_le_bytes(field(bytes.len()));

    mix64(mix64(accumulator ^ length.wrapping_mul(PRIME)) ^ mix64(u64::from_le_bytes(last)))
}
