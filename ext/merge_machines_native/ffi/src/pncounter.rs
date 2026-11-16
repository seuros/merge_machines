//! `MergeMachines::PNCounter`: a counter that goes both ways.

use std::cell::RefCell;

use magnus::prelude::*;
use matryona_machines::ruby::{Class, Registry};
use merge_machines::PNCounter;

use crate::shape::shaped;

/// The counter at a single replica count.
type Held<const R: usize> = PNCounter<R>;

shaped!($ Counter by counter over Held as "replicas" { R4 = 4, R8 = 8, R16 = 16, R32 = 32 });

/// `MergeMachines::PNCounter`, pinned when [`define`] creates it.
static CLASS: Registry<Class> = Registry::new("merge_machines");

#[magnus::wrap(class = "MergeMachines::PNCounter", free_immediately, size)]
struct Wrapper(RefCell<Counter>);

impl Wrapper {
    crate::exact!("MergeMachines::PNCounter");
    crate::eq!();
    crate::merge!(
        "MergeMachines::PNCounter",
        "a PNCounter of the same replica count"
    );
    crate::shape::shaped_methods!(
        "MergeMachines::PNCounter",
        Counter by counter over Held,
        replicas,
        value
    );
    crate::shape::counter_step!(counter, increment => "incremented");
    crate::shape::counter_step!(counter, decrement => "decremented");

    fn value(&self) -> i128 {
        counter!(&*self.0.borrow(), |counter| counter.value())
    }

    fn added(&self) -> u128 {
        counter!(&*self.0.borrow(), |counter| counter.added())
    }

    fn removed(&self) -> u128 {
        counter!(&*self.0.borrow(), |counter| counter.removed())
    }
}

crate::define!("PNCounter", new(1) {
    "replicas" => replicas(0),
    "increment" => increment(2),
    "decrement" => decrement(2),
    "value" => value(0),
    "added" => added(0),
    "removed" => removed(0),
    "merge" => merge(1),
});
