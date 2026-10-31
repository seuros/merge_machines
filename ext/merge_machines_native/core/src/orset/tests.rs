use crate::{CapacityError, Merge, OrSet, Tag};

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
