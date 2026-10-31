//! `MergeMachines::GCounter`: a counter that only goes up.

use std::cell::RefCell;

use bytes::Bytes;
use magnus::scan_args::{get_kwargs, scan_args};
use magnus::typed_data::Obj;
use magnus::{Error, RHash, RModule, RString, Ruby, Value, function, method, prelude::*};
use merge_machines::{GCounter, Merge, ReplicaId};

use crate::{
    REPLICAS, blob_shape, capacity, codec_error, mismatch, required_kwarg, unsupported,
    unsupported_blob,
};

/// A counter at one of the monomorphised replica counts.
#[derive(Clone)]
enum Counter {
    R4(Box<GCounter<4>>),
    R8(Box<GCounter<8>>),
    R16(Box<GCounter<16>>),
    R32(Box<GCounter<32>>),
}

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

const fn replicas_of<const R: usize>(_: &GCounter<R>) -> usize {
    R
}

/// Each shape is constructed behind its own call frame.
#[inline(never)]
fn boxed_new<const R: usize>() -> Box<GCounter<R>> {
    Box::new(GCounter::new())
}

#[inline(never)]
fn boxed_decode<const R: usize>(ruby: &Ruby, frame: &[u8]) -> Result<Box<GCounter<R>>, Error> {
    let mut counter = boxed_new::<R>();
    counter
        .decode_into(frame)
        .map_err(|error| codec_error(ruby, error))?;
    Ok(counter)
}

impl Wrapper {
    fn new(ruby: &Ruby, args: &[Value]) -> Result<Self, Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let replicas: usize = required_kwarg(args.keywords, "replicas")?;
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
        let replica: ReplicaId = required_kwarg(args.keywords, "replica")?;
        Ok(counter!(&*self.0.borrow(), |counter| counter.count(replica)))
    }

    /// Every replica's count.
    fn value(&self) -> u64 {
        counter!(&*self.0.borrow(), |counter| counter.value())
    }

    fn merge(ruby: &Ruby, rb_self: Obj<Self>, other: &Self) -> Result<Obj<Self>, Error> {
        let source = other.0.borrow().clone();
        let folded = match (&mut *rb_self.0.borrow_mut(), &source) {
            (Counter::R4(target), Counter::R4(source)) => target.merge(source),
            (Counter::R8(target), Counter::R8(source)) => target.merge(source),
            (Counter::R16(target), Counter::R16(source)) => target.merge(source),
            (Counter::R32(target), Counter::R32(source)) => target.merge(source),
            _ => return Err(mismatch(ruby, "counter")),
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
    let keywords =
        get_kwargs::<_, (ReplicaId,), (Option<u64>,), ()>(keywords, &["replica"], &["by"])?;
    let (replica,) = keywords.required;
    let (by,) = keywords.optional;
    Ok((replica, by.unwrap_or(1)))
}

/// The variant for `replicas`, read from `blob` when there is one.
fn dispatch(ruby: &Ruby, replicas: usize, blob: Option<&[u8]>) -> Result<Counter, Error> {
    match (replicas, blob) {
        (4, None) => Ok(Counter::R4(boxed_new::<4>())),
        (8, None) => Ok(Counter::R8(boxed_new::<8>())),
        (16, None) => Ok(Counter::R16(boxed_new::<16>())),
        (32, None) => Ok(Counter::R32(boxed_new::<32>())),
        (4, Some(frame)) => Ok(Counter::R4(boxed_decode::<4>(ruby, frame)?)),
        (8, Some(frame)) => Ok(Counter::R8(boxed_decode::<8>(ruby, frame)?)),
        (16, Some(frame)) => Ok(Counter::R16(boxed_decode::<16>(ruby, frame)?)),
        (32, Some(frame)) => Ok(Counter::R32(boxed_decode::<32>(ruby, frame)?)),
        (_, None) => Err(unsupported(ruby, "replicas", replicas, &REPLICAS)),
        (_, Some(_)) => Err(unsupported_blob(ruby, "replica count", &REPLICAS)),
    }
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
