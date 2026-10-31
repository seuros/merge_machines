//! The frozen `Data` object the hybrid clock hands back.

use std::sync::OnceLock;

use magnus::value::Opaque;
use magnus::{Error, RClass, RModule, RStruct, Ruby, Value, prelude::*};
use merge_machines::HybridTimestamp;

use crate::{defined_twice, uninitialised};

static TIMESTAMP: OnceLock<Opaque<RClass>> = OnceLock::new();

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let timestamp = ruby.define_data(None, ("wall_ns", "counter"))?;
    module.const_set("Timestamp", timestamp)?;
    TIMESTAMP
        .set(timestamp.into())
        .map_err(|_| defined_twice(ruby, "Timestamp"))
}

/// A wall-clock reading in nanoseconds and the counter that breaks its ties.
pub fn timestamp(ruby: &Ruby, stamp: HybridTimestamp) -> Result<Value, Error> {
    let class = TIMESTAMP
        .get()
        .map(|class| ruby.get_inner(*class))
        .ok_or_else(|| uninitialised(ruby))?;
    let instance = RStruct::try_convert(class.obj_alloc()?)?;
    for (index, member) in [
        ruby.into_value(stamp.wall_ns),
        ruby.into_value(stamp.counter),
    ]
    .into_iter()
    .enumerate()
    {
        instance.aset(index, member)?;
    }
    instance.freeze();
    Ok(instance.as_value())
}

/// The timestamp a `wall:` and `counter:` name.
pub const fn stamped(wall_ns: u64, counter: u64) -> HybridTimestamp {
    HybridTimestamp { wall_ns, counter }
}
