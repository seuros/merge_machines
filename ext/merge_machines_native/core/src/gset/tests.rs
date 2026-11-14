use crate::{GSet, Inserting, Merge, Refusal};
use matryona_machines::testing::{refuses, unchanged};

#[test]
fn inserting_the_same_element_twice_changes_nothing() {
    let mut set = GSet::<u8, 4>::new();

    assert_eq!(set.insert(3), Ok(Inserting::Inserted));
    assert_eq!(
        unchanged(&mut set, |s| s.insert(3)),
        Ok(Inserting::Duplicate)
    );

    assert_eq!(set.len(), 1);
    assert!(set.contains(&3));
    assert!(!set.contains(&4));
}

#[test]
fn an_empty_set_says_so() {
    let mut set = GSet::<u8, 2>::new();
    assert!(set.is_empty());

    assert_eq!(set.insert(1), Ok(Inserting::Inserted));

    assert!(!set.is_empty());
}

#[test]
fn disjoint_inserts_converge_whichever_way_they_merge() {
    let mut left = GSet::<u8, 8>::new();
    let mut right = GSet::<u8, 8>::new();
    for element in [1, 2, 3] {
        assert_eq!(left.insert(element), Ok(Inserting::Inserted));
    }
    for element in [3, 4, 5] {
        assert_eq!(right.insert(element), Ok(Inserting::Inserted));
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
        assert_eq!(forwards.insert(element), Ok(Inserting::Inserted));
    }
    for element in [3, 2, 1] {
        assert_eq!(backwards.insert(element), Ok(Inserting::Inserted));
    }

    assert_eq!(forwards, backwards);
}

#[test]
fn a_new_element_needs_a_free_slot() {
    let mut set = GSet::<u8, 2>::new();
    assert_eq!(set.insert(1), Ok(Inserting::Inserted));
    assert_eq!(set.insert(2), Ok(Inserting::Inserted));

    refuses(&mut set, Refusal::Full, |s| s.insert(3));
    assert_eq!(set.insert(1), Ok(Inserting::Duplicate));
}

#[test]
fn a_refused_merge_lands_nothing() {
    let mut left = GSet::<u8, 2>::new();
    assert_eq!(left.insert(1), Ok(Inserting::Inserted));
    let mut right = GSet::<u8, 2>::new();
    assert_eq!(right.insert(2), Ok(Inserting::Inserted));
    assert_eq!(right.insert(3), Ok(Inserting::Inserted));

    refuses(&mut left, Refusal::Full, |l| l.merge(&right));
}

#[test]
fn a_frame_holding_one_element_twice_is_malformed() {
    let mut set = GSet::<u64, 4>::new();
    assert_eq!(set.insert(9), Ok(Inserting::Inserted));
    assert_eq!(set.insert(8), Ok(Inserting::Inserted));
    let mut frame = [0_u8; FRAME_LEN];
    assert_eq!(set.encode(&mut frame), Ok(FRAME_LEN));
    let (first, second) = frame[RECORDS..].split_at_mut(8);
    second[..8].copy_from_slice(first);

    assert_eq!(GSet::<u64, 4>::decode(&frame), Err(Refusal::Malformed));
}

#[test]
fn a_frame_claiming_more_elements_than_slots_is_malformed() {
    let mut frame = [0_u8; FRAME_LEN];
    assert_eq!(GSet::<u64, 4>::new().encode(&mut frame), Ok(FRAME_LEN));
    frame[3] = 5;

    assert_eq!(GSet::<u64, 4>::decode(&frame), Err(Refusal::Malformed));
}

/// Length of a four-element set's frame.
const FRAME_LEN: usize = GSet::<u64, 4>::ENCODED_LEN;

/// Where a frame's element records start: tag, shape, count.
const RECORDS: usize = 5;
