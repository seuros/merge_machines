use crate::{CodecError, LogicalClock};

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

#[test]
fn a_frame_round_trips_the_counter() {
    let mut clock = LogicalClock::new();
    clock.tick();
    clock.observe(41);

    let mut frame = [0_u8; LogicalClock::ENCODED_LEN];
    assert_eq!(clock.encode(&mut frame), Ok(LogicalClock::ENCODED_LEN));

    let restored = LogicalClock::decode(&frame);
    assert_eq!(restored, Ok(clock));
    assert_eq!(restored.map(|clock| clock.now()), Ok(42));
}

#[test]
fn encoding_into_a_short_buffer_is_refused() {
    let mut frame = [0_u8; LogicalClock::ENCODED_LEN - 1];

    assert_eq!(
        LogicalClock::new().encode(&mut frame),
        Err(CodecError::Truncated)
    );
}

#[test]
fn a_short_frame_is_refused() {
    assert_eq!(LogicalClock::decode(&[]), Err(CodecError::Truncated));
}

#[test]
fn another_structures_frame_is_refused() {
    let mut frame = [0_u8; LogicalClock::ENCODED_LEN];
    assert_eq!(
        LogicalClock::new().encode(&mut frame),
        Ok(LogicalClock::ENCODED_LEN)
    );
    frame[crate::TAG] = crate::TAG_HYBRID_CLOCK;

    assert_eq!(LogicalClock::decode(&frame), Err(CodecError::Tag));
}

#[test]
fn a_refused_frame_leaves_the_clock_alone() {
    let mut clock = LogicalClock::new();
    clock.tick();
    let before = clock;

    assert_eq!(clock.decode_into(&[]), Err(CodecError::Truncated));

    assert_eq!(clock, before);
}

#[cfg(feature = "alloc")]
#[test]
fn the_allocated_frame_is_the_frame() {
    let mut clock = LogicalClock::new();
    clock.tick();

    let mut frame = [0_u8; LogicalClock::ENCODED_LEN];
    assert_eq!(clock.encode(&mut frame), Ok(LogicalClock::ENCODED_LEN));

    assert_eq!(*clock.encoded(), frame);
}
