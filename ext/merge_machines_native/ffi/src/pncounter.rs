//! `MergeMachines::PNCounter`: a counter replicas can lower as well as raise.

use std::cell::RefCell;

use bytes::Bytes;
use magnus::scan_args::scan_args;
use magnus::typed_data::Obj;
use magnus::{Error, RHash, RModule, RString, Ruby, Value, function, method, prelude::*};
use merge_machines::{Merge, PNCounter};

use crate::gcounter::increment_args;
use crate::{
    REPLICAS, blob_shape, capacity, codec_error, mismatch, required_kwarg, unsupported,
    unsupported_blob,
};

/// A counter at one of the monomorphised replica counts.
#[derive(Clone)]
enum Counter {
    R4(Box<PNCounter<4>>),
    R8(Box<PNCounter<8>>),
    R16(Box<PNCounter<16>>),
    R32(Box<PNCounter<32>>),
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

#[magnus::wrap(class = "MergeMachines::PNCounter", free_immediately, size)]
struct Wrapper(RefCell<Counter>);

const fn replicas_of<const R: usize>(_: &PNCounter<R>) -> usize {
    R
}

/// Each shape is constructed behind its own call frame.
#[inline(never)]
fn boxed_new<const R: usize>() -> Box<PNCounter<R>> {
    Box::new(PNCounter::new())
}

#[inline(never)]
fn boxed_decode<const R: usize>(ruby: &Ruby, frame: &[u8]) -> Result<Box<PNCounter<R>>, Error> {
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

    /// Takes `by:` away on `replica:`'s own account.
    fn decrement(ruby: &Ruby, rb_self: &Self, args: &[Value]) -> Result<(), Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let (replica, by) = increment_args(args.keywords)?;
        capacity(
            ruby,
            counter!(&mut *rb_self.0.borrow_mut(), |counter| counter
                .decrement(replica, by)),
        )
    }

    /// Everything added, minus everything taken away.
    fn value(&self) -> i128 {
        counter!(&*self.0.borrow(), |counter| counter.value())
    }

    /// Everything added, by every replica.
    fn added(&self) -> u64 {
        counter!(&*self.0.borrow(), |counter| counter.added())
    }

    /// Everything taken away, by every replica.
    fn removed(&self) -> u64 {
        counter!(&*self.0.borrow(), |counter| counter.removed())
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

    /// The core frame, tag 0x62.
    fn dump(ruby: &Ruby, rb_self: &Self) -> RString {
        let frame = counter!(&*rb_self.0.borrow(), |counter| counter.encoded());
        ruby.str_from_slice(&frame)
    }

    fn inspect(&self) -> String {
        format!(
            "#<MergeMachines::PNCounter replicas={} value={}>",
            self.replicas(),
            self.value()
        )
    }
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
    let class = module.define_class("PNCounter", ruby.class_object())?;
    class.define_singleton_method("new", function!(Wrapper::new, -1))?;
    class.define_singleton_method("load", function!(Wrapper::load, 1))?;
    class.define_method("replicas", method!(Wrapper::replicas, 0))?;
    class.define_method("increment", method!(Wrapper::increment, -1))?;
    class.define_method("decrement", method!(Wrapper::decrement, -1))?;
    class.define_method("value", method!(Wrapper::value, 0))?;
    class.define_method("added", method!(Wrapper::added, 0))?;
    class.define_method("removed", method!(Wrapper::removed, 0))?;
    class.define_method("merge", method!(Wrapper::merge, 1))?;
    class.define_method("dump", method!(Wrapper::dump, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
