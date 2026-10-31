use crate::{CodecError, Merge, PNCounter};

#[test]
fn the_value_is_what_was_added_minus_what_was_taken() {
    let mut counter = PNCounter::<4>::new();

    assert_eq!(counter.increment(1, 10), Ok(()));
    assert_eq!(counter.decrement(1, 3), Ok(()));
    assert_eq!(counter.decrement(2, 2), Ok(()));

    assert_eq!(counter.added(), 10);
    assert_eq!(counter.removed(), 5);
    assert_eq!(counter.value(), 5);
}

#[test]
fn the_value_goes_below_zero() {
    let mut counter = PNCounter::<2>::new();

    assert_eq!(counter.decrement(1, 7), Ok(()));

    assert_eq!(counter.value(), -7);
}

#[test]
fn an_increment_and_a_decrement_elsewhere_converge_either_way() {
    let mut left = PNCounter::<4>::new();
    let mut right = PNCounter::<4>::new();
    assert_eq!(left.increment(1, 5), Ok(()));
    assert_eq!(right.decrement(2, 8), Ok(()));

    let mut forwards = left;
    assert_eq!(forwards.merge(&right), Ok(()));
    let mut backwards = right;
    assert_eq!(backwards.merge(&left), Ok(()));

    assert_eq!(forwards, backwards);
    assert_eq!(forwards.value(), -3);
}

#[test]
fn a_decrement_the_other_side_has_not_seen_is_not_lost() {
    let mut replica = PNCounter::<2>::new();
    assert_eq!(replica.increment(1, 4), Ok(()));
    let mut peer = replica;
    assert_eq!(peer.decrement(2, 1), Ok(()));

    assert_eq!(replica.merge(&peer), Ok(()));
    assert_eq!(replica.merge(&peer), Ok(()));

    assert_eq!(replica.value(), 3);
}

/// Length of a four-replica counter's frame.
const FRAME_LEN: usize = PNCounter::<4>::ENCODED_LEN;

#[test]
fn a_frame_round_trips_what_went_up_and_what_came_down() {
    let mut counter = PNCounter::<4>::new();
    assert_eq!(counter.increment(1, 10), Ok(()));
    assert_eq!(counter.decrement(2, 4), Ok(()));

    let mut frame = [0_u8; FRAME_LEN];
    assert_eq!(counter.encode(&mut frame), Ok(FRAME_LEN));

    let restored = PNCounter::<4>::decode(&frame);
    assert_eq!(restored, Ok(counter));
    assert_eq!(restored.map(|counter| counter.value()), Ok(6));
}

#[test]
fn a_balance_is_not_clamped_by_the_total_of_either_side() {
    let mut counter = PNCounter::<4>::new();
    assert_eq!(counter.increment(1, u64::MAX), Ok(()));
    assert_eq!(counter.increment(2, u64::MAX), Ok(()));
    assert_eq!(counter.decrement(3, u64::MAX), Ok(()));

    assert_eq!(counter.added(), u128::from(u64::MAX) + u128::from(u64::MAX));
    assert_eq!(counter.removed(), u128::from(u64::MAX));
    assert_eq!(counter.value(), i128::from(u64::MAX));

    let mut frame = [0_u8; FRAME_LEN];
    assert_eq!(counter.encode(&mut frame), Ok(FRAME_LEN));

    assert_eq!(
        PNCounter::<4>::decode(&frame).map(|restored| restored.value()),
        Ok(i128::from(u64::MAX))
    );
}

#[test]
fn encoding_into_a_short_buffer_is_refused() {
    let mut frame = [0_u8; FRAME_LEN - 1];

    assert_eq!(
        PNCounter::<4>::new().encode(&mut frame),
        Err(CodecError::Truncated)
    );
}

#[test]
fn a_short_frame_is_refused() {
    assert_eq!(PNCounter::<4>::decode(&[]), Err(CodecError::Truncated));
}

#[test]
fn another_structures_frame_is_refused() {
    let mut frame = [0_u8; FRAME_LEN];
    assert_eq!(PNCounter::<4>::new().encode(&mut frame), Ok(FRAME_LEN));
    frame[crate::TAG] = crate::TAG_GCOUNTER;

    assert_eq!(PNCounter::<4>::decode(&frame), Err(CodecError::Tag));
}

#[test]
fn a_frame_written_at_another_replica_count_is_refused() {
    let mut frame = [0_u8; PNCounter::<8>::ENCODED_LEN];
    assert_eq!(
        PNCounter::<8>::new().encode(&mut frame),
        Ok(PNCounter::<8>::ENCODED_LEN)
    );

    assert_eq!(PNCounter::<4>::decode(&frame), Err(CodecError::Parameters));
}

#[test]
fn a_refused_frame_leaves_the_counter_alone() {
    let mut counter = PNCounter::<4>::new();
    assert_eq!(counter.decrement(1, 5), Ok(()));
    let before = counter;

    assert_eq!(counter.decode_into(&[]), Err(CodecError::Truncated));

    assert_eq!(counter, before);
}

#[cfg(feature = "alloc")]
#[test]
fn the_allocated_frame_is_the_frame() {
    let mut counter = PNCounter::<4>::new();
    assert_eq!(counter.increment(3, 9), Ok(()));
    assert_eq!(counter.decrement(4, 1), Ok(()));

    let mut frame = [0_u8; FRAME_LEN];
    assert_eq!(counter.encode(&mut frame), Ok(FRAME_LEN));

    assert_eq!(*counter.encoded(), frame);
}
