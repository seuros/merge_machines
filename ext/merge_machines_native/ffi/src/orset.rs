//! `MergeMachines::OrSet`: a set of unsigned 64-bit Integers elements can
//! leave, where a concurrent add survives a remove. The caller picks every
//! add's `Tag`.

use std::cell::RefCell;

use magnus::typed_data::Obj;
use magnus::{Error, RArray, Ruby, Symbol, Value, prelude::*};
use matryona_machines::ruby::{Class, Registry, integer};
use merge_machines::{Adding, OrSet, Removing, Tag, Tagged};

use crate::data::{self, classes, tag_from};
use crate::refusal;
use crate::shape::shaped;

/// The set at a single capacity, over the 64-bit elements a frame carries.
type Held<const N: usize> = OrSet<u64, N>;

shaped!($ Set by set over Held as "capacity" { N64 = 64, N256 = 256, N1024 = 1024 });

/// `MergeMachines::OrSet`, pinned when [`define`] creates it.
static CLASS: Registry<Class> = Registry::new("merge_machines");

#[magnus::wrap(class = "MergeMachines::OrSet", free_immediately, size)]
struct Wrapper(RefCell<Set>);

impl Wrapper {
    crate::exact!("MergeMachines::OrSet");
    crate::eq!();
    crate::merge!("MergeMachines::OrSet", "an OrSet of the same capacity");
    crate::shape::shaped_methods!(
        "MergeMachines::OrSet",
        Set by set over Held,
        capacity,
        size
    );
    crate::shape::set_readers!(set);

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
}

crate::define!("OrSet", new(1) {
    "capacity" => capacity(0),
    "add" => add(2),
    "remove" => remove(1),
    "include?" => includes(1),
    "to_a" => to_a(0),
    "size" => size(0),
    "empty?" => is_empty(0),
    "adds" => adds(0),
    "removes" => removes(0),
    "merge" => merge(1),
});
