//! Randomized runs of every structure against a plain reference model. Each
//! refusal and each honoured repeat is checked against a clone taken before
//! the call: it must have changed nothing.

use crate::{
    Adding, GCounter, GSet, HybridClock, HybridTimestamp, Inserting, LogicalClock, LwwRegister,
    Merge, OrSet, PNCounter, Refusal, Removing, Tag, Writing,
};

/// Replicas a run draws ids from: more than the three slots the states get.
const IDS: usize = 5;

/// Elements a set run draws from.
const ELEMENTS: u8 = 6;

/// Counters per replica an `OrSet` run tags its adds with.
const COUNTERS: usize = 4;

/// Operations per run.
const STEPS: usize = 600;

/// Seeds per structure.
const SEEDS: [u64; 4] = [1, 0x9E37_79B9, 0xDEAD_BEEF, 0x1234_5678_9ABC];

/// A xorshift generator: deterministic, allocation-free, good enough to mix.
struct Draw(u64);

impl Draw {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A number below `bound`.
    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % bound as u64).unwrap_or(0)
    }

    /// A small number below `bound`, as a set element.
    fn byte(&mut self, bound: u8) -> u8 {
        u8::try_from(self.next() % u64::from(bound)).unwrap_or(0)
    }
}

/// An amount that sometimes comes close to the top of a `u64`.
fn amount(draw: &mut Draw) -> u64 {
    match draw.below(8) {
        0 => 0,
        1 => u64::MAX / 2,
        _ => draw.next() % 10,
    }
}

/// Per-replica counts, `None` for a replica the state holds no slot for.
type Counts = [Option<u64>; IDS];

/// The `GCounter<3>` model: what `increment` must answer, and the new counts.
fn modelled_increment(counts: &Counts, replica: usize, by: u64) -> Result<Counts, Refusal> {
    let mut next = *counts;
    if by == 0 {
        return Ok(next);
    }
    match counts[replica] {
        Some(count) => next[replica] = Some(count.checked_add(by).ok_or(Refusal::Overflow)?),
        None if counts.iter().flatten().count() == 3 => return Err(Refusal::Full),
        None => next[replica] = Some(by),
    }
    Ok(next)
}

/// The `GCounter<3>` model of a merge.
fn modelled_merge(into: &Counts, from: &Counts) -> Result<Counts, Refusal> {
    let mut next = *into;
    for (mine, theirs) in next.iter_mut().zip(from) {
        *mine = match (*mine, *theirs) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (left, right) => left.or(right),
        };
    }
    if next.iter().flatten().count() > 3 {
        return Err(Refusal::Full);
    }
    Ok(next)
}

fn assert_counts(counter: &GCounter<3>, counts: &Counts) {
    for (replica, count) in (0..).zip(counts) {
        assert_eq!(counter.count(replica), count.unwrap_or(0));
    }
    assert_eq!(counter.entries().count(), counts.iter().flatten().count());
}

#[test]
fn gcounter_and_pncounter_follow_the_model() {
    for seed in SEEDS {
        let mut draw = Draw(seed);
        let mut states = [GCounter::<3>::new(); 3];
        let mut models = [[None; IDS]; 3];
        let mut pn = [PNCounter::<3>::new(); 2];
        for _ in 0..STEPS {
            let (at, from) = (draw.below(3), draw.below(3));
            let replica = draw.below(IDS);
            let before = states[at];
            let (answer, expected) = if draw.below(3) == 0 {
                let source = states[from];
                let expected = modelled_merge(&models[at], &models[from]);
                (states[at].merge(&source), expected)
            } else {
                let by = amount(&mut draw);
                let expected = modelled_increment(&models[at], replica, by);
                (states[at].increment(replica as u64, by), expected)
            };
            assert_eq!(answer, expected.map(|_| ()));
            match expected {
                Ok(next) => models[at] = next,
                Err(_) => assert_eq!(states[at], before, "a refusal changed the counter"),
            }
            assert_counts(&states[at], &models[at]);

            let side = at % 2;
            let held = pn[side];
            let step = match draw.below(3) {
                0 => {
                    let source = pn[1 - side];
                    pn[side].merge(&source)
                }
                1 => pn[side].increment(replica as u64, amount(&mut draw)),
                _ => pn[side].decrement(replica as u64, amount(&mut draw)),
            };
            if step.is_err() {
                assert_eq!(pn[side], held, "a refusal changed the counter");
            }
        }
    }
}

#[test]
fn gset_follows_the_model() {
    for seed in SEEDS {
        let mut draw = Draw(seed);
        let mut states = [GSet::<u8, 3>::new(); 3];
        let mut models = [[false; ELEMENTS as usize]; 3];
        for _ in 0..STEPS {
            let (at, from) = (draw.below(3), draw.below(3));
            let before = states[at];
            if draw.below(3) == 0 {
                let source = states[from];
                let mut union = models[at];
                for (mine, theirs) in union.iter_mut().zip(models[from]) {
                    *mine |= theirs;
                }
                let fits = union.iter().filter(|held| **held).count() <= 3;
                let answer = states[at].merge(&source);
                assert_eq!(answer, if fits { Ok(()) } else { Err(Refusal::Full) });
                if fits {
                    models[at] = union;
                }
            } else {
                let element = draw.byte(ELEMENTS);
                let held = models[at].iter().filter(|held| **held).count();
                let expected = match (models[at][usize::from(element)], held) {
                    (true, _) => Ok(Inserting::Duplicate),
                    (false, 3) => Err(Refusal::Full),
                    (false, _) => Ok(Inserting::Inserted),
                };
                assert_eq!(states[at].insert(element), expected);
                if expected == Ok(Inserting::Inserted) {
                    models[at][usize::from(element)] = true;
                }
            }
            if states[at].len() == before.len() {
                assert_eq!(states[at], before, "a refusal or repeat changed the set");
            }
            for (element, held) in (0..).zip(models[at]) {
                assert_eq!(states[at].contains(&element), held);
            }
        }
    }
}

/// What an `OrSet` run knows of one tag: the element it added, and whether
/// that add is tombstoned.
type Tags = [[Option<(u8, bool)>; COUNTERS]; IDS];

fn orset_step(
    draw: &mut Draw,
    set: &mut OrSet<u8, 3>,
    model: &mut Tags,
    other: (&OrSet<u8, 3>, &Tags),
) {
    let before = *set;
    let held = model.iter().flatten().flatten().count();
    let changed = match draw.below(4) {
        0 => {
            let mut union = *model;
            let mut mismatch = false;
            for (mine, theirs) in union.iter_mut().flatten().zip(other.1.iter().flatten()) {
                *mine = match (*mine, *theirs) {
                    (Some(left), Some(right)) if left.0 != right.0 => {
                        mismatch = true;
                        Some(left)
                    }
                    (Some(left), Some(right)) => Some((left.0, left.1 || right.1)),
                    (left, right) => left.or(right),
                };
            }
            let expected = match (mismatch, union.iter().flatten().flatten().count()) {
                (true, _) => Err(Refusal::Mismatch),
                (false, count) if count > 3 => Err(Refusal::Full),
                (false, _) => Ok(()),
            };
            assert_eq!(set.merge(other.0), expected);
            let changed = expected.is_ok() && union != *model;
            if expected.is_ok() {
                *model = union;
            }
            changed
        }
        1 => {
            let element = draw.byte(ELEMENTS);
            let mut expected = Removing::NotHeld;
            for (held, removed) in model.iter_mut().flatten().flatten() {
                if *held == element && !*removed {
                    *removed = true;
                    expected = Removing::Removed;
                }
            }
            assert_eq!(set.remove(&element), expected);
            expected == Removing::Removed
        }
        _ => {
            let (replica, counter) = (draw.below(IDS), draw.below(COUNTERS));
            let element = draw.byte(ELEMENTS);
            let expected = match (model[replica][counter], held) {
                (Some((added, _)), _) if added == element => Ok(Adding::Duplicate),
                (Some(_), _) => Err(Refusal::Mismatch),
                (None, 3) => Err(Refusal::Full),
                (None, _) => Ok(Adding::Added),
            };
            assert_eq!(
                set.add(element, Tag::new(replica as u64, counter as u64)),
                expected
            );
            if expected == Ok(Adding::Added) {
                model[replica][counter] = Some((element, false));
            }
            expected == Ok(Adding::Added)
        }
    };
    if !changed {
        assert_eq!(*set, before, "a refusal or repeat changed the set");
    }
    for element in 0..ELEMENTS {
        let live = model
            .iter()
            .flatten()
            .flatten()
            .any(|seen| *seen == (element, false));
        assert_eq!(set.contains(&element), live);
    }
}

#[test]
fn orset_follows_the_model() {
    for seed in SEEDS {
        let mut draw = Draw(seed);
        let mut states = [OrSet::<u8, 3>::new(); 3];
        let mut models: [Tags; 3] = [[[None; COUNTERS]; IDS]; 3];
        for _ in 0..STEPS {
            let (at, from) = (draw.below(3), draw.below(3));
            let (source, known) = (states[from], models[from]);
            orset_step(
                &mut draw,
                &mut states[at],
                &mut models[at],
                (&source, &known),
            );
        }
    }
}

#[test]
fn lww_register_follows_the_model() {
    for seed in SEEDS {
        let mut draw = Draw(seed);
        let mut states = [LwwRegister::new(0_u8, 0_u64, 0); 3];
        for _ in 0..STEPS {
            let (at, from) = (draw.below(3), draw.below(3));
            let before = states[at];
            let held = (*before.stamp(), before.replica(), *before.value());
            let (stamp, replica) = (draw.next() % 6, draw.next() % 3);
            let value = if draw.below(2) == 0 {
                held.2
            } else {
                draw.byte(3)
            };
            let (stamp, replica, value, answer) = if draw.below(3) == 0 {
                let source = states[from];
                let theirs = (*source.stamp(), source.replica(), *source.value());
                let answer = states[at].merge(&source).map(|()| Writing::Written);
                (theirs.0, theirs.1, theirs.2, answer)
            } else {
                (stamp, replica, value, states[at].set(value, stamp, replica))
            };
            let expected = match (stamp, replica).cmp(&(held.0, held.1)) {
                core::cmp::Ordering::Less => Ok(Writing::Superseded),
                core::cmp::Ordering::Equal if value == held.2 => Ok(Writing::Duplicate),
                core::cmp::Ordering::Equal => Err(Refusal::Mismatch),
                core::cmp::Ordering::Greater => Ok(Writing::Written),
            };
            match (answer, expected) {
                (Ok(_), Ok(Writing::Written)) => {
                    assert_eq!(
                        (*states[at].stamp(), states[at].replica()),
                        (stamp, replica)
                    );
                }
                (Ok(_), Ok(Writing::Superseded | Writing::Duplicate))
                | (Err(Refusal::Mismatch), Err(_)) => {
                    assert_eq!(states[at], before, "a refusal or losing write changed it");
                }
                (answer, expected) => assert_eq!(answer.map(|_| ()), expected.map(|_| ())),
            }
        }
    }
}

#[test]
fn clocks_refuse_without_moving() {
    for seed in SEEDS {
        let mut draw = Draw(seed);
        let mut logical = LogicalClock::new();
        let mut hybrid = HybridClock::new();
        for _ in 0..STEPS {
            let near_top = u64::MAX - (draw.next() % 4);
            let before = logical;
            let answer = if draw.below(2) == 0 {
                logical.tick()
            } else {
                logical.observe(if draw.below(4) == 0 {
                    near_top
                } else {
                    draw.next() % 50
                })
            };
            match answer {
                Ok(stamp) => assert!(stamp > before.now() && stamp == logical.now()),
                Err(refusal) => {
                    assert_eq!(refusal, Refusal::Exhausted);
                    assert_eq!(logical, before);
                }
            }

            let before = hybrid;
            let wall_ns = draw.next() % 8;
            let observed = HybridTimestamp::new(draw.next() % 8, near_top);
            let answer = if draw.below(2) == 0 {
                hybrid.tick(wall_ns)
            } else {
                hybrid.observe(wall_ns, observed)
            };
            match answer {
                Ok(stamp) => assert!(stamp > before.last() && stamp == hybrid.last()),
                Err(refusal) => {
                    assert_eq!(refusal, Refusal::Exhausted);
                    assert_eq!(hybrid, before);
                }
            }
            if draw.below(50) == 0 {
                hybrid = HybridClock::new();
                logical = LogicalClock::new();
            }
        }
    }
}
