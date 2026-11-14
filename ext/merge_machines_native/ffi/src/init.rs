//! The extension's entry point.

use magnus::{Error, Ruby};

use crate::{data, gcounter, gset, hybrid, logical, lww, orset, pncounter};

/// Defines `MergeMachines` and every class in it, once, when Ruby loads the extension.
#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("MergeMachines")?;
    data::define(ruby, module)?;
    logical::define(ruby, module)?;
    hybrid::define(ruby, module)?;
    gcounter::define(ruby, module)?;
    pncounter::define(ruby, module)?;
    lww::define(ruby, module)?;
    gset::define(ruby, module)?;
    orset::define(ruby, module)
}
