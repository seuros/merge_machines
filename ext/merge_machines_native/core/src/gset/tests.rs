use crate::{CapacityError, GSet, Merge};

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
