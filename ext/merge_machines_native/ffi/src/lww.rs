//! `MergeMachines::LwwRegister`: one value, held by whichever write was
//! stamped last.

use std::cell::RefCell;

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
use crate::shape::{Framed, boxed_decode, boxed_new, frame_len_of};
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

/// The two registers are stamped by different clocks.
struct ClockMismatch;

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

impl Register {
    const fn kind(&self) -> Kind {
        match self {
            Self::Logical(_) => Kind::Logical,
            Self::Hybrid(_) => Kind::Hybrid,
        }
    }

    /// The hash of the value being held.
    fn value(&self) -> u64 {
        register!(self, |inner| *inner.value())
    }

    /// Keeps whichever of the two writes was stamped last.
    fn merge(&mut self, other: &Self) -> Result<(), ClockMismatch> {
        match (self, other) {
            (Self::Logical(held), Self::Logical(source)) => {
                let Ok(()) = held.merge(source);
                Ok(())
            }
            (Self::Hybrid(held), Self::Hybrid(source)) => {
                let Ok(()) = held.merge(source);
                Ok(())
            }
            _ => Err(ClockMismatch),
        }
    }
}

/// A register, and whether a write has landed in it.
///
/// A write is kept only when it beats the one being held, so a register that
/// has had none has to say so rather than hold a zeroed write: stamp 0 from
/// replica 0 is a write like any other, and nothing beats it.
#[derive(Clone)]
enum Slot {
    Empty(Register),
    Held(Register),
}

impl Slot {
    const fn register(&self) -> &Register {
        match self {
            Self::Empty(register) | Self::Held(register) => register,
        }
    }

    const fn kind(&self) -> Kind {
        self.register().kind()
    }

    /// The hash of the value being held, `None` until the first write.
    fn held(&self) -> Option<u64> {
        match self {
            Self::Empty(_) => None,
            Self::Held(register) => Some(register.value()),
        }
    }

    /// Folds `other` in: taken outright when no write has landed here yet,
    /// otherwise kept only when it beats the write being held.
    fn merge(&mut self, other: &Self) -> Result<(), ClockMismatch> {
        if self.kind() != other.kind() {
            return Err(ClockMismatch);
        }
        let Self::Held(source) = other else {
            return Ok(());
        };
        if let Self::Held(held) = self {
            held.merge(source)
        } else {
            *self = Self::Held(source.clone());
            Ok(())
        }
    }
}

/// The register, and the string its value was hashed from.
#[magnus::wrap(class = "MergeMachines::LwwRegister", free_immediately, size)]
struct Wrapper {
    slot: RefCell<Slot>,
    names: RefCell<Names>,
}

impl<S: Stamp> Framed for LwwRegister<u64, S> {
    const FRAME_LEN: usize = Self::ENCODED_LEN;

    fn blank() -> Self {
        Self::new(0, S::default(), 0)
    }

    fn read_frame(&mut self, frame: &[u8]) -> Result<(), CodecError> {
        self.decode_into(frame)
    }
}

/// Whether a frame carries a write rather than the state a register starts out
/// holding.
fn holds_write<S: Stamp>(register: &LwwRegister<u64, S>) -> bool {
    *register != LwwRegister::blank()
}

#[inline(never)]
fn boxed_write<S: Stamp>(element: u64, stamp: S, replica: ReplicaId) -> Box<LwwRegister<u64, S>> {
    Box::new(LwwRegister::new(element, stamp, replica))
}

impl Wrapper {
    fn new(ruby: &Ruby, args: &[Value]) -> Result<Self, Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let kind = requested_kind(ruby, optional_kwarg(args.keywords, "stamp")?)?;
        Ok(Self {
            slot: RefCell::new(Slot::Empty(dispatch(ruby, kind, None)?)),
            names: RefCell::new(Names::new()),
        })
    }

    /// Reads a blob written by [`dump`](Self::dump).
    ///
    /// A blob names the value it holds, so one with no name record and a zeroed
    /// frame came from a register nobody wrote.
    fn load(ruby: &Ruby, blob: Bytes) -> Result<Self, Error> {
        let kind = match blob.get(crate::SHAPE).map(|field| field[0]) {
            Some(STAMP_LOGICAL) => Kind::Logical,
            Some(STAMP_HYBRID) => Kind::Hybrid,
            _ => return Err(codec_error(ruby, CodecError::Parameters)),
        };
        let register = dispatch(ruby, kind, Some(&blob))?;
        let frame_len = register!(&register, |inner| frame_len_of(&**inner));
        let names = names::read(ruby, &blob[frame_len..])?;
        let written = !names.is_empty() || register!(&register, |inner| holds_write(&**inner));
        let slot = if written {
            names::complete(ruby, &names, core::iter::once(register.value()))?;
            Slot::Held(register)
        } else {
            Slot::Empty(register)
        };

        Ok(Self {
            slot: RefCell::new(slot),
            names: RefCell::new(names),
        })
    }

    fn kind(&self) -> Kind {
        self.slot.borrow().kind()
    }

    /// Which clock stamps this register's writes.
    fn stamp_kind(ruby: &Ruby, rb_self: &Self) -> Symbol {
        ruby.to_symbol(rb_self.kind().name())
    }

    /// Writes `value` if `stamp:` and `replica:` beat the write already held.
    fn set(ruby: &Ruby, rb_self: &Self, args: &[Value]) -> Result<(), Error> {
        let args = scan_args::<(String,), (), (), (), RHash, ()>(args)?;
        let (value,) = args.required;
        let (replica, stamp) =
            get_kwargs::<_, (ReplicaId, Value), (), ()>(args.keywords, &["replica", "stamp"], &[])?
                .required;

        let element = hash64(value.as_bytes());
        let kind = rb_self.kind();
        let write = match kind {
            Kind::Logical => {
                Register::Logical(boxed_write(element, u64::try_convert(stamp)?, replica))
            }
            Kind::Hybrid => Register::Hybrid(boxed_write(element, hybrid(stamp)?, replica)),
        };
        folded(ruby, &mut rb_self.slot.borrow_mut(), &Slot::Held(write))?;
        rb_self.names.borrow_mut().insert(element, value);
        rb_self.prune();
        Ok(())
    }

    /// The value being held, `nil` until the first write.
    fn value(&self) -> Option<String> {
        let held = self.slot.borrow().held()?;
        self.names.borrow().get(&held).cloned()
    }

    /// The stamp of the write being held.
    fn stamp(ruby: &Ruby, rb_self: &Self) -> Result<Value, Error> {
        match rb_self.slot.borrow().register() {
            Register::Logical(register) => Ok(ruby.into_value(*register.stamp())),
            Register::Hybrid(register) => timestamp(ruby, *register.stamp()),
        }
    }

    /// The replica that wrote what is being held.
    fn replica(&self) -> ReplicaId {
        register!(self.slot.borrow().register(), |register| register.replica())
    }

    fn merge(ruby: &Ruby, rb_self: Obj<Self>, other: &Self) -> Result<Obj<Self>, Error> {
        let source = other.slot.borrow().clone();
        let names = other.names.borrow().clone();
        folded(ruby, &mut rb_self.slot.borrow_mut(), &source)?;
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
        let slot = rb_self.slot.borrow();
        let frame = register!(slot.register(), |register| register.encoded());
        let mut blob = Vec::from(frame);
        names::write(&mut blob, &rb_self.names.borrow(), slot.held().into_iter());
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

    /// Drops the names of writes that lost.
    fn prune(&self) {
        let held = self.slot.borrow().held();
        self.names
            .borrow_mut()
            .retain(|element, _| Some(*element) == held);
    }
}

/// Folds `other` into `slot`, rejecting a register the other clock stamps.
fn folded(ruby: &Ruby, slot: &mut Slot, other: &Slot) -> Result<(), Error> {
    slot.merge(other)
        .map_err(|ClockMismatch| mismatch(ruby, "register"))
}

/// The timestamp a `Timestamp` names.
fn hybrid(value: Value) -> Result<HybridTimestamp, Error> {
    Ok(stamped(
        value.funcall("wall_ns", ())?,
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
        (Kind::Logical, None) => Ok(Register::Logical(boxed_new())),
        (Kind::Hybrid, None) => Ok(Register::Hybrid(boxed_new())),
        (Kind::Logical, Some(frame)) => Ok(Register::Logical(boxed_decode(ruby, frame)?)),
        (Kind::Hybrid, Some(frame)) => Ok(Register::Hybrid(boxed_decode(ruby, frame)?)),
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
