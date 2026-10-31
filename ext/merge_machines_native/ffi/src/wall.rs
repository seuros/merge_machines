//! The wall-clock reading a hybrid timestamp is stamped at.
//!
//! `Process::CLOCK_REALTIME` is physical time two machines can compare. It
//! steps backwards when the host's clock is corrected, and no deadline may be
//! derived from it.

use std::sync::OnceLock;

use magnus::value::Opaque;
use magnus::{Error, RModule, Ruby, Value, prelude::*};

use crate::{defined_twice, uninitialised};

static PROCESS: OnceLock<Opaque<RModule>> = OnceLock::new();
static REALTIME: OnceLock<Opaque<Value>> = OnceLock::new();

pub fn define(ruby: &Ruby) -> Result<(), Error> {
    let process: RModule = ruby.class_object().const_get("Process")?;
    let realtime: Value = process.const_get("CLOCK_REALTIME")?;
    PROCESS
        .set(process.into())
        .map_err(|_| defined_twice(ruby, "Process"))?;
    REALTIME
        .set(realtime.into())
        .map_err(|_| defined_twice(ruby, "Process::CLOCK_REALTIME"))
}

/// `Process.clock_gettime(Process::CLOCK_REALTIME, :nanosecond)`.
pub fn now_ns(ruby: &Ruby) -> Result<u64, Error> {
    let (Some(process), Some(realtime)) = (PROCESS.get(), REALTIME.get()) else {
        return Err(uninitialised(ruby));
    };
    ruby.get_inner(*process).funcall(
        "clock_gettime",
        (ruby.get_inner(*realtime), ruby.to_symbol("nanosecond")),
    )
}
