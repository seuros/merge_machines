use crate::{LogicalClock, Refusal};

/// What a test that stamps events returns.
type Stamped = Result<(), Refusal>;

#[test]
fn local_events_are_strictly_increasing() -> Stamped {
    let mut clock = LogicalClock::new();

    let mut previous = clock.now();
    for _ in 0..1_000 {
        let stamp = clock.tick()?;
        assert!(stamp > previous);
        previous = stamp;
    }
    assert_eq!(clock.now(), previous);
    Ok(())
}

#[test]
fn observing_a_later_timestamp_jumps_past_it() -> Stamped {
    let mut clock = LogicalClock::new();
    clock.tick()?;

    assert_eq!(clock.observe(500), Ok(501));
    assert_eq!(clock.now(), 501);
    Ok(())
}

#[test]
fn observing_an_earlier_timestamp_still_advances() -> Stamped {
    let mut clock = LogicalClock::new();
    for _ in 0..10 {
        clock.tick()?;
    }

    assert_eq!(clock.observe(2), Ok(11));
    Ok(())
}

#[test]
fn a_send_is_always_stamped_before_the_matching_receive() -> Stamped {
    let mut sender = LogicalClock::new();
    let mut receiver = LogicalClock::new();

    for round in 0..100 {
        let sent_at = sender.tick()?;
        let received_at = receiver.observe(sent_at)?;
        assert!(received_at > sent_at, "round {round}");

        let replied_at = sender.observe(receiver.tick()?)?;
        assert!(replied_at > received_at, "round {round}");
    }
    Ok(())
}

#[test]
fn a_counter_with_nothing_left_refuses_to_reissue_its_last_timestamp() {
    let mut clock = LogicalClock::new();

    assert_eq!(clock.observe(u64::MAX - 1), Ok(u64::MAX));
    assert_eq!(clock.tick(), Err(Refusal::Exhausted));
    assert_eq!(clock.observe(7), Err(Refusal::Exhausted));
    assert_eq!(clock.now(), u64::MAX);

    let mut fresh = LogicalClock::new();

    assert_eq!(fresh.observe(u64::MAX), Err(Refusal::Exhausted));
    assert_eq!(fresh.now(), 0);
}
