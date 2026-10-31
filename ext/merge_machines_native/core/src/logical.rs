//! Lamport's logical clock: ordering events without reading a clock.

/// A counter that orders events across replicas, with no clock involved.
///
/// From Lamport (1978), "Time, Clocks, and the Ordering of Events in a
/// Distributed System". A local event advances the counter by one. An observed
/// timestamp from another replica pushes the counter past it, so a message is
/// always stamped later than the send it answers.
///
/// If event `a` happened before event `b` then `a`'s timestamp is lower. The
/// converse does not hold: a lower timestamp does not prove happens-before,
/// because two replicas that never talked can issue the same number. That is
/// what vector clocks buy and this does not.
///
/// ```
/// use merge_machines::LogicalClock;
///
/// let mut sender = LogicalClock::new();
/// let mut receiver = LogicalClock::new();
///
/// let sent_at = sender.tick();
/// assert_eq!(sent_at, 1);
///
/// receiver.tick();
/// receiver.tick();
///
/// // Receiving is stamped after both the local history and the send.
/// assert_eq!(receiver.observe(sent_at), 3);
/// assert_eq!(receiver.observe(99), 100);
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LogicalClock {
    counter: u64,
}

impl LogicalClock {
    #[must_use]
    pub const fn new() -> Self {
        Self { counter: 0 }
    }

    /// The last timestamp this clock issued.
    #[must_use]
    pub const fn now(&self) -> u64 {
        self.counter
    }

    /// Stamps a local event.
    pub const fn tick(&mut self) -> u64 {
        self.counter = self.counter.saturating_add(1);
        self.counter
    }

    /// Stamps the receipt of `observed`, a timestamp from another replica.
    pub const fn observe(&mut self, observed: u64) -> u64 {
        if observed > self.counter {
            self.counter = observed;
        }
        self.tick()
    }
}

#[cfg(test)]
mod tests;
