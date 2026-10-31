use crate::{ClockExhausted, CodecError, HybridClock, HybridTimestamp};

const MS: u64 = 1_000_000;

/// What a test that stamps events returns.
type Stamped = Result<(), ClockExhausted>;

#[test]
fn timestamps_keep_rising_while_the_wall_clock_falls() -> Stamped {
    let mut clock = HybridClock::new();
    let mut previous = clock.tick(1_000 * MS)?;

    for step in 1..1_000 {
        let stamp = clock.tick(1_000 * MS - step * MS)?;
        assert!(stamp > previous, "step {step}: {stamp:?} <= {previous:?}");
        previous = stamp;
    }
    Ok(())
}

#[test]
fn a_stalled_wall_clock_is_carried_by_the_counter() -> Stamped {
    let mut clock = HybridClock::new();

    let first = clock.tick(7 * MS)?;
    let second = clock.tick(7 * MS)?;

    assert_eq!(first.wall_ns, second.wall_ns);
    assert_eq!(second.counter, first.counter + 1);
    Ok(())
}

#[test]
fn an_advancing_wall_clock_resets_the_counter() -> Stamped {
    let mut clock = HybridClock::new();
    clock.tick(7 * MS)?;
    clock.tick(7 * MS)?;

    assert_eq!(
        clock.tick(8 * MS),
        Ok(HybridTimestamp {
            wall_ns: 8 * MS,
            counter: 0
        })
    );
    Ok(())
}

#[test]
fn receiving_is_stamped_after_both_sides() -> Stamped {
    let mut ahead = HybridClock::new();
    let mut behind = HybridClock::new();

    let sent = ahead.tick(900 * MS)?;
    let local = behind.tick(100 * MS)?;
    let stamp = behind.observe(100 * MS, sent)?;

    assert!(stamp > sent);
    assert!(stamp > local);
    Ok(())
}

#[test]
fn a_message_round_trip_never_loses_causality() -> Stamped {
    let mut left = HybridClock::new();
    let mut right = HybridClock::new();

    let mut previous = HybridTimestamp::default();
    for round in 0..200 {
        let wall = 500 * MS - round * MS;
        let sent = left.tick(wall)?;
        let received = right.observe(wall / 2, sent)?;
        let replied = left.observe(wall, received)?;

        assert!(received > sent, "round {round}");
        assert!(replied > received, "round {round}");
        assert!(replied > previous, "round {round}");
        previous = replied;
    }
    Ok(())
}

#[test]
fn a_counter_with_nothing_left_refuses_to_reissue_its_last_timestamp() {
    let crowded = HybridTimestamp {
        wall_ns: 7 * MS,
        counter: u64::MAX - 1,
    };
    let full = HybridTimestamp {
        wall_ns: 7 * MS,
        counter: u64::MAX,
    };
    let mut clock = HybridClock::new();

    assert_eq!(clock.observe(0, crowded), Ok(full));
    assert_eq!(clock.tick(7 * MS), Err(ClockExhausted));
    assert_eq!(clock.tick(2 * MS), Err(ClockExhausted));
    assert_eq!(clock.observe(0, crowded), Err(ClockExhausted));
    assert_eq!(clock.last(), full);

    assert_eq!(
        clock.tick(8 * MS),
        Ok(HybridTimestamp {
            wall_ns: 8 * MS,
            counter: 0
        })
    );

    let mut fresh = HybridClock::new();

    assert_eq!(fresh.observe(0, full), Err(ClockExhausted));
    assert_eq!(fresh.last(), HybridTimestamp::default());
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

#[test]
fn a_frame_round_trips_the_last_timestamp_issued() -> Stamped {
    let mut clock = HybridClock::new();
    clock.tick(900)?;
    clock.tick(900)?;

    let mut frame = [0_u8; HybridClock::ENCODED_LEN];
    assert_eq!(clock.encode(&mut frame), Ok(HybridClock::ENCODED_LEN));

    let restored = HybridClock::decode(&frame);
    assert_eq!(restored, Ok(clock));
    assert_eq!(
        restored.map(|clock| clock.last()),
        Ok(HybridTimestamp {
            wall_ns: 900,
            counter: 1,
        })
    );
    Ok(())
}

#[test]
fn encoding_into_a_short_buffer_is_refused() {
    let mut frame = [0_u8; HybridClock::ENCODED_LEN - 1];

    assert_eq!(
        HybridClock::new().encode(&mut frame),
        Err(CodecError::Truncated)
    );
}

#[test]
fn a_short_frame_is_refused() {
    assert_eq!(HybridClock::decode(&[]), Err(CodecError::Truncated));
}

#[test]
fn another_structures_frame_is_refused() {
    let mut frame = [0_u8; HybridClock::ENCODED_LEN];
    assert_eq!(
        HybridClock::new().encode(&mut frame),
        Ok(HybridClock::ENCODED_LEN)
    );
    frame[crate::TAG] = crate::TAG_LOGICAL_CLOCK;

    assert_eq!(HybridClock::decode(&frame), Err(CodecError::Tag));
}

#[test]
fn a_refused_frame_leaves_the_clock_alone() -> Stamped {
    let mut clock = HybridClock::new();
    clock.tick(5)?;
    let before = clock;

    assert_eq!(clock.decode_into(&[]), Err(CodecError::Truncated));

    assert_eq!(clock, before);
    Ok(())
}

#[cfg(feature = "alloc")]
#[test]
fn the_allocated_frame_is_the_frame() -> Stamped {
    let mut clock = HybridClock::new();
    clock.tick(17)?;

    let mut frame = [0_u8; HybridClock::ENCODED_LEN];
    assert_eq!(clock.encode(&mut frame), Ok(HybridClock::ENCODED_LEN));

    assert_eq!(*clock.encoded(), frame);
    Ok(())
}
