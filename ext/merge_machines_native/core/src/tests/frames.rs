//! The frame contract every state shares, run once per structure: a frame
//! round-trips, `encoded` is the frame `encode` writes, and a short buffer,
//! a short frame or another structure's frame is refused with the state
//! exactly as it was. A frame written at another shape is refused too.

use core::fmt::Debug;

use matryona_machines::testing::refuses;

use crate::{
    Adding, GCounter, GSet, HybridClock, HybridTimestamp, Inserting, LogicalClock, LwwRegister,
    OrSet, PNCounter, Refusal, Removing, TAG, Tag,
};

/// The codec every state carries, gathered so one test can drive them all.
trait Framed: Clone + PartialEq + Debug + Sized {
    const LEN: usize;
    fn encode(&self, out: &mut [u8]) -> Result<usize, Refusal>;
    #[cfg(feature = "alloc")]
    fn encoded(&self) -> alloc::boxed::Box<[u8]>;
    fn decode(bytes: &[u8]) -> Result<Self, Refusal>;
    fn decode_into(&mut self, bytes: &[u8]) -> Result<(), Refusal>;
}

macro_rules! framed {
    ($($state:ty),+ $(,)?) => {$(
        impl Framed for $state {
            const LEN: usize = <$state>::ENCODED_LEN;
            fn encode(&self, out: &mut [u8]) -> Result<usize, Refusal> {
                <$state>::encode(self, out)
            }
            #[cfg(feature = "alloc")]
            fn encoded(&self) -> alloc::boxed::Box<[u8]> {
                <$state>::encoded(self)
            }
            fn decode(bytes: &[u8]) -> Result<Self, Refusal> {
                <$state>::decode(bytes)
            }
            fn decode_into(&mut self, bytes: &[u8]) -> Result<(), Refusal> {
                <$state>::decode_into(self, bytes)
            }
        }
    )+};
}

framed!(
    LogicalClock,
    HybridClock,
    GCounter<4>,
    GCounter<8>,
    PNCounter<4>,
    PNCounter<8>,
    LwwRegister<u64, u64>,
    LwwRegister<u64, HybridTimestamp>,
    GSet<u64, 4>,
    GSet<u64, 8>,
    OrSet<u64, 4>,
    OrSet<u64, 8>,
);

/// Frames longer than any state's, so one buffer serves them all.
const ROOM: usize = 1024;

/// Runs the contract on `state`, which should hold something, with `foreign`
/// the tag of another structure.
fn assert_contract<S: Framed>(state: &S, foreign: u8) {
    let name = core::any::type_name::<S>();
    let mut room = [0_u8; ROOM];
    assert!(S::LEN <= ROOM, "{name}");
    let frame = &mut room[..S::LEN];
    assert_eq!(state.encode(frame), Ok(S::LEN), "{name}");
    assert_eq!(S::decode(frame).as_ref(), Ok(state), "{name}");
    #[cfg(feature = "alloc")]
    assert_eq!(&*state.encoded(), &*frame, "{name}");

    let mut short = [0_u8; ROOM];
    assert_eq!(
        state.encode(&mut short[..S::LEN - 1]),
        Err(Refusal::Truncated),
        "{name}"
    );

    let mut other = [0_u8; ROOM];
    other[..S::LEN].copy_from_slice(frame);
    other[TAG] = foreign;
    for (bytes, refusal) in [
        (&frame[..S::LEN - 1], Refusal::Truncated),
        (&[][..], Refusal::Truncated),
        (&other[..S::LEN], Refusal::WrongStructure),
    ] {
        assert_eq!(S::decode(bytes), Err(refusal), "{name}");
        refuses(&mut state.clone(), refusal, |held| held.decode_into(bytes));
    }
}

#[test]
fn every_frame_round_trips_and_refuses_without_moving() -> Result<(), Refusal> {
    let mut logical = LogicalClock::new();
    logical.observe(41)?;
    assert_contract(&logical, crate::TAG_HYBRID_CLOCK);

    let mut hybrid = HybridClock::new();
    hybrid.tick(900)?;
    hybrid.tick(900)?;
    assert_contract(&hybrid, crate::TAG_LOGICAL_CLOCK);

    let mut grow = GCounter::<4>::new();
    grow.increment(7, 2)?;
    grow.increment(9, 5)?;
    assert_contract(&grow, crate::TAG_PNCOUNTER);

    let mut both = PNCounter::<4>::new();
    both.increment(1, 10)?;
    both.decrement(2, 4)?;
    assert_contract(&both, crate::TAG_GCOUNTER);

    assert_contract(&LwwRegister::new(77_u64, 9_u64, 3), crate::TAG_GSET);
    let stamp = HybridTimestamp::new(1_000, 4);
    assert_contract(&LwwRegister::new(77_u64, stamp, 3), crate::TAG_GSET);

    let mut set = GSet::<u64, 4>::new();
    assert_eq!(set.insert(11)?, Inserting::Inserted);
    assert_eq!(set.insert(22)?, Inserting::Inserted);
    assert_contract(&set, crate::TAG_ORSET);

    let mut tombstoned = OrSet::<u64, 4>::new();
    assert_eq!(tombstoned.add(11, Tag::new(1, 1))?, Adding::Added);
    assert_eq!(tombstoned.add(22, Tag::new(1, 2))?, Adding::Added);
    assert_eq!(tombstoned.remove(&11), Removing::Removed);
    assert_contract(&tombstoned, crate::TAG_GSET);
    Ok(())
}

/// `written`'s frame, read back as an `R`: another shape of the same structure.
/// The buffer is as long as either frame (zero-padded), so a frame shorter than
/// an `R` reaches the shape check instead of stopping at `Truncated`.
fn assert_wrong_shape<W: Framed, R: Framed>(written: &W) {
    let mut room = [0_u8; ROOM];
    assert_eq!(written.encode(&mut room[..W::LEN]), Ok(W::LEN));
    let frame = &room[..W::LEN.max(R::LEN)];
    let name = core::any::type_name::<R>();
    assert_eq!(R::decode(frame), Err(Refusal::WrongShape), "{name}");
}

#[test]
fn a_frame_written_at_another_shape_is_refused() {
    assert_wrong_shape::<GCounter<8>, GCounter<4>>(&GCounter::new());
    assert_wrong_shape::<PNCounter<8>, PNCounter<4>>(&PNCounter::new());
    assert_wrong_shape::<GSet<u64, 8>, GSet<u64, 4>>(&GSet::new());
    assert_wrong_shape::<OrSet<u64, 8>, OrSet<u64, 4>>(&OrSet::new());
    let stamp = HybridTimestamp::new(1, 1);
    assert_wrong_shape::<_, LwwRegister<u64, u64>>(&LwwRegister::new(1_u64, stamp, 1));
    let logical = LwwRegister::new(1_u64, 1_u64, 1);
    assert_wrong_shape::<_, LwwRegister<u64, HybridTimestamp>>(&logical);
}
