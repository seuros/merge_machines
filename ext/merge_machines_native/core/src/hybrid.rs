//! A hybrid logical clock: timestamps near physical time that still order.

use crate::{
    FIELD_LEN, Reader, Refusal, STAMP_HYBRID, Stamp, TAG_HYBRID_CLOCK, TAG_LEN, Writer,
    tagged_frame,
};

/// Bytes a [`HybridTimestamp`] occupies in a frame.
const TIMESTAMP_LEN: usize = 2 * FIELD_LEN;

/// A wall-clock reading paired with a counter that breaks its ties.
///
/// Both fields are protocol numbers every peer must read the same way, not
/// handles to anything this crate holds. `wall_ns` is an event-time stamp in
/// nanoseconds since the Unix epoch, not an instant on a monotonic clock, so
/// it stays a `u64` rather than a `Duration`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct HybridTimestamp {
    /// The wall-clock reading this was issued at, in nanoseconds.
    pub wall_ns: u64,
    /// How many timestamps have been issued at this reading.
    pub counter: u64,
}

impl HybridTimestamp {
    /// The timestamp at reading `wall_ns` with tie-breaking `counter`. Every
    /// pair is a valid timestamp.
    #[must_use]
    pub const fn new(wall_ns: u64, counter: u64) -> Self {
        Self { wall_ns, counter }
    }
}

impl Stamp for HybridTimestamp {
    const KIND: u8 = STAMP_HYBRID;
    const ENCODED_LEN: usize = TIMESTAMP_LEN;

    fn write(self, field: &mut [u8]) -> Result<(), Refusal> {
        let mut field = Writer::new(field);
        field.u64(self.wall_ns)?;
        field.u64(self.counter)
    }

    fn read(field: &[u8]) -> Result<Self, Refusal> {
        let mut field = Reader::new(field);
        Ok(Self {
            wall_ns: field.u64()?,
            counter: field.u64()?,
        })
    }
}

/// Issues timestamps that track physical time without ever going backwards.
///
/// # The reading this wants
///
/// `wall_ns` is physical time that two machines can compare: nanoseconds since
/// the Unix epoch. It is allowed to jump backwards, which is exactly why the
/// counter exists.
///
/// It is not a monotonic reading, and nothing derived from it is a deadline, a
/// timeout or a budget. Measuring how long something took needs the monotonic
/// clock, which this crate neither reads nor models. Wiring a monotonic reading
/// in here produces timestamps that no other machine can order against its own.
///
/// ```
/// use merge_machines::{Refusal, HybridClock, HybridTimestamp};
///
/// const MS: u64 = 1_000_000;
/// let mut clock = HybridClock::new();
///
/// let first = clock.tick(10 * MS)?;
/// assert_eq!(first.counter, 0);
///
/// // Same reading twice: the counter separates them.
/// let second = clock.tick(10 * MS)?;
/// assert!(second > first);
///
/// // The wall clock is corrected backwards; the timestamps are not.
/// let third = clock.tick(4 * MS)?;
/// assert!(third > second);
/// assert_eq!(third.wall_ns, 10 * MS);
///
/// // A reading that really did advance resets the counter.
/// let fourth = clock.tick(11 * MS)?;
/// assert_eq!(fourth.counter, 0);
///
/// // A counter with nothing left reports it instead of repeating itself.
/// let crowded = HybridTimestamp { wall_ns: 11 * MS, counter: u64::MAX };
/// assert_eq!(clock.observe(0, crowded), Err(Refusal::Exhausted));
/// assert_eq!(clock.last(), fourth);
/// # Ok::<(), Refusal>(())
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct HybridClock {
    last: HybridTimestamp,
}

const fn larger(left: u64, right: u64) -> u64 {
    if left > right { left } else { right }
}

impl HybridClock {
    /// A clock that has issued nothing: its last timestamp is `(0, 0)`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            last: HybridTimestamp {
                wall_ns: 0,
                counter: 0,
            },
        }
    }

    /// The last timestamp this clock issued.
    #[must_use]
    pub const fn last(&self) -> HybridTimestamp {
        self.last
    }

    /// Stamps a local event at wall-clock reading `wall_ns`.
    ///
    /// # Errors
    ///
    /// [`Refusal::Exhausted`] when the reading has not advanced and the counter
    /// that separates two events at the same reading holds its highest value,
    /// which leaves the clock where it was.
    pub const fn tick(&mut self, wall_ns: u64) -> Result<HybridTimestamp, Refusal> {
        if wall_ns > self.last.wall_ns {
            self.last = HybridTimestamp {
                wall_ns,
                counter: 0,
            };
            return Ok(self.last);
        }
        let Some(counter) = self.last.counter.checked_add(1) else {
            return Err(Refusal::Exhausted);
        };
        self.last.counter = counter;
        Ok(self.last)
    }

    /// Stamps the receipt of `observed` at wall-clock reading `wall_ns`.
    ///
    /// # Errors
    ///
    /// The same as [`tick`](Self::tick), and the observed timestamp is not
    /// adopted either.
    pub const fn observe(
        &mut self,
        wall_ns: u64,
        observed: HybridTimestamp,
    ) -> Result<HybridTimestamp, Refusal> {
        let latest = larger(wall_ns, larger(self.last.wall_ns, observed.wall_ns));
        let counter = if latest == self.last.wall_ns && latest == observed.wall_ns {
            larger(self.last.counter, observed.counter).checked_add(1)
        } else if latest == self.last.wall_ns {
            self.last.counter.checked_add(1)
        } else if latest == observed.wall_ns {
            observed.counter.checked_add(1)
        } else {
            Some(0)
        };
        let Some(counter) = counter else {
            return Err(Refusal::Exhausted);
        };

        self.last = HybridTimestamp {
            wall_ns: latest,
            counter,
        };
        Ok(self.last)
    }

    /// Length of the [`encode`](Self::encode) frame.
    pub const ENCODED_LEN: usize = TAG_LEN + TIMESTAMP_LEN;

    encoders!("clock");

    /// Fills a frame of exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    fn write_frame(&self, frame: &mut Writer<'_>) -> Result<(), Refusal> {
        frame.byte(TAG_HYBRID_CLOCK)?;
        self.last.write(frame.take(TIMESTAMP_LEN)?)
    }

    /// Reads a clock written by [`encode`](Self::encode).
    ///
    /// # Errors
    ///
    /// [`Refusal::Truncated`] when `bytes` is too short,
    /// [`Refusal::WrongStructure`] when it holds another structure.
    pub fn decode(bytes: &[u8]) -> Result<Self, Refusal> {
        let mut clock = Self::new();
        clock.decode_into(bytes)?;
        Ok(clock)
    }

    /// Reads a frame over a clock the caller already holds.
    ///
    /// The clock is untouched when the frame is rejected.
    ///
    /// # Errors
    ///
    /// The same as [`decode`](Self::decode).
    pub fn decode_into(&mut self, bytes: &[u8]) -> Result<(), Refusal> {
        let mut frame = tagged_frame(bytes, Self::ENCODED_LEN, TAG_HYBRID_CLOCK)?;
        self.last = HybridTimestamp::read(frame.take(TIMESTAMP_LEN)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
