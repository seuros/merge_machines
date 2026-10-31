use crate::{Merge, PNCounter};

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
