//! `MergeMachines::OrSet`: a set of unsigned 64-bit Integers elements can
//! leave, where a concurrent add survives a remove. The caller picks every
//! add's `Tag`.

use std::cell::RefCell;

use magnus::typed_data::Obj;
use magnus::{Error, RArray, RModule, RString, Ruby, Symbol, Value, function, method, prelude::*};
use matryona_machines::ruby::{Class, Message, Registry, bytes, exact_typed_with, integer};
use merge_machines::{Adding, OrSet, Removing, Tag, Tagged};

use crate::data::{self, classes, tag_from};
use crate::shape::shaped;
use crate::{expected_class, merged, refusal};

/// The set at a single capacity, over the 64-bit elements a frame carries.
type Held<const N: usize> = OrSet<u64, N>;

shaped!(Set over Held as "capacity" { N64 = 64, N256 = 256, N1024 = 1024 });

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

/// `MergeMachines::OrSet`, pinned when [`define`] creates it.
static CLASS: Registry<Class> = Registry::new("merge_machines");

#[magnus::wrap(class = "MergeMachines::OrSet", free_immediately, size)]
struct Wrapper(RefCell<Set>);

const fn capacity_of<const N: usize>(_: &Held<N>) -> usize {
    N
}

impl Wrapper {
    /// `value` as exactly a `MergeMachines::OrSet`: `TypeError` with `message`
    /// for anything else, a subclass instance included.
    fn exact(ruby: &Ruby, value: Value, message: impl Message) -> Result<Obj<Self>, Error> {
        exact_typed_with(ruby, value, CLASS.get(ruby)?.get(ruby), message)
    }

    /// `OrSet::<u64, capacity>::new()`.
    fn new(ruby: &Ruby, capacity: Value) -> Result<Self, Error> {
        let capacity: usize = integer(capacity, "capacity")?;
        Set::blank(ruby, capacity).map(|held| Self(RefCell::new(held)))
    }

    /// `OrSet::decode`, at the capacity the frame names.
    fn decode(ruby: &Ruby, frame: Value) -> Result<Self, Error> {
        let frame = bytes(frame, "frame")?;
        Set::decoded(ruby, &frame).map(|held| Self(RefCell::new(held)))
    }

    fn capacity(&self) -> usize {
        set!(&*self.0.borrow(), |set| capacity_of(&**set))
    }

    /// `:added`, `:duplicate`, `:full` or `:mismatch`. The tag is read before
    /// the core is called.
    fn add(ruby: &Ruby, rb_self: Obj<Self>, element: Value, tag: Value) -> Result<Symbol, Error> {
        rb_self.check_frozen()?;
        let element: u64 = integer(element, "element")?;
        let tag = tag_from(ruby, tag)?;
        let outcome = set!(&mut *rb_self.0.borrow_mut(), |set| set.add(element, tag));
        Ok(match outcome {
            Ok(Adding::Added) => ruby.to_symbol("added"),
            Ok(Adding::Duplicate) => ruby.to_symbol("duplicate"),
            Err(refused) => refusal(ruby, refused),
        })
    }

    /// `:removed` or `:not_held`.
    fn remove(ruby: &Ruby, rb_self: Obj<Self>, element: Value) -> Result<Symbol, Error> {
        rb_self.check_frozen()?;
        let element: u64 = integer(element, "element")?;
        let outcome = set!(&mut *rb_self.0.borrow_mut(), |set| set.remove(&element));
        Ok(ruby.to_symbol(match outcome {
            Removing::Removed => "removed",
            Removing::NotHeld => "not_held",
        }))
    }

    fn includes(&self, element: Value) -> Result<bool, Error> {
        let element: u64 = integer(element, "element")?;
        Ok(set!(&*self.0.borrow(), |set| set.contains(&element)))
    }

    /// `OrSet::iter`, collected.
    fn to_a(&self) -> Vec<u64> {
        set!(&*self.0.borrow(), |set| set.iter().collect())
    }

    fn size(&self) -> usize {
        set!(&*self.0.borrow(), |set| set.len())
    }

    fn is_empty(&self) -> bool {
        set!(&*self.0.borrow(), |set| set.is_empty())
    }

    /// Every add seen, tombstoned or not, as `MergeMachines::Tagged` values.
    fn adds(ruby: &Ruby, rb_self: &Self) -> Result<RArray, Error> {
        let classes = classes(ruby)?;
        let adds: Vec<Tagged<u64>> = set!(&*rb_self.0.borrow(), |set| set.adds().collect());
        let out = ruby.ary_new_capa(adds.len());
        for add in adds {
            out.push(data::tagged(ruby, classes, add)?)?;
        }
        Ok(out)
    }

    /// Every tombstoned tag, as `MergeMachines::Tag` values.
    fn removes(ruby: &Ruby, rb_self: &Self) -> Result<RArray, Error> {
        let classes = classes(ruby)?;
        let tags: Vec<Tag> = set!(&*rb_self.0.borrow(), |set| set.removes().collect());
        let out = ruby.ary_new_capa(tags.len());
        for tag in tags {
            out.push(data::tag(ruby, classes, tag)?)?;
        }
        Ok(out)
    }

    fn merge(ruby: &Ruby, rb_self: Obj<Self>, other: Value) -> Result<Symbol, Error> {
        rb_self.check_frozen()?;
        let source = Self::exact(ruby, other, || {
            expected_class("a MergeMachines::OrSet", other)
        })?
        .0
        .borrow()
        .clone();
        let outcome = rb_self.0.borrow_mut().absorb(&source);
        merged(ruby, outcome, other, "a OrSet of the same capacity")
    }

    fn eq(ruby: &Ruby, rb_self: &Self, other: Value) -> bool {
        Self::exact(ruby, other, || "")
            .is_ok_and(|other| rb_self.0.borrow().same(&other.0.borrow()))
    }

    /// `OrSet::encoded`.
    fn encoded(ruby: &Ruby, rb_self: &Self) -> RString {
        let frame = set!(&*rb_self.0.borrow(), |set| set.encoded());
        ruby.str_from_slice(&frame)
    }

    fn inspect(&self) -> String {
        format!(
            "#<MergeMachines::OrSet capacity={} size={}>",
            self.capacity(),
            self.size()
        )
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = module.define_class("OrSet", ruby.class_object())?;
    CLASS.set(ruby, Class::pin(ruby, class))?;
    class.undef_default_alloc_func();
    class.define_singleton_method("new", function!(Wrapper::new, 1))?;
    class.define_singleton_method("decode", function!(Wrapper::decode, 1))?;
    class.define_method("capacity", method!(Wrapper::capacity, 0))?;
    class.define_method("add", method!(Wrapper::add, 2))?;
    class.define_method("remove", method!(Wrapper::remove, 1))?;
    class.define_method("include?", method!(Wrapper::includes, 1))?;
    class.define_method("to_a", method!(Wrapper::to_a, 0))?;
    class.define_method("size", method!(Wrapper::size, 0))?;
    class.define_method("empty?", method!(Wrapper::is_empty, 0))?;
    class.define_method("adds", method!(Wrapper::adds, 0))?;
    class.define_method("removes", method!(Wrapper::removes, 0))?;
    class.define_method("merge", method!(Wrapper::merge, 1))?;
    class.define_method("==", method!(Wrapper::eq, 1))?;
    class.define_method("encoded", method!(Wrapper::encoded, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
