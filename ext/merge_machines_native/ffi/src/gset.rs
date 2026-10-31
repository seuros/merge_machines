//! `MergeMachines::GSet`: a set nothing is ever taken out of.

use std::cell::RefCell;

use bytes::Bytes;
use magnus::scan_args::scan_args;
use magnus::typed_data::Obj;
use magnus::{Error, RArray, RHash, RModule, RString, Ruby, Value, function, method, prelude::*};
use merge_machines::GSet;

use crate::hash::hash64;
use crate::names::{self, Names};
use crate::shape::{frame_len_of, shaped};
use crate::{
    CAPACITIES, blob_shape, capacity, mismatch, required_kwarg, unsupported, unsupported_blob,
};

/// The set at a single capacity, over the 64-bit hash each key becomes.
type Held<const N: usize> = GSet<u64, N>;

shaped!(Set over Held { N64 = 64, N256 = 256, N1024 = 1024 });

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

/// The set counts 64-bit key hashes, and `names` holds the string each one
/// came from.
#[magnus::wrap(class = "MergeMachines::GSet", free_immediately, size)]
struct Wrapper {
    set: RefCell<Set>,
    names: RefCell<Names>,
}

const fn capacity_of<const N: usize>(_: &Held<N>) -> usize {
    N
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
        let elements = set!(&set, |inner| inner.iter().collect::<Vec<u64>>());
        names::complete(ruby, &names, elements.into_iter())?;

        let wrapper = Self {
            set: RefCell::new(set),
            names: RefCell::new(names),
        };
        wrapper.prune();
        Ok(wrapper)
    }

    fn capacity(&self) -> usize {
        set!(&*self.set.borrow(), |set| capacity_of(&**set))
    }

    /// Adds `key`, which is a no-op if it is already in.
    fn add(ruby: &Ruby, rb_self: &Self, key: String) -> Result<(), Error> {
        let element = hash64(key.as_bytes());
        capacity(
            ruby,
            set!(&mut *rb_self.set.borrow_mut(), |set| set.insert(element)),
        )?;
        rb_self.names.borrow_mut().insert(element, key);
        Ok(())
    }

    fn includes(&self, key: String) -> bool {
        let element = hash64(key.as_bytes());
        set!(&*self.set.borrow(), |set| set.contains(&element))
    }

    /// The keys in the set, in no particular order.
    fn members(ruby: &Ruby, rb_self: &Self) -> Result<RArray, Error> {
        let elements = set!(&*rb_self.set.borrow(), |set| set
            .iter()
            .collect::<Vec<u64>>());
        names::members(ruby, &rb_self.names.borrow(), &elements)
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
        let Some(folded) = rb_self.set.borrow_mut().absorb(&source) else {
            return Err(mismatch(ruby, "set"));
        };
        rb_self.names.borrow_mut().extend(names);
        rb_self.prune();
        capacity(ruby, folded)?;
        Ok(rb_self)
    }

    /// The core frame followed by the names of the keys it holds.
    ///
    /// ```text
    /// core frame   GSet::<u64, capacity>::ENCODED_LEN bytes, tag 0x81
    /// name record  u64 little-endian byte length, then that many UTF-8 bytes
    /// ```
    ///
    /// One record per key, in the core frame's order, filling the rest of the
    /// blob. The frame holds 64-bit hashes, so [`load`](Self::load) rehashes
    /// each name to rebuild what [`members`](Self::members) reads.
    fn dump(ruby: &Ruby, rb_self: &Self) -> RString {
        let (frame, elements) = set!(&*rb_self.set.borrow(), |set| (
            set.encoded(),
            set.iter().collect::<Vec<u64>>()
        ));
        let mut blob = Vec::from(frame);
        names::write(&mut blob, &rb_self.names.borrow(), elements.into_iter());
        ruby.str_from_slice(&blob)
    }

    fn inspect(&self) -> String {
        format!(
            "#<MergeMachines::GSet capacity={} size={}>",
            self.capacity(),
            self.size()
        )
    }

    /// Leaves a name for every key the core holds and for nothing else.
    ///
    /// A merge extends the table before the core reports whether everything
    /// fit, because a merge that overran capacity still took whatever it had
    /// room for and those keys need their names. Reconciling afterwards keeps
    /// the names of the keys that landed and drops the ones that did not.
    fn prune(&self) {
        let set = self.set.borrow();
        self.names
            .borrow_mut()
            .retain(|element, _| set!(&*set, |held| held.contains(element)));
    }
}

/// The variant for `capacity`, read from `blob` when there is one.
fn dispatch(ruby: &Ruby, capacity: usize, blob: Option<&[u8]>) -> Result<Set, Error> {
    let Some(frame) = blob else {
        return Set::blank(capacity)
            .ok_or_else(|| unsupported(ruby, "capacity", capacity, &CAPACITIES));
    };
    Set::decoded(ruby, capacity, frame)
        .unwrap_or_else(|| Err(unsupported_blob(ruby, "capacity", &CAPACITIES)))
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = module.define_class("GSet", ruby.class_object())?;
    class.define_singleton_method("new", function!(Wrapper::new, -1))?;
    class.define_singleton_method("load", function!(Wrapper::load, 1))?;
    class.define_method("capacity", method!(Wrapper::capacity, 0))?;
    class.define_method("add", method!(Wrapper::add, 1))?;
    class.define_method("include?", method!(Wrapper::includes, 1))?;
    class.define_method("members", method!(Wrapper::members, 0))?;
    class.define_method("size", method!(Wrapper::size, 0))?;
    class.define_method("empty?", method!(Wrapper::is_empty, 0))?;
    class.define_method("merge", method!(Wrapper::merge, 1))?;
    class.define_method("dump", method!(Wrapper::dump, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
