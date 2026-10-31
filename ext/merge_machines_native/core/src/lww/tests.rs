use crate::{HybridTimestamp, LwwRegister, Merge};

#[test]
fn the_later_stamp_wins_from_either_side() {
    let early = LwwRegister::new(1_u32, 3_u64, 7);
    let late = LwwRegister::new(2_u32, 4_u64, 1);

    let mut forwards = early;
    assert_eq!(forwards.merge(&late), Ok(()));
    let mut backwards = late;
    assert_eq!(backwards.merge(&early), Ok(()));

    assert_eq!(forwards, backwards);
    assert_eq!(*forwards.value(), 2);
    assert_eq!(forwards.replica(), 1);
}

#[test]
fn an_equal_stamp_is_broken_the_same_way_from_both_sides() {
    let low = LwwRegister::new(1_u32, 5_u64, 2);
    let high = LwwRegister::new(2_u32, 5_u64, 3);

    let mut forwards = low;
    assert_eq!(forwards.merge(&high), Ok(()));
    let mut backwards = high;
    assert_eq!(backwards.merge(&low), Ok(()));

    assert_eq!(forwards, backwards);
    assert_eq!(*forwards.value(), 2);
    assert_eq!(forwards.replica(), 3);
}

#[test]
fn set_resolves_the_same_way_a_merge_does() {
    let held = LwwRegister::new(1_u32, 5_u64, 2);
    let incoming = LwwRegister::new(9_u32, 5_u64, 4);

    let mut by_set = held;
    by_set.set(9, 5, 4);
    let mut by_merge = held;
    assert_eq!(by_merge.merge(&incoming), Ok(()));

    assert_eq!(by_set, by_merge);
}

#[test]
fn a_hybrid_stamp_orders_by_counter_when_the_reading_matches() {
    let first = LwwRegister::new(
        1_u32,
        HybridTimestamp {
            wall_ns: 100,
            counter: 0,
        },
        9,
    );
    let second = LwwRegister::new(
        2_u32,
        HybridTimestamp {
            wall_ns: 100,
            counter: 1,
        },
        1,
    );

    let mut register = first;
    assert_eq!(register.merge(&second), Ok(()));

    assert_eq!(*register.value(), 2);
}
