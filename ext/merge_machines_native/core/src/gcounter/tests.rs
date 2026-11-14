use crate::{GCounter, Merge, Refusal};
use matryona_machines::testing::refuses;

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

    refuses(&mut counter, Refusal::Full, |c| c.increment(3, 1));
    assert_eq!(counter.increment(1, 1), Ok(()));
}

#[test]
fn a_count_that_would_pass_the_maximum_is_refused_whole() {
    let mut counter = GCounter::<2>::new();
    assert_eq!(counter.increment(1, u64::MAX - 1), Ok(()));

    refuses(&mut counter, Refusal::Overflow, |c| c.increment(1, 2));
    assert_eq!(counter.count(1), u64::MAX - 1);
    assert_eq!(counter.increment(1, 1), Ok(()));
}

#[test]
fn adding_zero_claims_no_slot() {
    let mut counter = GCounter::<1>::new();
    assert_eq!(counter.increment(1, 0), Ok(()));
    assert_eq!(counter.entries().count(), 0);
    assert_eq!(counter.increment(2, 1), Ok(()));
    assert_eq!(counter.increment(3, 0), Ok(()));
}

#[test]
fn a_refused_merge_raises_no_count_and_claims_no_slot() {
    let mut left = GCounter::<2>::new();
    assert_eq!(left.increment(1, 1), Ok(()));
    let mut right = GCounter::<2>::new();
    assert_eq!(right.increment(1, 50), Ok(()));
    assert_eq!(right.increment(2, 1), Ok(()));
    let mut crowded = GCounter::<2>::new();
    assert_eq!(crowded.increment(3, 1), Ok(()));
    assert_eq!(crowded.increment(4, 1), Ok(()));

    refuses(&mut left, Refusal::Full, |l| l.merge(&crowded));
    let mut both = crowded;
    refuses(&mut both, Refusal::Full, |b| b.merge(&right));
    assert_eq!(both.count(1), 0, "the raise must not land without the slot");
}

#[test]
fn merging_more_replicas_than_there_are_slots_reports_capacity() {
    let mut left = GCounter::<1>::new();
    let mut right = GCounter::<1>::new();
    assert_eq!(left.increment(1, 1), Ok(()));
    assert_eq!(right.increment(2, 1), Ok(()));

    assert_eq!(left.merge(&right), Err(Refusal::Full));
}

/// Length of a four-replica counter's frame.
const FRAME_LEN: usize = GCounter::<4>::ENCODED_LEN;

/// A four-replica frame holding `records`, with its count field saying so.
fn frame_of(records: &[(u64, u64)]) -> [u8; FRAME_LEN] {
    let mut frame = [0_u8; FRAME_LEN];
    assert_eq!(GCounter::<4>::new().encode(&mut frame), Ok(FRAME_LEN));
    frame[3] = u8::try_from(records.len()).unwrap_or(u8::MAX);
    let (slots, _) = frame[5..].as_chunks_mut::<16>();
    for (slot, (replica, count)) in slots.iter_mut().zip(records) {
        slot[..8].copy_from_slice(&replica.to_le_bytes());
        slot[8..].copy_from_slice(&count.to_le_bytes());
    }
    frame
}

#[test]
fn a_frame_no_sequence_of_calls_produces_is_malformed() {
    assert!(GCounter::<4>::decode(&frame_of(&[(1, 1), (2, 2)])).is_ok());
    for records in [
        &[(1, 1), (1, 2)][..],
        &[(1, 0)][..],
        &[(1, 1), (2, 1), (3, 1), (4, 1), (5, 1)][..],
    ] {
        assert_eq!(
            GCounter::<4>::decode(&frame_of(records)),
            Err(Refusal::Malformed),
            "{records:?}"
        );
    }
}
