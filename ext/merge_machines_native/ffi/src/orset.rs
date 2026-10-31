//! `MergeMachines::OrSet`: a set elements can leave, where a concurrent add
//! survives a remove.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use magnus::scan_args::scan_args;
use magnus::typed_data::Obj;
use magnus::{Error, RArray, RHash, RModule, RString, Ruby, Value, function, method, prelude::*};
use merge_machines::{Merge, OrSet, ReplicaId, Tag};

use crate::hash::hash64;
use crate::names::{self, Names, named};
use crate::{
    CAPACITIES, blob_shape, capacity, codec_error, mismatch, required_kwarg, unsupported,
    unsupported_blob,
};

/// Tags minted in this process, so no two sets allocate the same one.
static MINTED: AtomicU64 = AtomicU64::new(0);

/// A set at one of the monomorphised capacities.
#[derive(Clone)]
enum Set {
    N64(Box<OrSet<u64, 64>>),
    N256(Box<OrSet<u64, 256>>),
    N1024(Box<OrSet<u64, 1024>>),
}

/// Runs `body` against whichever variant is held.
macro_rules! set {
    ($set:expr, |$inner:ident| $body:expr) => {
        match $set {
            Set::N64($inner) => $body,
            Set::N256($inner) => $body,
            Set::N1024($inner) => $body,
        }
    };
}

/// The set holds 64-bit key hashes, and `names` holds the string each one came
/// from.
#[magnus::wrap(class = "MergeMachines::OrSet", free_immediately, size)]
struct Wrapper {
    set: RefCell<Set>,
    names: RefCell<Names>,
}

const fn capacity_of<const N: usize>(_: &OrSet<u64, N>) -> usize {
    N
}

const fn frame_len_of<const N: usize>(_: &OrSet<u64, N>) -> usize {
    OrSet::<u64, N>::ENCODED_LEN
}

/// Each shape is constructed behind its own call frame.
#[inline(never)]
fn boxed_new<const N: usize>() -> Box<OrSet<u64, N>> {
    Box::new(OrSet::new())
}

#[inline(never)]
fn boxed_decode<const N: usize>(ruby: &Ruby, frame: &[u8]) -> Result<Box<OrSet<u64, N>>, Error> {
    let mut set = boxed_new::<N>();
    set.decode_into(frame)
        .map_err(|error| codec_error(ruby, error))?;
    Ok(set)
}

impl Wrapper {
    fn new(ruby: &Ruby, args: &[Value]) -> Result<Self, Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let capacity: usize = required_kwarg(args.keywords, "capacity")?;
        Ok(Self {
            set: RefCell::new(dispatch(ruby, capacity, None)?),
            names: RefCell::new(Names::new()),
        })
    }

    /// Reads a blob written by [`dump`](Self::dump).
    fn load(ruby: &Ruby, blob: Bytes) -> Result<Self, Error> {
        let set = dispatch(ruby, blob_shape(ruby, &blob)?, Some(&blob))?;
        let frame_len = set!(&set, |inner| frame_len_of(&**inner));
        let names = names::read(ruby, &blob[frame_len..])?;
        let elements = set!(&set, |inner| inner
            .adds()
            .map(|add| add.element)
            .collect::<Vec<u64>>());
        names::complete(ruby, &names, elements.into_iter())?;

        Ok(Self {
            set: RefCell::new(set),
            names: RefCell::new(names),
        })
    }

    fn capacity(&self) -> usize {
        set!(&*self.set.borrow(), |set| capacity_of(&**set))
    }

    /// Adds `key` under a tag of `replica:` and the next counter that replica
    /// has not used here.
    fn add(ruby: &Ruby, rb_self: &Self, args: &[Value]) -> Result<(), Error> {
        let args = scan_args::<(String,), (), (), (), RHash, ()>(args)?;
        let (key,) = args.required;
        let replica: ReplicaId = required_kwarg(args.keywords, "replica")?;

        let element = hash64(key.as_bytes());
        let tag = Tag::new(replica, rb_self.next_counter(replica));
        capacity(
            ruby,
            set!(&mut *rb_self.set.borrow_mut(), |set| set.add(element, tag)),
        )?;
        rb_self.names.borrow_mut().insert(element, key);
        Ok(())
    }

    /// Tombstones every tag this set has seen for `key`.
    fn remove(ruby: &Ruby, rb_self: &Self, key: String) -> Result<(), Error> {
        let element = hash64(key.as_bytes());
        capacity(
            ruby,
            set!(&mut *rb_self.set.borrow_mut(), |set| set.remove(&element)),
        )
    }

    fn includes(&self, key: String) -> bool {
        let element = hash64(key.as_bytes());
        set!(&*self.set.borrow(), |set| set.contains(&element))
    }

    /// The keys still in the set, each once, in no particular order.
    fn members(ruby: &Ruby, rb_self: &Self) -> Result<RArray, Error> {
        let elements = set!(&*rb_self.set.borrow(), |set| set
            .iter()
            .collect::<Vec<u64>>());
        let names = rb_self.names.borrow();
        let members = ruby.ary_new_capa(elements.len());
        for element in elements {
            members.push(ruby.str_new(&named(ruby, &names, element)?))?;
        }
        Ok(members)
    }

    fn size(&self) -> usize {
        set!(&*self.set.borrow(), |set| set.len())
    }

    fn is_empty(&self) -> bool {
        set!(&*self.set.borrow(), |set| set.is_empty())
    }

    fn merge(ruby: &Ruby, rb_self: Obj<Self>, other: &Self) -> Result<Obj<Self>, Error> {
        let source = other.set.borrow().clone();
        let names = other.names.borrow().clone();
        let folded = match (&mut *rb_self.set.borrow_mut(), &source) {
            (Set::N64(target), Set::N64(source)) => target.merge(source),
            (Set::N256(target), Set::N256(source)) => target.merge(source),
            (Set::N1024(target), Set::N1024(source)) => target.merge(source),
            _ => return Err(mismatch(ruby, "set")),
        };
        capacity(ruby, folded)?;
        rb_self.names.borrow_mut().extend(names);
        Ok(rb_self)
    }

    /// The core frame followed by the names of every key it has seen added.
    ///
    /// ```text
    /// core frame   OrSet::<u64, capacity>::ENCODED_LEN bytes, tag 0x82
    /// name record  u64 little-endian byte length, then that many UTF-8 bytes
    /// ```
    ///
    /// One record per add, tombstoned or not, in the core frame's order,
    /// filling the rest of the blob. The frame holds 64-bit hashes, so
    /// [`load`](Self::load) rehashes each name to rebuild what
    /// [`members`](Self::members) reads.
    fn dump(ruby: &Ruby, rb_self: &Self) -> RString {
        let (frame, elements) = set!(&*rb_self.set.borrow(), |set| (
            set.encoded(),
            set.adds().map(|add| add.element).collect::<Vec<u64>>()
        ));
        let mut blob = Vec::from(frame);
        names::write(&mut blob, &rb_self.names.borrow(), elements.into_iter());
        ruby.str_from_slice(&blob)
    }

    fn inspect(&self) -> String {
        format!(
            "#<MergeMachines::OrSet capacity={} size={}>",
            self.capacity(),
            self.size()
        )
    }

    /// A counter no tag in this process has used, and above anything this
    /// replica already used in this set.
    fn next_counter(&self, replica: ReplicaId) -> u64 {
        let used = |tag: Tag| (tag.replica == replica).then_some(tag.counter);
        let highest = set!(&*self.set.borrow(), |set| set
            .adds()
            .map(|add| add.tag)
            .chain(set.removes())
            .filter_map(used)
            .max())
        .unwrap_or(0);

        let minted = MINTED.fetch_add(1, Ordering::Relaxed);
        highest.max(minted).saturating_add(1)
    }
}

/// The variant for `capacity`, read from `blob` when there is one.
fn dispatch(ruby: &Ruby, capacity: usize, blob: Option<&[u8]>) -> Result<Set, Error> {
    match (capacity, blob) {
        (64, None) => Ok(Set::N64(boxed_new::<64>())),
        (256, None) => Ok(Set::N256(boxed_new::<256>())),
        (1024, None) => Ok(Set::N1024(boxed_new::<1024>())),
        (64, Some(frame)) => Ok(Set::N64(boxed_decode::<64>(ruby, frame)?)),
        (256, Some(frame)) => Ok(Set::N256(boxed_decode::<256>(ruby, frame)?)),
        (1024, Some(frame)) => Ok(Set::N1024(boxed_decode::<1024>(ruby, frame)?)),
        (_, None) => Err(unsupported(ruby, "capacity", capacity, &CAPACITIES)),
        (_, Some(_)) => Err(unsupported_blob(ruby, "capacity", &CAPACITIES)),
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = module.define_class("OrSet", ruby.class_object())?;
    class.define_singleton_method("new", function!(Wrapper::new, -1))?;
    class.define_singleton_method("load", function!(Wrapper::load, 1))?;
    class.define_method("capacity", method!(Wrapper::capacity, 0))?;
    class.define_method("add", method!(Wrapper::add, -1))?;
    class.define_method("remove", method!(Wrapper::remove, 1))?;
    class.define_method("include?", method!(Wrapper::includes, 1))?;
    class.define_method("members", method!(Wrapper::members, 0))?;
    class.define_method("size", method!(Wrapper::size, 0))?;
    class.define_method("empty?", method!(Wrapper::is_empty, 0))?;
    class.define_method("merge", method!(Wrapper::merge, 1))?;
    class.define_method("dump", method!(Wrapper::dump, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
