//! `MergeMachines::GCounter`: a counter that only goes up.

use std::cell::RefCell;

use bytes::Bytes;
use magnus::scan_args::{get_kwargs, scan_args};
use magnus::typed_data::Obj;
use magnus::{Error, RHash, RModule, RString, Ruby, Value, function, method, prelude::*};
use merge_machines::{GCounter, ReplicaId};

use crate::shape::shaped;
use crate::{
    REPLICAS, Whole, blob_shape, capacity, mismatch, required_kwarg, unsupported, unsupported_blob,
};

/// The counter at a single replica count.
type Held<const R: usize> = GCounter<R>;

shaped!(Counter over Held { R4 = 4, R8 = 8, R16 = 16, R32 = 32 });

/// Runs `body` against whichever variant is held.
macro_rules! counter {
    ($counter:expr, |$inner:ident| $body:expr) => {
        match $counter {
            Counter::R4($inner) => $body,
            Counter::R8($inner) => $body,
            Counter::R16($inner) => $body,
            Counter::R32($inner) => $body,
        }
    };
}

#[magnus::wrap(class = "MergeMachines::GCounter", free_immediately, size)]
struct Wrapper(RefCell<Counter>);

const fn replicas_of<const R: usize>(_: &Held<R>) -> usize {
    R
}

impl Wrapper {
    fn new(ruby: &Ruby, args: &[Value]) -> Result<Self, Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let Whole(replicas): Whole<usize> = required_kwarg(args.keywords, "replicas")?;
        Ok(Self(RefCell::new(dispatch(ruby, replicas, None)?)))
    }

    /// Reads a blob written by [`dump`](Self::dump).
    fn load(ruby: &Ruby, blob: Bytes) -> Result<Self, Error> {
        let replicas = blob_shape(ruby, &blob)?;
        Ok(Self(RefCell::new(dispatch(ruby, replicas, Some(&blob))?)))
    }

    fn replicas(&self) -> usize {
        counter!(&*self.0.borrow(), |counter| replicas_of(&**counter))
    }

    /// Adds `by:` to `replica:`'s own count.
    fn increment(ruby: &Ruby, rb_self: &Self, args: &[Value]) -> Result<(), Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let (replica, by) = increment_args(args.keywords)?;
        capacity(
            ruby,
            counter!(&mut *rb_self.0.borrow_mut(), |counter| counter
                .increment(replica, by)),
        )
    }

    /// What `replica:` has added.
    fn count(&self, args: &[Value]) -> Result<u64, Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let Whole(replica): Whole<ReplicaId> = required_kwarg(args.keywords, "replica")?;
        Ok(counter!(&*self.0.borrow(), |counter| counter.count(replica)))
    }

    /// Every replica's count.
    fn value(&self) -> u128 {
        counter!(&*self.0.borrow(), |counter| counter.value())
    }

    fn merge(ruby: &Ruby, rb_self: Obj<Self>, other: &Self) -> Result<Obj<Self>, Error> {
        let source = other.0.borrow().clone();
        let Some(folded) = rb_self.0.borrow_mut().absorb(&source) else {
            return Err(mismatch(ruby, "counter"));
        };
        capacity(ruby, folded)?;
        Ok(rb_self)
    }

    /// The core frame, tag 0x61.
    fn dump(ruby: &Ruby, rb_self: &Self) -> RString {
        let frame = counter!(&*rb_self.0.borrow(), |counter| counter.encoded());
        ruby.str_from_slice(&frame)
    }

    fn inspect(&self) -> String {
        format!(
            "#<MergeMachines::GCounter replicas={} value={}>",
            self.replicas(),
            self.value()
        )
    }
}

/// The `replica:` an increment is credited to and the `by:` it adds.
pub fn increment_args(keywords: RHash) -> Result<(ReplicaId, u64), Error> {
    let keywords = get_kwargs::<_, (Whole<ReplicaId>,), (Option<Whole<u64>>,), ()>(
        keywords,
        &["replica"],
        &["by"],
    )?;
    let (Whole(replica),) = keywords.required;
    let (by,) = keywords.optional;
    Ok((replica, by.map_or(1, |Whole(by)| by)))
}

/// The variant for `replicas`, read from `blob` when there is one.
fn dispatch(ruby: &Ruby, replicas: usize, blob: Option<&[u8]>) -> Result<Counter, Error> {
    let Some(frame) = blob else {
        return Counter::blank(replicas)
            .ok_or_else(|| unsupported(ruby, "replicas", replicas, &REPLICAS));
    };
    Counter::decoded(ruby, replicas, frame)
        .unwrap_or_else(|| Err(unsupported_blob(ruby, "replica count", &REPLICAS)))
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = module.define_class("GCounter", ruby.class_object())?;
    class.define_singleton_method("new", function!(Wrapper::new, -1))?;
    class.define_singleton_method("load", function!(Wrapper::load, 1))?;
    class.define_method("replicas", method!(Wrapper::replicas, 0))?;
    class.define_method("increment", method!(Wrapper::increment, -1))?;
    class.define_method("count", method!(Wrapper::count, -1))?;
    class.define_method("value", method!(Wrapper::value, 0))?;
    class.define_method("merge", method!(Wrapper::merge, 1))?;
    class.define_method("dump", method!(Wrapper::dump, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
