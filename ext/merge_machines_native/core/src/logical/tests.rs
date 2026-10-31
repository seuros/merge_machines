use crate::LogicalClock;

#[test]
fn local_events_are_strictly_increasing() {
    let mut clock = LogicalClock::new();

    let mut previous = clock.now();
    for _ in 0..1_000 {
        let stamp = clock.tick();
        assert!(stamp > previous);
        previous = stamp;
    }
    assert_eq!(clock.now(), previous);
}

#[test]
fn observing_a_later_timestamp_jumps_past_it() {
    let mut clock = LogicalClock::new();
    clock.tick();

    assert_eq!(clock.observe(500), 501);
    assert_eq!(clock.now(), 501);
}

#[test]
fn observing_an_earlier_timestamp_still_advances() {
    let mut clock = LogicalClock::new();
    for _ in 0..10 {
        clock.tick();
    }

    assert_eq!(clock.observe(2), 11);
}

#[test]
fn a_send_is_always_stamped_before_the_matching_receive() {
    let mut sender = LogicalClock::new();
    let mut receiver = LogicalClock::new();

    for round in 0..100 {
        let sent_at = sender.tick();
        let received_at = receiver.observe(sent_at);
        assert!(received_at > sent_at, "round {round}");

        let replied_at = sender.observe(receiver.tick());
        assert!(replied_at > received_at, "round {round}");
    }
}
