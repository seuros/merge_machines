use crate::{CapacityError, CodecError, GCounter, Merge};

#[test]
fn the_value_is_the_sum_of_every_replicas_count() {
    let mut counter = GCounter::<4>::new();

    assert_eq!(counter.increment(7, 2), Ok(()));
    assert_eq!(counter.increment(9, 5), Ok(()));
    assert_eq!(counter.increment(7, 1), Ok(()));

    assert_eq!(counter.count(7), 3);
    assert_eq!(counter.count(9), 5);
    assert_eq!(counter.count(11), 0);
    assert_eq!(counter.value(), 8);
}

#[test]
fn concurrent_increments_converge_whichever_way_they_merge() {
    let mut left = GCounter::<4>::new();
    let mut right = GCounter::<4>::new();
    assert_eq!(left.increment(1, 4), Ok(()));
    assert_eq!(right.increment(2, 6), Ok(()));

    let mut forwards = left;
    assert_eq!(forwards.merge(&right), Ok(()));
    let mut backwards = right;
    assert_eq!(backwards.merge(&left), Ok(()));

    assert_eq!(forwards, backwards);
    assert_eq!(forwards.value(), 10);
}

#[test]
fn redelivering_a_replicas_count_does_not_count_it_twice() {
    let mut source = GCounter::<2>::new();
    assert_eq!(source.increment(1, 9), Ok(()));

    let mut sink = GCounter::<2>::new();
    for _ in 0..10 {
        assert_eq!(sink.merge(&source), Ok(()));
    }

    assert_eq!(sink.value(), 9);
}

#[test]
fn a_stale_state_never_lowers_a_count() {
    let mut stale = GCounter::<2>::new();
    assert_eq!(stale.increment(1, 1), Ok(()));
    let mut current = stale;
    assert_eq!(current.increment(1, 100), Ok(()));

    assert_eq!(current.merge(&stale), Ok(()));

    assert_eq!(current.count(1), 101);
}

#[test]
fn an_unseen_replica_needs_a_free_slot() {
    let mut counter = GCounter::<2>::new();
    assert_eq!(counter.increment(1, 1), Ok(()));
    assert_eq!(counter.increment(2, 1), Ok(()));

    assert_eq!(counter.increment(3, 1), Err(CapacityError));
    assert_eq!(counter.increment(1, 1), Ok(()));
}

#[test]
fn merging_more_replicas_than_there_are_slots_reports_capacity() {
    let mut left = GCounter::<1>::new();
    let mut right = GCounter::<1>::new();
    assert_eq!(left.increment(1, 1), Ok(()));
    assert_eq!(right.increment(2, 1), Ok(()));

    assert_eq!(left.merge(&right), Err(CapacityError));
}

/// Length of a four-replica counter's frame.
const FRAME_LEN: usize = GCounter::<4>::ENCODED_LEN;

#[test]
fn a_frame_round_trips_every_replicas_count() {
    let mut counter = GCounter::<4>::new();
    assert_eq!(counter.increment(7, 2), Ok(()));
    assert_eq!(counter.increment(9, 5), Ok(()));

    let mut frame = [0_u8; FRAME_LEN];
    assert_eq!(counter.encode(&mut frame), Ok(FRAME_LEN));

    let restored = GCounter::<4>::decode(&frame);
    assert_eq!(restored, Ok(counter));
    assert_eq!(restored.map(|counter| counter.value()), Ok(7));
}

#[test]
fn encoding_into_a_short_buffer_is_refused() {
    let mut frame = [0_u8; FRAME_LEN - 1];

    assert_eq!(
        GCounter::<4>::new().encode(&mut frame),
        Err(CodecError::Truncated)
    );
}

#[test]
fn a_short_frame_is_refused() {
    assert_eq!(GCounter::<4>::decode(&[]), Err(CodecError::Truncated));
}

#[test]
fn another_structures_frame_is_refused() {
    let mut frame = [0_u8; FRAME_LEN];
    assert_eq!(GCounter::<4>::new().encode(&mut frame), Ok(FRAME_LEN));
    frame[crate::TAG] = crate::TAG_GSET;

    assert_eq!(GCounter::<4>::decode(&frame), Err(CodecError::Tag));
}

#[test]
fn a_frame_written_at_another_replica_count_is_refused() {
    let mut frame = [0_u8; GCounter::<8>::ENCODED_LEN];
    assert_eq!(
        GCounter::<8>::new().encode(&mut frame),
        Ok(GCounter::<8>::ENCODED_LEN)
    );

    assert_eq!(GCounter::<4>::decode(&frame), Err(CodecError::Parameters));
}

#[test]
fn a_refused_frame_leaves_the_counter_alone() {
    let mut counter = GCounter::<4>::new();
    assert_eq!(counter.increment(1, 5), Ok(()));
    let before = counter;

    assert_eq!(counter.decode_into(&[]), Err(CodecError::Truncated));

    assert_eq!(counter, before);
}

#[cfg(feature = "alloc")]
#[test]
fn the_allocated_frame_is_the_frame() {
    let mut counter = GCounter::<4>::new();
    assert_eq!(counter.increment(3, 9), Ok(()));

    let mut frame = [0_u8; FRAME_LEN];
    assert_eq!(counter.encode(&mut frame), Ok(FRAME_LEN));

    assert_eq!(*counter.encoded(), frame);
}
