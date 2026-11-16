//! The `Data` values Ruby defines and the binding reads and builds:
//! `Timestamp`, `Entry`, `Tag` and `Tagged`.
//!
//! `lib/merge_machines.rb` defines each class before this extension loads, and
//! each one's `initialize` calls the private hook here, so a hand-built value
//! is held to the core's constructor exactly like one the binding returns. The
//! classes are looked up once, at load, so building one after a core call has
//! mutated never has to find a constant.

use magnus::{Error, RClass, RModule, Ruby, Value, function, prelude::*};
use matryona_machines::ruby::{Class, exact_fields_with, integer};

use crate::expected_message;
use merge_machines::{Entry, HybridTimestamp, Tag, Tagged};

/// The classes this extension builds, resolved and pinned at load so no
/// lookup follows a mutation.
pub struct Classes {
    pub timestamp: Class,
    pub entry: Class,
    pub tag: Class,
    pub tagged: Class,
}

matryona_machines::registry! {
    static CLASSES: Classes = "merge_machines";
    pub fn classes;
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = |name| Class::resolve(ruby, module, name);
    let classes = Classes {
        timestamp: class("Timestamp")?,
        entry: class("Entry")?,
        tag: class("Tag")?,
        tagged: class("Tagged")?,
    };
    CLASSES.set(ruby, classes)?;
    module.define_singleton_method("__timestamp", function!(checked_timestamp, 2))?;
    module.define_singleton_method("__entry", function!(checked_entry, 2))?;
    module.define_singleton_method("__tag", function!(checked_tag, 2))?;
    module.define_singleton_method("__tagged", function!(checked_tagged, 2))?;
    Ok(())
}

/// `Timestamp.new`'s check. `HybridTimestamp::new` takes every pair of
/// unsigned 64-bit Integers, so reading both is the whole check.
fn checked_timestamp(wall_ns: Value, counter: Value) -> Result<(), Error> {
    integer::<u64>(wall_ns, "wall_ns")?;
    integer::<u64>(counter, "counter").map(drop)
}

/// `Entry.new`'s check. `Entry::new` takes every pair, as for a `Timestamp`.
fn checked_entry(replica: Value, count: Value) -> Result<(), Error> {
    integer::<u64>(replica, "replica")?;
    integer::<u64>(count, "count").map(drop)
}

/// `Tag.new`'s check. `Tag::new` takes every pair, as for a `Timestamp`.
fn checked_tag(replica: Value, counter: Value) -> Result<(), Error> {
    integer::<u64>(replica, "replica")?;
    integer::<u64>(counter, "counter").map(drop)
}

/// `Tagged.new`'s check: an unsigned 64-bit element and a `Tag`, which
/// `Tagged::new` takes whatever they hold.
fn checked_tagged(ruby: &Ruby, element: Value, tag: Value) -> Result<(), Error> {
    integer::<u64>(element, "element")?;
    tag_from(ruby, tag).map(drop)
}

/// The two fields of a value of exactly `class`, as unsigned 64-bit
/// Integers: `TypeError` naming `what` for a value of any other class, a
/// subclass included. The fields are read natively, so a method redefined on
/// an opened singleton class answers nothing. Reading each as a `u64` is the
/// core constructor's whole check, so a value that skipped `initialize`
/// (`Marshal.load`, `Data#initialize` bound directly) is held to it here:
/// `TypeError` or `RangeError` before the core sees it.
fn pair(
    ruby: &Ruby,
    value: Value,
    class: RClass,
    what: &str,
    [first, second]: [&str; 2],
) -> Result<(u64, u64), Error> {
    let [one, two] = exact_fields_with::<2>(ruby, value, class, || expected_message(what, value))?;
    Ok((integer(one, first)?, integer(two, second)?))
}

/// The core timestamp a `MergeMachines::Timestamp` names.
pub fn timestamp_from(ruby: &Ruby, value: Value) -> Result<HybridTimestamp, Error> {
    let class = classes(ruby)?.timestamp.get(ruby);
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
    let class = classes(ruby)?.tag.get(ruby);
    let (replica, counter) = pair(
        ruby,
        value,
        class,
        "a MergeMachines::Tag",
        ["replica", "counter"],
    )?;
    Ok(Tag::new(replica, counter))
}

pub fn timestamp(ruby: &Ruby, classes: &Classes, stamp: HybridTimestamp) -> Result<Value, Error> {
    classes
        .timestamp
        .get(ruby)
        .funcall("new", (stamp.wall_ns, stamp.counter))
}

pub fn entry(ruby: &Ruby, classes: &Classes, entry: Entry) -> Result<Value, Error> {
    classes
        .entry
        .get(ruby)
        .funcall("new", (entry.replica, entry.count))
}

pub fn tag(ruby: &Ruby, classes: &Classes, tag: Tag) -> Result<Value, Error> {
    classes
        .tag
        .get(ruby)
        .funcall("new", (tag.replica, tag.counter))
}

pub fn tagged(ruby: &Ruby, classes: &Classes, add: Tagged<u64>) -> Result<Value, Error> {
    let tag = tag(ruby, classes, add.tag)?;
    classes.tagged.get(ruby).funcall("new", (add.element, tag))
}
