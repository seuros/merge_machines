use crate::{HybridTimestamp, LwwRegister, Merge, Refusal, Stamp, Writing};
use matryona_machines::testing::refuses;

/// The later stamp wins, and an equal stamp is broken by the higher replica,
/// the same way whichever side merges.
#[test]
fn the_winner_is_the_same_from_either_side() {
    for (one, other, (value, replica)) in [
        (
            LwwRegister::new(1_u32, 3_u64, 7),
            LwwRegister::new(2_u32, 4_u64, 1),
            (2, 1),
        ),
        (
            LwwRegister::new(1_u32, 5_u64, 2),
            LwwRegister::new(2_u32, 5_u64, 3),
            (2, 3),
        ),
    ] {
        let mut forwards = one;
        assert_eq!(forwards.merge(&other), Ok(()));
        let mut backwards = other;
        assert_eq!(backwards.merge(&one), Ok(()));

        assert_eq!(forwards, backwards);
        assert_eq!((*forwards.value(), forwards.replica()), (value, replica));
    }
}

#[test]
fn set_resolves_the_same_way_a_merge_does() {
    let held = LwwRegister::new(1_u32, 5_u64, 2);
    let incoming = LwwRegister::new(9_u32, 5_u64, 4);

    let mut by_set = held;
    assert_eq!(by_set.set(9, 5, 4), Ok(Writing::Written));
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

#[test]
fn every_write_is_answered_by_name() {
    let mut register = LwwRegister::new(1_u32, 5_u64, 2);
    let held = register;

    assert_eq!(register.set(7, 4, 9), Ok(Writing::Superseded));
    assert_eq!(register.set(7, 5, 1), Ok(Writing::Superseded));
    assert_eq!(register.set(1, 5, 2), Ok(Writing::Duplicate));
    assert_eq!(register, held);
    assert_eq!(register.set(8, 5, 2), Err(Refusal::Mismatch));
    assert_eq!(register, held);
    assert_eq!(register.set(8, 5, 3), Ok(Writing::Written));
    assert_eq!(*register.value(), 8);
}

#[test]
fn a_merge_of_two_values_under_one_stamp_is_refused() {
    let mut left = LwwRegister::new(1_u32, 5_u64, 2);
    let right = LwwRegister::new(9_u32, 5_u64, 2);
    let before = left;

    refuses(&mut left, Refusal::Mismatch, |l| l.merge(&right));
    assert_eq!(left.merge(&before), Ok(()));
}

#[test]
fn a_stamp_refuses_a_field_too_short_to_hold_it() {
    let mut short = [0_u8; 15];
    let stamp = HybridTimestamp::new(7, 9);
    assert_eq!(stamp.write(&mut short), Err(Refusal::Truncated));
    assert_eq!(HybridTimestamp::read(&short), Err(Refusal::Truncated));
    assert_eq!(7_u64.write(&mut short[..7]), Err(Refusal::Truncated));
    assert_eq!(<u64 as Stamp>::read(&short[..7]), Err(Refusal::Truncated));

    let mut room = [0_u8; 17];
    assert_eq!(stamp.write(&mut room), Ok(()));
    assert_eq!(HybridTimestamp::read(&room), Ok(stamp));
    assert_eq!(room[16], 0, "nothing past the stamp is written");
}
