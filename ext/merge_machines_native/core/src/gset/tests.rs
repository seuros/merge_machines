use crate::{CapacityError, CodecError, GSet, Merge};

#[test]
fn inserting_the_same_element_twice_changes_nothing() {
    let mut set = GSet::<u8, 4>::new();

    assert_eq!(set.insert(3), Ok(()));
    assert_eq!(set.insert(3), Ok(()));

    assert_eq!(set.len(), 1);
    assert!(set.contains(&3));
    assert!(!set.contains(&4));
}

#[test]
fn an_empty_set_says_so() {
    let mut set = GSet::<u8, 2>::new();
    assert!(set.is_empty());

    assert_eq!(set.insert(1), Ok(()));

    assert!(!set.is_empty());
}

#[test]
fn disjoint_inserts_converge_whichever_way_they_merge() {
    let mut left = GSet::<u8, 8>::new();
    let mut right = GSet::<u8, 8>::new();
    for element in [1, 2, 3] {
        assert_eq!(left.insert(element), Ok(()));
    }
    for element in [3, 4, 5] {
        assert_eq!(right.insert(element), Ok(()));
    }

    let mut forwards = left;
    assert_eq!(forwards.merge(&right), Ok(()));
    let mut backwards = right;
    assert_eq!(backwards.merge(&left), Ok(()));

    assert_eq!(forwards, backwards);
    assert_eq!(forwards.len(), 5);
}

#[test]
fn equality_ignores_which_slot_an_element_landed_in() {
    let mut forwards = GSet::<u8, 4>::new();
    let mut backwards = GSet::<u8, 4>::new();
    for element in [1, 2, 3] {
        assert_eq!(forwards.insert(element), Ok(()));
    }
    for element in [3, 2, 1] {
        assert_eq!(backwards.insert(element), Ok(()));
    }

    assert_eq!(forwards, backwards);
}

#[test]
fn a_new_element_needs_a_free_slot() {
    let mut set = GSet::<u8, 2>::new();
    assert_eq!(set.insert(1), Ok(()));
    assert_eq!(set.insert(2), Ok(()));

    assert_eq!(set.insert(3), Err(CapacityError));
    assert_eq!(set.insert(1), Ok(()));
}

/// Length of a four-element set's frame.
const FRAME_LEN: usize = GSet::<u64, 4>::ENCODED_LEN;

#[test]
fn a_frame_round_trips_every_element() {
    let mut set = GSet::<u64, 4>::new();
    assert_eq!(set.insert(11), Ok(()));
    assert_eq!(set.insert(22), Ok(()));

    let mut frame = [0_u8; FRAME_LEN];
    assert_eq!(set.encode(&mut frame), Ok(FRAME_LEN));

    let restored = GSet::<u64, 4>::decode(&frame);
    assert_eq!(restored, Ok(set));
    assert_eq!(restored.map(|set| set.len()), Ok(2));
}

#[test]
fn encoding_into_a_short_buffer_is_refused() {
    let mut frame = [0_u8; FRAME_LEN - 1];

    assert_eq!(
        GSet::<u64, 4>::new().encode(&mut frame),
        Err(CodecError::Truncated)
    );
}

#[test]
fn a_short_frame_is_refused() {
    assert_eq!(GSet::<u64, 4>::decode(&[]), Err(CodecError::Truncated));
}

#[test]
fn another_structures_frame_is_refused() {
    let mut frame = [0_u8; FRAME_LEN];
    assert_eq!(GSet::<u64, 4>::new().encode(&mut frame), Ok(FRAME_LEN));
    frame[crate::TAG] = crate::TAG_ORSET;

    assert_eq!(GSet::<u64, 4>::decode(&frame), Err(CodecError::Tag));
}

#[test]
fn a_frame_written_at_another_capacity_is_refused() {
    let mut frame = [0_u8; GSet::<u64, 8>::ENCODED_LEN];
    assert_eq!(
        GSet::<u64, 8>::new().encode(&mut frame),
        Ok(GSet::<u64, 8>::ENCODED_LEN)
    );

    assert_eq!(GSet::<u64, 4>::decode(&frame), Err(CodecError::Parameters));
}

#[test]
fn a_refused_frame_leaves_the_set_alone() {
    let mut set = GSet::<u64, 4>::new();
    assert_eq!(set.insert(5), Ok(()));
    let before = set;

    assert_eq!(set.decode_into(&[]), Err(CodecError::Truncated));

    assert_eq!(set, before);
}

#[cfg(feature = "alloc")]
#[test]
fn the_allocated_frame_is_the_frame() {
    let mut set = GSet::<u64, 4>::new();
    assert_eq!(set.insert(7), Ok(()));

    let mut frame = [0_u8; FRAME_LEN];
    assert_eq!(set.encode(&mut frame), Ok(FRAME_LEN));

    assert_eq!(*set.encoded(), frame);
}
