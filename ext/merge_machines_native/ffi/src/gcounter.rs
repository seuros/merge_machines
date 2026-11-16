//! `MergeMachines::GCounter`: a counter that only goes up.

use std::cell::RefCell;

use magnus::{Error, RArray, Ruby, Value, prelude::*};
use matryona_machines::ruby::{Class, Registry, integer};
use merge_machines::{Entry, GCounter, ReplicaId};

use crate::data::{self, classes};
use crate::shape::shaped;

/// The counter at a single replica count.
type Held<const R: usize> = GCounter<R>;

shaped!($ Counter by counter over Held as "replicas" { R4 = 4, R8 = 8, R16 = 16, R32 = 32 });

/// `MergeMachines::GCounter`, pinned when [`define`] creates it.
static CLASS: Registry<Class> = Registry::new("merge_machines");

#[magnus::wrap(class = "MergeMachines::GCounter", free_immediately, size)]
struct Wrapper(RefCell<Counter>);

impl Wrapper {
    crate::exact!("MergeMachines::GCounter");
    crate::eq!();
    crate::merge!(
        "MergeMachines::GCounter",
        "a GCounter of the same replica count"
    );
    crate::shape::shaped_methods!(
        "MergeMachines::GCounter",
        Counter by counter over Held,
        replicas,
        value
    );
    crate::shape::counter_step!(counter, increment => "incremented");

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
}

crate::define!("GCounter", new(1) {
    "replicas" => replicas(0),
    "increment" => increment(2),
    "count" => count(1),
    "value" => value(0),
    "entries" => entries(0),
    "merge" => merge(1),
});
