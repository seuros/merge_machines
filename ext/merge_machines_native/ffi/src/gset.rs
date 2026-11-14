//! `MergeMachines::GSet`: a set of unsigned 64-bit Integers nothing is ever
//! taken out of.

use std::cell::RefCell;

use magnus::typed_data::Obj;
use magnus::{Error, RModule, RString, Ruby, Symbol, Value, function, method, prelude::*};
use matryona_machines::ruby::{Class, Message, Registry, bytes, exact_typed_with, integer};
use merge_machines::{GSet, Inserting};

use crate::shape::shaped;
use crate::{expected_class, merged, refusal};

/// The set at a single capacity, over the 64-bit elements a frame carries.
type Held<const N: usize> = GSet<u64, N>;

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

/// `MergeMachines::GSet`, pinned when [`define`] creates it.
static CLASS: Registry<Class> = Registry::new("merge_machines");

#[magnus::wrap(class = "MergeMachines::GSet", free_immediately, size)]
struct Wrapper(RefCell<Set>);

const fn capacity_of<const N: usize>(_: &Held<N>) -> usize {
    N
}

impl Wrapper {
    /// `value` as exactly a `MergeMachines::GSet`: `TypeError` with `message`
    /// for anything else, a subclass instance included.
    fn exact(ruby: &Ruby, value: Value, message: impl Message) -> Result<Obj<Self>, Error> {
        exact_typed_with(ruby, value, CLASS.get(ruby)?.get(ruby), message)
    }

    /// `GSet::<u64, capacity>::new()`.
    fn new(ruby: &Ruby, capacity: Value) -> Result<Self, Error> {
        let capacity: usize = integer(capacity, "capacity")?;
        Set::blank(ruby, capacity).map(|held| Self(RefCell::new(held)))
    }

    /// `GSet::decode`, at the capacity the frame names.
    fn decode(ruby: &Ruby, frame: Value) -> Result<Self, Error> {
        let frame = bytes(frame, "frame")?;
        Set::decoded(ruby, &frame).map(|held| Self(RefCell::new(held)))
    }

    fn capacity(&self) -> usize {
        set!(&*self.0.borrow(), |set| capacity_of(&**set))
    }

    /// `:inserted`, `:duplicate` or `:full`.
    fn insert(ruby: &Ruby, rb_self: Obj<Self>, element: Value) -> Result<Symbol, Error> {
        rb_self.check_frozen()?;
        let element: u64 = integer(element, "element")?;
        let outcome = set!(&mut *rb_self.0.borrow_mut(), |set| set.insert(element));
        Ok(match outcome {
            Ok(Inserting::Inserted) => ruby.to_symbol("inserted"),
            Ok(Inserting::Duplicate) => ruby.to_symbol("duplicate"),
            Err(refused) => refusal(ruby, refused),
        })
    }

    fn includes(&self, element: Value) -> Result<bool, Error> {
        let element: u64 = integer(element, "element")?;
        Ok(set!(&*self.0.borrow(), |set| set.contains(&element)))
    }

    /// `GSet::iter`, collected.
    fn to_a(&self) -> Vec<u64> {
        set!(&*self.0.borrow(), |set| set.iter().collect())
    }

    fn size(&self) -> usize {
        set!(&*self.0.borrow(), |set| set.len())
    }

    fn is_empty(&self) -> bool {
        set!(&*self.0.borrow(), |set| set.is_empty())
    }

    fn merge(ruby: &Ruby, rb_self: Obj<Self>, other: Value) -> Result<Symbol, Error> {
        rb_self.check_frozen()?;
        let source = Self::exact(ruby, other, || {
            expected_class("a MergeMachines::GSet", other)
        })?
        .0
        .borrow()
        .clone();
        let outcome = rb_self.0.borrow_mut().absorb(&source);
        merged(ruby, outcome, other, "a GSet of the same capacity")
    }

    fn eq(ruby: &Ruby, rb_self: &Self, other: Value) -> bool {
        Self::exact(ruby, other, || "")
            .is_ok_and(|other| rb_self.0.borrow().same(&other.0.borrow()))
    }

    /// `GSet::encoded`.
    fn encoded(ruby: &Ruby, rb_self: &Self) -> RString {
        let frame = set!(&*rb_self.0.borrow(), |set| set.encoded());
        ruby.str_from_slice(&frame)
    }

    fn inspect(&self) -> String {
        format!(
            "#<MergeMachines::GSet capacity={} size={}>",
            self.capacity(),
            self.size()
        )
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = module.define_class("GSet", ruby.class_object())?;
    CLASS.set(ruby, Class::pin(ruby, class))?;
    class.undef_default_alloc_func();
    class.define_singleton_method("new", function!(Wrapper::new, 1))?;
    class.define_singleton_method("decode", function!(Wrapper::decode, 1))?;
    class.define_method("capacity", method!(Wrapper::capacity, 0))?;
    class.define_method("insert", method!(Wrapper::insert, 1))?;
    class.define_method("include?", method!(Wrapper::includes, 1))?;
    class.define_method("to_a", method!(Wrapper::to_a, 0))?;
    class.define_method("size", method!(Wrapper::size, 0))?;
    class.define_method("empty?", method!(Wrapper::is_empty, 0))?;
    class.define_method("merge", method!(Wrapper::merge, 1))?;
    class.define_method("==", method!(Wrapper::eq, 1))?;
    class.define_method("encoded", method!(Wrapper::encoded, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
