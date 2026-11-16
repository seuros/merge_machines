//! `MergeMachines::GSet`: a set of unsigned 64-bit Integers nothing is ever
//! taken out of.

use std::cell::RefCell;

use magnus::typed_data::Obj;
use magnus::{Error, Ruby, Symbol, Value, prelude::*};
use matryona_machines::ruby::{Class, Registry, integer};
use merge_machines::{GSet, Inserting};

use crate::refusal;
use crate::shape::shaped;

/// The set at a single capacity, over the 64-bit elements a frame carries.
type Held<const N: usize> = GSet<u64, N>;

shaped!($ Set by set over Held as "capacity" { N64 = 64, N256 = 256, N1024 = 1024 });

/// `MergeMachines::GSet`, pinned when [`define`] creates it.
static CLASS: Registry<Class> = Registry::new("merge_machines");

#[magnus::wrap(class = "MergeMachines::GSet", free_immediately, size)]
struct Wrapper(RefCell<Set>);

impl Wrapper {
    crate::exact!("MergeMachines::GSet");
    crate::eq!();
    crate::merge!("MergeMachines::GSet", "a GSet of the same capacity");
    crate::shape::shaped_methods!(
        "MergeMachines::GSet",
        Set by set over Held,
        capacity,
        size
    );
    crate::shape::set_readers!(set);

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
}

crate::define!("GSet", new(1) {
    "capacity" => capacity(0),
    "insert" => insert(1),
    "include?" => includes(1),
    "to_a" => to_a(0),
    "size" => size(0),
    "empty?" => is_empty(0),
    "merge" => merge(1),
});
