use crate::{HybridClock, HybridTimestamp};

const MS: u64 = 1_000_000;

#[test]
fn timestamps_keep_rising_while_the_wall_clock_falls() {
    let mut clock = HybridClock::new();
    let mut previous = clock.tick(1_000 * MS);

    for step in 1..1_000 {
        let stamp = clock.tick(1_000 * MS - step * MS);
        assert!(stamp > previous, "step {step}: {stamp:?} <= {previous:?}");
        previous = stamp;
    }
}

#[test]
fn a_stalled_wall_clock_is_carried_by_the_counter() {
    let mut clock = HybridClock::new();

    let first = clock.tick(7 * MS);
    let second = clock.tick(7 * MS);

    assert_eq!(first.wall_ns, second.wall_ns);
    assert_eq!(second.counter, first.counter + 1);
}

#[test]
fn an_advancing_wall_clock_resets_the_counter() {
    let mut clock = HybridClock::new();
    clock.tick(7 * MS);
    clock.tick(7 * MS);

    let moved = clock.tick(8 * MS);

    assert_eq!(
        moved,
        HybridTimestamp {
            wall_ns: 8 * MS,
            counter: 0
        }
    );
}

#[test]
fn receiving_is_stamped_after_both_sides() {
    let mut ahead = HybridClock::new();
    let mut behind = HybridClock::new();

    let sent = ahead.tick(900 * MS);
    let local = behind.tick(100 * MS);
    let stamp = behind.observe(100 * MS, sent);

    assert!(stamp > sent);
    assert!(stamp > local);
}

#[test]
fn a_message_round_trip_never_loses_causality() {
    let mut left = HybridClock::new();
    let mut right = HybridClock::new();

    let mut previous = HybridTimestamp::default();
    for round in 0..200 {
        let wall = 500 * MS - round * MS;
        let sent = left.tick(wall);
        let received = right.observe(wall / 2, sent);
        let replied = left.observe(wall, received);

        assert!(received > sent, "round {round}");
        assert!(replied > received, "round {round}");
        assert!(replied > previous, "round {round}");
        previous = replied;
    }
}

#[test]
fn ordering_is_by_reading_then_counter() {
    let early = HybridTimestamp {
        wall_ns: 10 * MS,
        counter: 9,
    };
    let late = HybridTimestamp {
        wall_ns: 11 * MS,
        counter: 0,
    };

    assert!(late > early);
    assert!(
        HybridTimestamp {
            wall_ns: 10 * MS,
            counter: 10
        } > early
    );
}
