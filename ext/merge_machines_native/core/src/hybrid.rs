//! A hybrid logical clock: timestamps near physical time that still order.

/// A wall-clock reading paired with a counter that breaks its ties.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct HybridTimestamp {
    /// The wall-clock reading this was issued at, in nanoseconds.
    pub wall_ns: u64,
    /// How many timestamps have been issued at this reading.
    pub counter: u64,
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
/// use merge_machines::HybridClock;
///
/// const MS: u64 = 1_000_000;
/// let mut clock = HybridClock::new();
///
/// let first = clock.tick(10 * MS);
/// assert_eq!(first.counter, 0);
///
/// // Same reading twice: the counter separates them.
/// let second = clock.tick(10 * MS);
/// assert!(second > first);
///
/// // The wall clock is corrected backwards; the timestamps are not.
/// let third = clock.tick(4 * MS);
/// assert!(third > second);
/// assert_eq!(third.wall_ns, 10 * MS);
///
/// // A reading that really did advance resets the counter.
/// let fourth = clock.tick(11 * MS);
/// assert_eq!(fourth.counter, 0);
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct HybridClock {
    last: HybridTimestamp,
}

const fn larger(left: u64, right: u64) -> u64 {
    if left > right { left } else { right }
}

impl HybridClock {
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
    pub const fn tick(&mut self, wall_ns: u64) -> HybridTimestamp {
        if wall_ns > self.last.wall_ns {
            self.last = HybridTimestamp {
                wall_ns,
                counter: 0,
            };
        } else {
            self.last.counter = self.last.counter.saturating_add(1);
        }
        self.last
    }

    /// Stamps the receipt of `observed` at wall-clock reading `wall_ns`.
    pub const fn observe(&mut self, wall_ns: u64, observed: HybridTimestamp) -> HybridTimestamp {
        let latest = larger(wall_ns, larger(self.last.wall_ns, observed.wall_ns));
        let counter = if latest == self.last.wall_ns && latest == observed.wall_ns {
            larger(self.last.counter, observed.counter).saturating_add(1)
        } else if latest == self.last.wall_ns {
            self.last.counter.saturating_add(1)
        } else if latest == observed.wall_ns {
            observed.counter.saturating_add(1)
        } else {
            0
        };

        self.last = HybridTimestamp {
            wall_ns: latest,
            counter,
        };
        self.last
    }
}

#[cfg(test)]
mod tests;
