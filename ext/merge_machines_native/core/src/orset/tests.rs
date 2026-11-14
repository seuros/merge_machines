use crate::{Adding, Merge, OrSet, Refusal, Removing, Tag};

#[test]
fn an_element_leaves_once_its_tags_are_tombstoned() {
    let mut set = OrSet::<u8, 4>::new();
    assert_eq!(set.add(1, Tag::new(7, 1)), Ok(Adding::Added));
    assert!(set.contains(&1));

    assert_eq!(set.remove(&1), Removing::Removed);

    assert!(!set.contains(&1));
    assert!(set.is_empty());
}

#[test]
fn re_adding_under_a_new_tag_brings_the_element_back() {
    let mut set = OrSet::<u8, 4>::new();
    assert_eq!(set.add(1, Tag::new(7, 1)), Ok(Adding::Added));
    assert_eq!(set.remove(&1), Removing::Removed);

    assert_eq!(set.add(1, Tag::new(7, 2)), Ok(Adding::Added));

    assert!(set.contains(&1));
    assert_eq!(set.len(), 1);
}

#[test]
fn a_concurrent_add_survives_a_remove_that_never_saw_it() {
    let mut shared = OrSet::<u8, 8>::new();
    assert_eq!(shared.add(1, Tag::new(1, 1)), Ok(Adding::Added));

    let mut remover = shared;
    let mut adder = shared;
    assert_eq!(remover.remove(&1), Removing::Removed);
    assert_eq!(adder.add(1, Tag::new(2, 1)), Ok(Adding::Added));

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
    assert_eq!(shared.add(1, Tag::new(1, 1)), Ok(Adding::Added));
    assert_eq!(shared.add(1, Tag::new(2, 1)), Ok(Adding::Added));

    let mut remover = shared;
    assert_eq!(remover.remove(&1), Removing::Removed);

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
    assert_eq!(set.add(1, Tag::new(1, 1)), Ok(Adding::Added));
    assert_eq!(set.add(1, Tag::new(2, 1)), Ok(Adding::Added));
    assert_eq!(set.add(2, Tag::new(1, 2)), Ok(Adding::Added));

    assert_eq!(set.len(), 2);
    assert_eq!(set.iter().filter(|element| *element == 1).count(), 1);
}

#[test]
fn the_same_tag_twice_claims_one_slot() {
    let mut set = OrSet::<u8, 1>::new();
    assert_eq!(set.add(1, Tag::new(1, 1)), Ok(Adding::Added));

    let before = set;
    assert_eq!(set.add(1, Tag::new(1, 1)), Ok(Adding::Duplicate));
    assert_eq!(set, before);
    assert_eq!(set.add(1, Tag::new(1, 2)), Err(Refusal::Full));
    assert_eq!(set, before);
}

#[test]
fn one_tag_naming_another_element_is_a_mismatch() {
    let mut set = OrSet::<u8, 4>::new();
    assert_eq!(set.add(1, Tag::new(1, 1)), Ok(Adding::Added));
    assert_eq!(set.remove(&1), Removing::Removed);

    let before = set;
    assert_eq!(set.add(2, Tag::new(1, 1)), Err(Refusal::Mismatch));
    assert_eq!(set, before);
    // The tombstoned add itself is still a repeat, not a new add.
    assert_eq!(set.add(1, Tag::new(1, 1)), Ok(Adding::Duplicate));
    assert_eq!(set, before);

    let mut other = OrSet::<u8, 4>::new();
    assert_eq!(other.add(2, Tag::new(1, 1)), Ok(Adding::Added));
    assert_eq!(set.merge(&other), Err(Refusal::Mismatch));
    assert_eq!(set, before);
}

#[test]
fn removing_what_is_not_held_changes_nothing() {
    let mut set = OrSet::<u8, 4>::new();
    assert_eq!(set.remove(&1), Removing::NotHeld);
    assert_eq!(set.add(1, Tag::new(1, 1)), Ok(Adding::Added));
    assert_eq!(set.remove(&1), Removing::Removed);

    let before = set;
    assert_eq!(set.remove(&1), Removing::NotHeld);
    assert_eq!(set, before);
}

#[test]
fn a_refused_merge_lands_neither_adds_nor_tombstones() {
    let mut left = OrSet::<u8, 2>::new();
    assert_eq!(left.add(1, Tag::new(1, 1)), Ok(Adding::Added));
    let mut right = left;
    assert_eq!(right.remove(&1), Removing::Removed);
    assert_eq!(right.add(2, Tag::new(2, 1)), Ok(Adding::Added));
    let mut third = right;
    assert_eq!(third.merge(&left), Ok(()));
    let mut crowded = OrSet::<u8, 2>::new();
    assert_eq!(crowded.add(1, Tag::new(1, 1)), Ok(Adding::Added));
    assert_eq!(crowded.add(3, Tag::new(3, 1)), Ok(Adding::Added));

    let before = crowded;
    assert_eq!(crowded.merge(&right), Err(Refusal::Full));
    assert_eq!(crowded, before);
    assert!(
        crowded.contains(&1),
        "the tombstone for (1, 1) must not land"
    );
}

#[test]
fn merging_more_adds_than_there_are_slots_reports_capacity() {
    let mut left = OrSet::<u8, 1>::new();
    let mut right = OrSet::<u8, 1>::new();
    assert_eq!(left.add(1, Tag::new(1, 1)), Ok(Adding::Added));
    assert_eq!(right.add(2, Tag::new(2, 1)), Ok(Adding::Added));

    assert_eq!(left.merge(&right), Err(Refusal::Full));
}

/// A four-slot frame holding `set`'s adds, with `removes` as its tombstones.
fn frame_with_tombstones(set: &OrSet<u64, 4>, removes: &[Tag]) -> [u8; FRAME] {
    let mut frame = [0_u8; FRAME];
    assert_eq!(set.encode(&mut frame), Ok(FRAME));
    frame[5] = u8::try_from(removes.len()).unwrap_or(u8::MAX);
    let start = 7 + 4 * 24;
    let (records, _) = frame[start..].as_chunks_mut::<16>();
    for (record, tag) in records.iter_mut().zip(removes) {
        record[..8].copy_from_slice(&tag.replica.to_le_bytes());
        record[8..].copy_from_slice(&tag.counter.to_le_bytes());
    }
    frame
}

/// Length of a four-slot set's frame.
const FRAME: usize = OrSet::<u64, 4>::ENCODED_LEN;

#[test]
fn a_frame_no_sequence_of_calls_produces_is_malformed() {
    let mut set = OrSet::<u64, 4>::new();
    assert_eq!(set.add(1, Tag::new(1, 1)), Ok(Adding::Added));
    assert_eq!(set.add(1, Tag::new(1, 2)), Ok(Adding::Added));

    let held = frame_with_tombstones(&set, &[Tag::new(1, 2)]);
    let decoded = OrSet::<u64, 4>::decode(&held);
    assert_eq!(decoded.map(|set| set.removes().count()), Ok(1));

    // Before 0.3 a tombstone naming no held add took the slot a later remove
    // needed, and that remove failed halfway. Such a frame is refused now.
    let foreign = frame_with_tombstones(&set, &[Tag::new(9, 9)]);
    assert_eq!(OrSet::<u64, 4>::decode(&foreign), Err(Refusal::Malformed));
    let twice = frame_with_tombstones(&set, &[Tag::new(1, 1), Tag::new(1, 1)]);
    assert_eq!(OrSet::<u64, 4>::decode(&twice), Err(Refusal::Malformed));

    let mut repeated = [0_u8; FRAME];
    assert_eq!(set.encode(&mut repeated), Ok(FRAME));
    repeated[7 + 24 + 8..7 + 48].fill(0);
    repeated[7 + 24 + 8] = 1;
    repeated[7 + 24 + 16] = 1;
    assert_eq!(OrSet::<u64, 4>::decode(&repeated), Err(Refusal::Malformed));
}
