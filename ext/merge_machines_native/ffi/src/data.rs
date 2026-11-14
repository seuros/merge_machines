//! The `Data` values Ruby defines and the binding reads and builds:
//! `Timestamp`, `Entry`, `Tag` and `Tagged`.
//!
//! `lib/merge_machines.rb` defines each class before this extension loads, and
//! each one's `initialize` calls the private hook here, so a hand-built value
//! is held to the core's constructor exactly like one the binding returns. The
//! classes are looked up once, at load, so building one after a core call has
//! mutated never has to find a constant.

use std::sync::OnceLock;

use magnus::value::Opaque;
use magnus::{Error, RClass, RModule, Ruby, Value, function, prelude::*};
use merge_machines::{Entry, HybridTimestamp, Tag, Tagged};

use crate::{Whole, expected, instance_of, uninitialised};

static TIMESTAMP: OnceLock<Opaque<RClass>> = OnceLock::new();
static ENTRY: OnceLock<Opaque<RClass>> = OnceLock::new();
static TAG: OnceLock<Opaque<RClass>> = OnceLock::new();
static TAGGED: OnceLock<Opaque<RClass>> = OnceLock::new();

/// The classes this extension builds, cached so no lookup follows a mutation.
#[derive(Clone, Copy)]
pub struct Classes {
    pub timestamp: RClass,
    pub entry: RClass,
    pub tag: RClass,
    pub tagged: RClass,
}

pub fn classes(ruby: &Ruby) -> Result<Classes, Error> {
    let class = |cell: &OnceLock<Opaque<RClass>>| {
        cell.get()
            .map(|class| ruby.get_inner(*class))
            .ok_or_else(|| uninitialised(ruby))
    };
    Ok(Classes {
        timestamp: class(&TIMESTAMP)?,
        entry: class(&ENTRY)?,
        tag: class(&TAG)?,
        tagged: class(&TAGGED)?,
    })
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    for (cell, name) in [
        (&TIMESTAMP, "Timestamp"),
        (&ENTRY, "Entry"),
        (&TAG, "Tag"),
        (&TAGGED, "Tagged"),
    ] {
        let class: RClass = module.const_get(name)?;
        // Held in a static, so pinned: `GC.compact` must not move it.
        ruby.gc_register_mark_object(class);
        // A second load keeps the first classes: they are the same constants.
        let _ = cell.get_or_init(|| class.into());
    }
    module.define_singleton_method("__timestamp", function!(checked_timestamp, 2))?;
    module.define_singleton_method("__entry", function!(checked_entry, 2))?;
    module.define_singleton_method("__tag", function!(checked_tag, 2))?;
    module.define_singleton_method("__tagged", function!(checked_tagged, 2))?;
    Ok(())
}

/// `Timestamp.new`'s check. `HybridTimestamp::new` takes every pair of
/// unsigned 64-bit Integers, so reading both is the whole check.
const fn checked_timestamp(Whole(_wall_ns): Whole<u64>, Whole(_counter): Whole<u64>) {}

/// `Entry.new`'s check. `Entry::new` takes every pair, as for a `Timestamp`.
const fn checked_entry(Whole(_replica): Whole<u64>, Whole(_count): Whole<u64>) {}

/// `Tag.new`'s check. `Tag::new` takes every pair, as for a `Timestamp`.
const fn checked_tag(Whole(_replica): Whole<u64>, Whole(_counter): Whole<u64>) {}

/// `Tagged.new`'s check: an unsigned 64-bit element and a `Tag`, which
/// `Tagged::new` takes whatever they hold.
fn checked_tagged(ruby: &Ruby, Whole(_element): Whole<u64>, tag: Value) -> Result<(), Error> {
    tag_from(ruby, tag).map(|_tag| ())
}

/// Two readers of a value of exactly `class`, as unsigned 64-bit Integers:
/// `TypeError` naming `what` for a value of any other class, a subclass
/// included. Reading each field as a `Whole<u64>` is the core constructor's
/// whole check, so a value that skipped `initialize` (`Marshal.load`,
/// `Data#initialize` bound directly) is held to it here: `TypeError` or
/// `RangeError` before the core sees it.
fn pair(
    ruby: &Ruby,
    value: Value,
    class: RClass,
    what: &str,
    [first, second]: [&str; 2],
) -> Result<(u64, u64), Error> {
    if !instance_of(value, class) {
        return Err(expected(ruby, what, value));
    }
    let Whole(left) = value.funcall::<_, _, Whole<u64>>(first, ())?;
    let Whole(right) = value.funcall::<_, _, Whole<u64>>(second, ())?;
    Ok((left, right))
}

/// The core timestamp a `MergeMachines::Timestamp` names.
pub fn timestamp_from(ruby: &Ruby, value: Value) -> Result<HybridTimestamp, Error> {
    let class = classes(ruby)?.timestamp;
    let (wall_ns, counter) = pair(
        ruby,
        value,
        class,
        "a MergeMachines::Timestamp",
        ["wall_ns", "counter"],
    )?;
    Ok(HybridTimestamp::new(wall_ns, counter))
}

/// The core tag a `MergeMachines::Tag` names.
pub fn tag_from(ruby: &Ruby, value: Value) -> Result<Tag, Error> {
    let class = classes(ruby)?.tag;
    let (replica, counter) = pair(
        ruby,
        value,
        class,
        "a MergeMachines::Tag",
        ["replica", "counter"],
    )?;
    Ok(Tag::new(replica, counter))
}

pub fn timestamp(classes: Classes, stamp: HybridTimestamp) -> Result<Value, Error> {
    classes
        .timestamp
        .funcall("new", (stamp.wall_ns, stamp.counter))
}

pub fn entry(classes: Classes, entry: Entry) -> Result<Value, Error> {
    classes.entry.funcall("new", (entry.replica, entry.count))
}

pub fn tag(classes: Classes, tag: Tag) -> Result<Value, Error> {
    classes.tag.funcall("new", (tag.replica, tag.counter))
}

pub fn tagged(classes: Classes, add: Tagged<u64>) -> Result<Value, Error> {
    let tag = tag(classes, add.tag)?;
    classes.tagged.funcall("new", (add.element, tag))
}
