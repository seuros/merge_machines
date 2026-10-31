use crate::{CapacityError, CodecError, Merge, OrSet, Tag};

#[test]
fn an_element_leaves_once_its_tags_are_tombstoned() {
    let mut set = OrSet::<u8, 4>::new();
    assert_eq!(set.add(1, Tag::new(7, 1)), Ok(()));
    assert!(set.contains(&1));

    assert_eq!(set.remove(&1), Ok(()));

    assert!(!set.contains(&1));
    assert!(set.is_empty());
}

#[test]
fn re_adding_under_a_new_tag_brings_the_element_back() {
    let mut set = OrSet::<u8, 4>::new();
    assert_eq!(set.add(1, Tag::new(7, 1)), Ok(()));
    assert_eq!(set.remove(&1), Ok(()));

    assert_eq!(set.add(1, Tag::new(7, 2)), Ok(()));

    assert!(set.contains(&1));
    assert_eq!(set.len(), 1);
}

#[test]
fn a_concurrent_add_survives_a_remove_that_never_saw_it() {
    let mut shared = OrSet::<u8, 8>::new();
    assert_eq!(shared.add(1, Tag::new(1, 1)), Ok(()));

    let mut remover = shared;
    let mut adder = shared;
    assert_eq!(remover.remove(&1), Ok(()));
    assert_eq!(adder.add(1, Tag::new(2, 1)), Ok(()));

    let mut forwards = remover;
    assert_eq!(forwards.merge(&adder), Ok(()));
    let mut backwards = adder;
    assert_eq!(backwards.merge(&remover), Ok(()));

    assert_eq!(forwards, backwards);
    assert!(forwards.contains(&1));
    assert_eq!(forwards.len(), 1);
}

#[test]
fn a_remove_that_saw_every_tag_wins_from_both_sides() {
    let mut shared = OrSet::<u8, 8>::new();
    assert_eq!(shared.add(1, Tag::new(1, 1)), Ok(()));
    assert_eq!(shared.add(1, Tag::new(2, 1)), Ok(()));

    let mut remover = shared;
    assert_eq!(remover.remove(&1), Ok(()));

    let mut forwards = remover;
    assert_eq!(forwards.merge(&shared), Ok(()));
    let mut backwards = shared;
    assert_eq!(backwards.merge(&remover), Ok(()));

    assert_eq!(forwards, backwards);
    assert!(!forwards.contains(&1));
}

#[test]
fn an_element_added_twice_is_listed_once() {
    let mut set = OrSet::<u8, 8>::new();
    assert_eq!(set.add(1, Tag::new(1, 1)), Ok(()));
    assert_eq!(set.add(1, Tag::new(2, 1)), Ok(()));
    assert_eq!(set.add(2, Tag::new(1, 2)), Ok(()));

    assert_eq!(set.len(), 2);
    assert_eq!(set.iter().filter(|element| *element == 1).count(), 1);
}

#[test]
fn the_same_tag_twice_claims_one_slot() {
    let mut set = OrSet::<u8, 1>::new();
    assert_eq!(set.add(1, Tag::new(1, 1)), Ok(()));

    assert_eq!(set.add(1, Tag::new(1, 1)), Ok(()));
    assert_eq!(set.add(1, Tag::new(1, 2)), Err(CapacityError));
}

#[test]
fn merging_more_adds_than_there_are_slots_reports_capacity() {
    let mut left = OrSet::<u8, 1>::new();
    let mut right = OrSet::<u8, 1>::new();
    assert_eq!(left.add(1, Tag::new(1, 1)), Ok(()));
    assert_eq!(right.add(2, Tag::new(2, 1)), Ok(()));

    assert_eq!(left.merge(&right), Err(CapacityError));
}

/// Length of a four-slot set's frame.
const FRAME_LEN: usize = OrSet::<u64, 4>::ENCODED_LEN;

#[test]
fn a_frame_round_trips_every_add_and_every_tombstone() {
    let mut set = OrSet::<u64, 4>::new();
    assert_eq!(set.add(11, Tag::new(1, 1)), Ok(()));
    assert_eq!(set.add(22, Tag::new(1, 2)), Ok(()));
    assert_eq!(set.remove(&11), Ok(()));

    let mut frame = [0_u8; FRAME_LEN];
    assert_eq!(set.encode(&mut frame), Ok(FRAME_LEN));

    let restored = OrSet::<u64, 4>::decode(&frame);
    assert_eq!(restored, Ok(set));
    assert_eq!(restored.map(|set| set.contains(&22)), Ok(true));
    assert_eq!(restored.map(|set| set.contains(&11)), Ok(false));
}

#[test]
fn encoding_into_a_short_buffer_is_refused() {
    let mut frame = [0_u8; FRAME_LEN - 1];

    assert_eq!(
        OrSet::<u64, 4>::new().encode(&mut frame),
        Err(CodecError::Truncated)
    );
}

#[test]
fn a_short_frame_is_refused() {
    assert_eq!(OrSet::<u64, 4>::decode(&[]), Err(CodecError::Truncated));
}

#[test]
fn another_structures_frame_is_refused() {
    let mut frame = [0_u8; FRAME_LEN];
    assert_eq!(OrSet::<u64, 4>::new().encode(&mut frame), Ok(FRAME_LEN));
    frame[crate::TAG] = crate::TAG_GSET;

    assert_eq!(OrSet::<u64, 4>::decode(&frame), Err(CodecError::Tag));
}

#[test]
fn a_frame_written_at_another_capacity_is_refused() {
    let mut frame = [0_u8; OrSet::<u64, 8>::ENCODED_LEN];
    assert_eq!(
        OrSet::<u64, 8>::new().encode(&mut frame),
        Ok(OrSet::<u64, 8>::ENCODED_LEN)
    );

    assert_eq!(OrSet::<u64, 4>::decode(&frame), Err(CodecError::Parameters));
}

#[test]
fn a_refused_frame_leaves_the_set_alone() {
    let mut set = OrSet::<u64, 4>::new();
    assert_eq!(set.add(5, Tag::new(1, 1)), Ok(()));
    let before = set;

    assert_eq!(set.decode_into(&[]), Err(CodecError::Truncated));

    assert_eq!(set, before);
}

#[cfg(feature = "alloc")]
#[test]
fn the_allocated_frame_is_the_frame() {
    let mut set = OrSet::<u64, 4>::new();
    assert_eq!(set.add(7, Tag::new(2, 1)), Ok(()));
    assert_eq!(set.remove(&7), Ok(()));

    let mut frame = [0_u8; FRAME_LEN];
    assert_eq!(set.encode(&mut frame), Ok(FRAME_LEN));

    assert_eq!(*set.encoded(), frame);
}
