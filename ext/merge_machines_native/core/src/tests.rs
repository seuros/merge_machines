use crate::{
    GCounter, GSet, HybridTimestamp, LwwRegister, Merge, OrSet, PNCounter, Tag, same_elements,
};
use core::fmt::Debug;

fn merged<C: Merge + Clone>(left: &C, right: &C) -> C {
    let mut out = left.clone();
    assert!(out.merge(right).is_ok(), "merge ran out of slots");
    out
}

/// Asserts the three laws every replica relies on: order does not matter,
/// grouping does not matter, and re-delivery does not matter.
fn assert_algebra<C: Merge + Clone + PartialEq + Debug>(left: &C, middle: &C, right: &C) {
    assert_eq!(
        merged(left, middle),
        merged(middle, left),
        "merge is not commutative"
    );
    assert_eq!(
        merged(&merged(left, middle), right),
        merged(left, &merged(middle, right)),
        "merge is not associative"
    );
    assert_eq!(merged(left, left), *left, "merge is not idempotent");
    assert_eq!(
        merged(&merged(left, middle), middle),
        merged(left, middle),
        "a redelivered state is not absorbed"
    );
}

#[test]
fn gcounter_merge_obeys_the_laws() {
    let mut left = GCounter::<8>::new();
    let mut middle = GCounter::<8>::new();
    let mut right = GCounter::<8>::new();
    assert_eq!(left.increment(1, 3), Ok(()));
    assert_eq!(left.increment(2, 1), Ok(()));
    assert_eq!(middle.increment(2, 9), Ok(()));
    assert_eq!(middle.increment(3, 4), Ok(()));
    assert_eq!(right.increment(1, 7), Ok(()));
    assert_eq!(right.increment(4, 2), Ok(()));

    assert_algebra(&left, &middle, &right);
}

#[test]
fn pncounter_merge_obeys_the_laws() {
    let mut left = PNCounter::<8>::new();
    let mut middle = PNCounter::<8>::new();
    let mut right = PNCounter::<8>::new();
    assert_eq!(left.increment(1, 10), Ok(()));
    assert_eq!(left.decrement(2, 3), Ok(()));
    assert_eq!(middle.decrement(1, 4), Ok(()));
    assert_eq!(middle.increment(3, 6), Ok(()));
    assert_eq!(right.increment(2, 5), Ok(()));
    assert_eq!(right.decrement(3, 1), Ok(()));

    assert_algebra(&left, &middle, &right);
}

#[test]
fn lww_register_merge_obeys_the_laws() {
    let left = LwwRegister::new(1_u32, 5_u64, 2);
    let middle = LwwRegister::new(2_u32, 5_u64, 3);
    let right = LwwRegister::new(3_u32, 9_u64, 1);

    assert_algebra(&left, &middle, &right);
}

#[test]
fn lww_register_merge_obeys_the_laws_with_hybrid_stamps() {
    let stamp = |wall_ns, counter| HybridTimestamp { wall_ns, counter };
    let left = LwwRegister::new(1_u32, stamp(100, 1), 2);
    let middle = LwwRegister::new(2_u32, stamp(100, 1), 7);
    let right = LwwRegister::new(3_u32, stamp(99, 40), 1);

    assert_algebra(&left, &middle, &right);
}

#[test]
fn gset_merge_obeys_the_laws() {
    let mut left = GSet::<u8, 16>::new();
    let mut middle = GSet::<u8, 16>::new();
    let mut right = GSet::<u8, 16>::new();
    for element in [1, 2, 3] {
        assert_eq!(left.insert(element), Ok(()));
    }
    for element in [3, 4] {
        assert_eq!(middle.insert(element), Ok(()));
    }
    for element in [5, 1] {
        assert_eq!(right.insert(element), Ok(()));
    }

    assert_algebra(&left, &middle, &right);
}

#[test]
fn orset_merge_obeys_the_laws() {
    let mut shared = OrSet::<u8, 16>::new();
    assert_eq!(shared.add(1, Tag::new(1, 1)), Ok(()));
    assert_eq!(shared.add(2, Tag::new(1, 2)), Ok(()));

    let mut left = shared;
    assert_eq!(left.remove(&1), Ok(()));
    assert_eq!(left.add(3, Tag::new(1, 3)), Ok(()));

    let mut middle = shared;
    assert_eq!(middle.add(1, Tag::new(2, 1)), Ok(()));
    assert_eq!(middle.remove(&2), Ok(()));

    let mut right = shared;
    assert_eq!(right.add(4, Tag::new(3, 1)), Ok(()));
    assert_eq!(right.remove(&4), Ok(()));

    assert_algebra(&left, &middle, &right);
}

#[test]
fn orset_converges_on_the_same_members_whatever_the_merge_order() {
    let mut shared = OrSet::<u8, 16>::new();
    assert_eq!(shared.add(1, Tag::new(1, 1)), Ok(()));

    let mut remover = shared;
    assert_eq!(remover.remove(&1), Ok(()));
    let mut adder = shared;
    assert_eq!(adder.add(1, Tag::new(2, 1)), Ok(()));
    let mut bystander = shared;
    assert_eq!(bystander.add(2, Tag::new(3, 1)), Ok(()));

    let one = merged(&merged(&remover, &adder), &bystander);
    let two = merged(&merged(&bystander, &remover), &adder);
    let three = merged(&merged(&adder, &bystander), &remover);

    assert_eq!(one, two);
    assert_eq!(two, three);
    assert_eq!(one.len(), 2);
}

#[test]
fn slot_equality_ignores_position_and_catches_a_missing_element() {
    let left = [Some(1), None, Some(2)];
    let shuffled = [Some(2), Some(1), None];
    let short = [Some(1), None, None];

    assert!(same_elements(&left, &shuffled));
    assert!(!same_elements(&left, &short));
    assert!(!same_elements(&short, &left));
}
