//! `MergeMachines::LwwRegister`: one value, held by whichever write was
//! stamped last.

use std::cell::{Cell, RefCell};

use bytes::Bytes;
use magnus::scan_args::{get_kwargs, scan_args};
use magnus::typed_data::Obj;
use magnus::{
    Error, RHash, RModule, RString, Ruby, Symbol, TryConvert, Value, function, method, prelude::*,
};
use merge_machines::{
    CodecError, HybridTimestamp, LwwRegister, Merge, ReplicaId, STAMP_HYBRID, STAMP_LOGICAL, Stamp,
};

use crate::data::{stamped, timestamp};
use crate::hash::hash64;
use crate::names::{self, Names};
use crate::{codec_error, mismatch, optional_kwarg};

/// Which clock stamps a register's writes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// One 64-bit counter per write.
    Logical,
    /// A wall-clock reading and a counter per write.
    Hybrid,
}

impl Kind {
    const fn name(self) -> &'static str {
        match self {
            Self::Logical => "logical",
            Self::Hybrid => "hybrid",
        }
    }
}

/// A register stamped by one of the two clocks.
#[derive(Clone)]
enum Register {
    Logical(Box<LwwRegister<u64, u64>>),
    Hybrid(Box<LwwRegister<u64, HybridTimestamp>>),
}

/// Runs `body` against whichever variant is held.
macro_rules! register {
    ($register:expr, |$inner:ident| $body:expr) => {
        match $register {
            Register::Logical($inner) => $body,
            Register::Hybrid($inner) => $body,
        }
    };
}

/// The register, and the string its value was hashed from.
#[magnus::wrap(class = "MergeMachines::LwwRegister", free_immediately, size)]
struct Wrapper {
    register: RefCell<Register>,
    names: RefCell<Names>,
    written: Cell<bool>,
}

const fn frame_len_of<S: Stamp>(_: &LwwRegister<u64, S>) -> usize {
    LwwRegister::<u64, S>::ENCODED_LEN
}

/// Whether a frame carries a write rather than the state [`boxed_new`]
/// constructs.
fn holds_write<S: Stamp>(register: &LwwRegister<u64, S>) -> bool {
    *register != LwwRegister::new(0, S::default(), 0)
}

/// Each shape is constructed behind its own call frame.
#[inline(never)]
fn boxed_new<S: Stamp>() -> Box<LwwRegister<u64, S>> {
    Box::new(LwwRegister::new(0, S::default(), 0))
}

#[inline(never)]
fn boxed_decode<S: Stamp>(ruby: &Ruby, frame: &[u8]) -> Result<Box<LwwRegister<u64, S>>, Error> {
    let mut register = boxed_new::<S>();
    register
        .decode_into(frame)
        .map_err(|error| codec_error(ruby, error))?;
    Ok(register)
}

impl Wrapper {
    fn new(ruby: &Ruby, args: &[Value]) -> Result<Self, Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let kind = requested_kind(ruby, optional_kwarg(args.keywords, "stamp")?)?;
        Ok(Self::holding(dispatch(ruby, kind, None)?, Names::new()))
    }

    /// Reads a blob written by [`dump`](Self::dump).
    fn load(ruby: &Ruby, blob: Bytes) -> Result<Self, Error> {
        let kind = match blob.get(crate::SHAPE).map(|field| field[0]) {
            Some(STAMP_LOGICAL) => Kind::Logical,
            Some(STAMP_HYBRID) => Kind::Hybrid,
            _ => return Err(codec_error(ruby, CodecError::Parameters)),
        };
        let register = dispatch(ruby, kind, Some(&blob))?;
        let frame_len = register!(&register, |inner| frame_len_of(&**inner));
        let names = names::read(ruby, &blob[frame_len..])?;
        if register!(&register, |inner| holds_write(&**inner)) {
            let held = register!(&register, |inner| *inner.value());
            names::complete(ruby, &names, core::iter::once(held))?;
        }

        Ok(Self::holding(register, names))
    }

    fn holding(register: Register, names: Names) -> Self {
        let written = !names.is_empty();
        Self {
            register: RefCell::new(register),
            names: RefCell::new(names),
            written: Cell::new(written),
        }
    }

    fn kind(&self) -> Kind {
        match &*self.register.borrow() {
            Register::Logical(_) => Kind::Logical,
            Register::Hybrid(_) => Kind::Hybrid,
        }
    }

    /// Which clock stamps this register's writes.
    fn stamp_kind(ruby: &Ruby, rb_self: &Self) -> Symbol {
        ruby.to_symbol(rb_self.kind().name())
    }

    /// Writes `value` if `stamp:` and `replica:` beat the write already held.
    fn set(rb_self: &Self, args: &[Value]) -> Result<(), Error> {
        let args = scan_args::<(String,), (), (), (), RHash, ()>(args)?;
        let (value,) = args.required;
        let (replica, stamp) =
            get_kwargs::<_, (ReplicaId, Value), (), ()>(args.keywords, &["replica", "stamp"], &[])?
                .required;

        let element = hash64(value.as_bytes());
        let kind = match &*rb_self.register.borrow() {
            Register::Logical(_) => Kind::Logical,
            Register::Hybrid(_) => Kind::Hybrid,
        };
        let logical = match kind {
            Kind::Logical => Some(u64::try_convert(stamp)?),
            Kind::Hybrid => None,
        };
        let hybrid_stamp = match kind {
            Kind::Hybrid => Some(hybrid(stamp)?),
            Kind::Logical => None,
        };
        let first = !rb_self.written.replace(true);
        match (&mut *rb_self.register.borrow_mut(), logical, hybrid_stamp) {
            (Register::Logical(register), Some(stamp), _) => {
                if first {
                    **register = LwwRegister::new(element, stamp, replica);
                } else {
                    register.set(element, stamp, replica);
                }
            }
            (Register::Hybrid(register), _, Some(stamp)) => {
                if first {
                    **register = LwwRegister::new(element, stamp, replica);
                } else {
                    register.set(element, stamp, replica);
                }
            }
            _ => unreachable!("the kind was read from the same register"),
        }
        rb_self.names.borrow_mut().insert(element, value);
        rb_self.prune();
        Ok(())
    }

    /// The value being held, `nil` until the first write.
    fn value(&self) -> Option<String> {
        let held = self.held();
        self.names.borrow().get(&held).cloned()
    }

    /// The stamp of the write being held.
    fn stamp(ruby: &Ruby, rb_self: &Self) -> Result<Value, Error> {
        match &*rb_self.register.borrow() {
            Register::Logical(register) => Ok(ruby.into_value(*register.stamp())),
            Register::Hybrid(register) => timestamp(ruby, *register.stamp()),
        }
    }

    /// The replica that wrote what is being held.
    fn replica(&self) -> ReplicaId {
        register!(&*self.register.borrow(), |register| register.replica())
    }

    fn merge(ruby: &Ruby, rb_self: Obj<Self>, other: &Self) -> Result<Obj<Self>, Error> {
        let source = other.register.borrow().clone();
        let names = other.names.borrow().clone();
        match (&mut *rb_self.register.borrow_mut(), &source) {
            (Register::Logical(target), Register::Logical(source)) => {
                let Ok(()) = target.merge(source);
            }
            (Register::Hybrid(target), Register::Hybrid(source)) => {
                let Ok(()) = target.merge(source);
            }
            _ => return Err(mismatch(ruby, "register")),
        }
        rb_self.names.borrow_mut().extend(names);
        rb_self.prune();
        Ok(rb_self)
    }

    /// The core frame followed by the name of the value it holds.
    ///
    /// ```text
    /// core frame   LwwRegister::<u64, S>::ENCODED_LEN bytes, tag 0x71
    /// name record  u64 little-endian byte length, then that many UTF-8 bytes
    /// ```
    ///
    /// The frame holds the value's 64-bit hash, so [`load`](Self::load)
    /// rehashes the name to give [`value`](Self::value) its string back.
    fn dump(ruby: &Ruby, rb_self: &Self) -> RString {
        let frame = register!(&*rb_self.register.borrow(), |register| register.encoded());
        let mut blob = Vec::from(frame);
        names::write(
            &mut blob,
            &rb_self.names.borrow(),
            core::iter::once(rb_self.held()),
        );
        ruby.str_from_slice(&blob)
    }

    fn inspect(&self) -> String {
        format!(
            "#<MergeMachines::LwwRegister stamp={} replica={} value={}>",
            self.kind().name(),
            self.replica(),
            self.value()
                .map_or_else(|| "nil".to_owned(), |value| format!("{value:?}"))
        )
    }

    /// The hash of the value being held.
    fn held(&self) -> u64 {
        register!(&*self.register.borrow(), |register| *register.value())
    }

    /// Drops the names of writes that lost.
    fn prune(&self) {
        let held = self.held();
        self.names
            .borrow_mut()
            .retain(|element, _| *element == held);
    }
}

/// The timestamp a `Timestamp` names.
fn hybrid(value: Value) -> Result<HybridTimestamp, Error> {
    Ok(stamped(
        value.funcall("wall", ())?,
        value.funcall("counter", ())?,
    ))
}

/// Which clock a `stamp:` keyword asks for, defaulting to the logical one.
fn requested_kind(ruby: &Ruby, given: Option<Value>) -> Result<Kind, Error> {
    let Some(given) = given else {
        return Ok(Kind::Logical);
    };
    let name = Symbol::from_value(given).and_then(|symbol| symbol.name().ok());
    match name.as_deref() {
        Some(name) if name == Kind::Logical.name() => Ok(Kind::Logical),
        Some(name) if name == Kind::Hybrid.name() => Ok(Kind::Hybrid),
        _ => Err(Error::new(
            ruby.exception_arg_error(),
            format!(
                "stamp {} is not supported, expected one of [:{}, :{}]",
                given.inspect(),
                Kind::Logical.name(),
                Kind::Hybrid.name()
            ),
        )),
    }
}

/// The variant for `kind`, read from `blob` when there is one.
fn dispatch(ruby: &Ruby, kind: Kind, blob: Option<&[u8]>) -> Result<Register, Error> {
    match (kind, blob) {
        (Kind::Logical, None) => Ok(Register::Logical(boxed_new::<u64>())),
        (Kind::Hybrid, None) => Ok(Register::Hybrid(boxed_new::<HybridTimestamp>())),
        (Kind::Logical, Some(frame)) => Ok(Register::Logical(boxed_decode::<u64>(ruby, frame)?)),
        (Kind::Hybrid, Some(frame)) => Ok(Register::Hybrid(boxed_decode::<HybridTimestamp>(
            ruby, frame,
        )?)),
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = module.define_class("LwwRegister", ruby.class_object())?;
    class.define_singleton_method("new", function!(Wrapper::new, -1))?;
    class.define_singleton_method("load", function!(Wrapper::load, 1))?;
    class.define_method("stamp_kind", method!(Wrapper::stamp_kind, 0))?;
    class.define_method("set", method!(Wrapper::set, -1))?;
    class.define_method("value", method!(Wrapper::value, 0))?;
    class.define_method("stamp", method!(Wrapper::stamp, 0))?;
    class.define_method("replica", method!(Wrapper::replica, 0))?;
    class.define_method("merge", method!(Wrapper::merge, 1))?;
    class.define_method("dump", method!(Wrapper::dump, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
