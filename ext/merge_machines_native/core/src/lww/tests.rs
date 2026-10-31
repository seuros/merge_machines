use crate::{CodecError, HybridTimestamp, LwwRegister, Merge};

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

/// Length of a logically stamped register's frame.
const LOGICAL_LEN: usize = LwwRegister::<u64, u64>::ENCODED_LEN;

/// Length of a hybrid stamped register's frame.
const HYBRID_LEN: usize = LwwRegister::<u64, HybridTimestamp>::ENCODED_LEN;

#[test]
fn a_frame_round_trips_the_value_its_stamp_and_its_writer() {
    let register = LwwRegister::new(77_u64, 9_u64, 3);

    let mut frame = [0_u8; LOGICAL_LEN];
    assert_eq!(register.encode(&mut frame), Ok(LOGICAL_LEN));

    assert_eq!(LwwRegister::<u64, u64>::decode(&frame), Ok(register));
}

#[test]
fn a_frame_round_trips_a_hybrid_stamp() {
    let stamp = HybridTimestamp {
        wall_ns: 1_000,
        counter: 4,
    };
    let register = LwwRegister::new(77_u64, stamp, 3);

    let mut frame = [0_u8; HYBRID_LEN];
    assert_eq!(register.encode(&mut frame), Ok(HYBRID_LEN));

    assert_eq!(
        LwwRegister::<u64, HybridTimestamp>::decode(&frame),
        Ok(register)
    );
}

#[test]
fn encoding_into_a_short_buffer_is_refused() {
    let mut frame = [0_u8; LOGICAL_LEN - 1];

    assert_eq!(
        LwwRegister::new(1_u64, 1_u64, 1).encode(&mut frame),
        Err(CodecError::Truncated)
    );
}

#[test]
fn a_short_frame_is_refused() {
    assert_eq!(
        LwwRegister::<u64, u64>::decode(&[]),
        Err(CodecError::Truncated)
    );
}

#[test]
fn another_structures_frame_is_refused() {
    let mut frame = [0_u8; LOGICAL_LEN];
    assert_eq!(
        LwwRegister::new(1_u64, 1_u64, 1).encode(&mut frame),
        Ok(LOGICAL_LEN)
    );
    frame[crate::TAG] = crate::TAG_GSET;

    assert_eq!(
        LwwRegister::<u64, u64>::decode(&frame),
        Err(CodecError::Tag)
    );
}

#[test]
fn a_frame_written_with_another_kind_of_stamp_is_refused() {
    let stamp = HybridTimestamp {
        wall_ns: 1,
        counter: 1,
    };
    let mut frame = [0_u8; HYBRID_LEN];
    assert_eq!(
        LwwRegister::new(1_u64, stamp, 1).encode(&mut frame),
        Ok(HYBRID_LEN)
    );

    assert_eq!(
        LwwRegister::<u64, u64>::decode(&frame),
        Err(CodecError::Parameters)
    );
}

#[test]
fn a_refused_frame_leaves_the_register_alone() {
    let mut register = LwwRegister::new(5_u64, 2_u64, 1);
    let before = register;

    assert_eq!(register.decode_into(&[]), Err(CodecError::Truncated));

    assert_eq!(register, before);
}

#[cfg(feature = "alloc")]
#[test]
fn the_allocated_frame_is_the_frame() {
    let register = LwwRegister::new(8_u64, 3_u64, 2);

    let mut frame = [0_u8; LOGICAL_LEN];
    assert_eq!(register.encode(&mut frame), Ok(LOGICAL_LEN));

    assert_eq!(*register.encoded(), frame);
}
