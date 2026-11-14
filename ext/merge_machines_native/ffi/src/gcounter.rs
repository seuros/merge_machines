//! `MergeMachines::GCounter`: a counter that only goes up.

use std::cell::RefCell;

use magnus::typed_data::Obj;
use magnus::{Error, RArray, RModule, RString, Ruby, Symbol, Value, function, method, prelude::*};
use matryona_machines::ruby::{Class, Registry, exact_typed, integer};
use merge_machines::{Entry, GCounter, ReplicaId};

use crate::data::{self, classes};
use crate::shape::shaped;
use crate::{merged, unit};

/// The counter at a single replica count.
type Held<const R: usize> = GCounter<R>;

shaped!(Counter over Held as "replicas" { R4 = 4, R8 = 8, R16 = 16, R32 = 32 });

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

/// `MergeMachines::GCounter`, pinned when [`define`] creates it.
static CLASS: Registry<Class> = Registry::new("merge_machines");

#[magnus::wrap(class = "MergeMachines::GCounter", free_immediately, size)]
struct Wrapper(RefCell<Counter>);

const fn replicas_of<const R: usize>(_: &Held<R>) -> usize {
    R
}

impl Wrapper {
    /// `value` as exactly a `MergeMachines::GCounter`: `TypeError` with `message`
    /// for anything else, a subclass instance included.
    fn exact(ruby: &Ruby, value: Value, message: &'static str) -> Result<Obj<Self>, Error> {
        exact_typed(ruby, value, CLASS.get(ruby)?.get(ruby), message)
    }

    /// `GCounter::<replicas>::new()`.
    fn new(ruby: &Ruby, replicas: Value) -> Result<Self, Error> {
        let replicas: usize = integer(replicas, "replicas")?;
        Counter::blank(ruby, replicas).map(|held| Self(RefCell::new(held)))
    }

    /// `GCounter::decode`, at the replica count the frame names.
    fn decode(ruby: &Ruby, frame: RString) -> Result<Self, Error> {
        let frame = frame.to_bytes();
        Counter::decoded(ruby, &frame).map(|held| Self(RefCell::new(held)))
    }

    fn replicas(&self) -> usize {
        counter!(&*self.0.borrow(), |counter| replicas_of(&**counter))
    }

    fn increment(
        ruby: &Ruby,
        rb_self: Obj<Self>,
        replica: Value,
        by: Value,
    ) -> Result<Symbol, Error> {
        rb_self.check_frozen()?;
        let replica: ReplicaId = integer(replica, "replica")?;
        let by: u64 = integer(by, "by")?;
        let outcome = counter!(&mut *rb_self.0.borrow_mut(), |counter| counter
            .increment(replica, by));
        Ok(unit(ruby, outcome, "incremented"))
    }

    fn count(&self, replica: Value) -> Result<u64, Error> {
        let replica: ReplicaId = integer(replica, "replica")?;
        Ok(counter!(&*self.0.borrow(), |counter| counter.count(replica)))
    }

    fn value(&self) -> u128 {
        counter!(&*self.0.borrow(), |counter| counter.value())
    }

    /// The per-replica counts, as `MergeMachines::Entry` values.
    fn entries(ruby: &Ruby, rb_self: &Self) -> Result<RArray, Error> {
        let classes = classes(ruby)?;
        let entries: Vec<Entry> =
            counter!(&*rb_self.0.borrow(), |counter| counter.entries().collect());
        let out = ruby.ary_new_capa(entries.len());
        for held in entries {
            out.push(data::entry(ruby, classes, held)?)?;
        }
        Ok(out)
    }

    fn merge(ruby: &Ruby, rb_self: Obj<Self>, other: Value) -> Result<Symbol, Error> {
        rb_self.check_frozen()?;
        let source = Self::exact(ruby, other, "expected a MergeMachines::GCounter")?
            .0
            .borrow()
            .clone();
        let outcome = rb_self.0.borrow_mut().absorb(&source);
        merged(ruby, outcome, other, "a GCounter of the same replica count")
    }

    fn eq(ruby: &Ruby, rb_self: &Self, other: Value) -> bool {
        Self::exact(ruby, other, "").is_ok_and(|other| rb_self.0.borrow().same(&other.0.borrow()))
    }

    /// `GCounter::encoded`.
    fn encoded(ruby: &Ruby, rb_self: &Self) -> RString {
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

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = module.define_class("GCounter", ruby.class_object())?;
    CLASS.set(ruby, Class::pin(ruby, class))?;
    class.undef_default_alloc_func();
    class.define_singleton_method("new", function!(Wrapper::new, 1))?;
    class.define_singleton_method("decode", function!(Wrapper::decode, 1))?;
    class.define_method("replicas", method!(Wrapper::replicas, 0))?;
    class.define_method("increment", method!(Wrapper::increment, 2))?;
    class.define_method("count", method!(Wrapper::count, 1))?;
    class.define_method("value", method!(Wrapper::value, 0))?;
    class.define_method("entries", method!(Wrapper::entries, 0))?;
    class.define_method("merge", method!(Wrapper::merge, 1))?;
    class.define_method("==", method!(Wrapper::eq, 1))?;
    class.define_method("encoded", method!(Wrapper::encoded, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
