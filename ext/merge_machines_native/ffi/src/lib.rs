//! Ruby bindings for the `merge-machines` replicated states: a thin mirror.
//!
//! Replica counts and set capacities are const generics, so every class
//! dispatches the number it is given to a fixed set of monomorphised variants
//! and raises `RangeError` for any other number. Elements, values, replica ids,
//! tags and stamps are unsigned 64-bit Integers, as on a core frame.
//!
//! Every refusal comes back as the `snake_case` symbol of its `Refusal` variant
//! and never raises. A frame `decode` refuses raises `RangeError`: it is saved
//! state a restoring constructor turned down.

/// A wrapper's `exact`, inside its `impl`: `value` as exactly the class the
/// file's `CLASS` registry pins, `TypeError` with `message` otherwise.
macro_rules! exact {
    ($class:literal) => {
        #[doc = concat!("`value` as exactly a `", $class, "`: `TypeError` with `message`")]
        /// for anything else, a subclass instance included.
        fn exact(
            ruby: &::magnus::Ruby,
            value: ::magnus::Value,
            message: impl ::matryona_machines::ruby::Message,
        ) -> Result<::magnus::typed_data::Obj<Self>, ::magnus::Error> {
            ::matryona_machines::ruby::exact_typed_with(
                ruby,
                value,
                CLASS.get(ruby)?.get(ruby),
                message,
            )
        }
    };
}
pub(crate) use exact;

/// A wrapper's `==`, inside its `impl`: true for an equal state of exactly
/// the class [`exact!`] pins, false for anything else, never raising.
macro_rules! eq {
    () => {
        /// `==`: an equal state of exactly this class.
        fn eq(ruby: &::magnus::Ruby, rb_self: &Self, other: ::magnus::Value) -> bool {
            Self::exact(ruby, other, || "")
                .is_ok_and(|other| *rb_self.0.borrow() == *other.0.borrow())
        }
    };
}
pub(crate) use eq;

/// A wrapper's `merge`, inside its `impl`: folds in a copy of exactly a
/// `$class` through the held state's `absorb`. `TypeError` naming the class
/// received for another class; `TypeError` naming `$what` once the borrow has
/// ended when `absorb` pairs nothing, as naming it runs `other.inspect`.
macro_rules! merge {
    ($class:literal, $what:literal) => {
        /// `:merged` or the refusal's symbol.
        fn merge(
            ruby: &::magnus::Ruby,
            rb_self: ::magnus::typed_data::Obj<Self>,
            other: ::magnus::Value,
        ) -> Result<::magnus::Symbol, ::magnus::Error> {
            rb_self.check_frozen()?;
            let source = Self::exact(ruby, other, || {
                $crate::expected_class(concat!("a ", $class), other)
            })?
            .0
            .borrow()
            .clone();
            let outcome = rb_self.0.borrow_mut().absorb(&source);
            $crate::merged(ruby, outcome, other, $what)
        }
    };
}
pub(crate) use merge;

/// A wrapper file's `define`: creates `MergeMachines::$name`, pins it in the
/// file's `CLASS`, undefines its allocator and binds `new` (taking `$arity`
/// arguments), `decode`, the listed methods in order, then `==`, `encoded`
/// and `inspect`.
macro_rules! define {
    ($name:literal, new($arity:tt) {
        $($method:literal => $function:ident($args:tt)),* $(,)?
    }) => {
        #[doc = concat!("Defines `MergeMachines::", $name, "` under `module`.")]
        pub fn define(
            ruby: &::magnus::Ruby,
            module: ::magnus::RModule,
        ) -> Result<(), ::magnus::Error> {
            use ::magnus::prelude::*;
            let class = module.define_class($name, ruby.class_object())?;
            CLASS.set(ruby, ::matryona_machines::ruby::Class::pin(ruby, class))?;
            class.undef_default_alloc_func();
            class.define_singleton_method("new", ::magnus::function!(Wrapper::new, $arity))?;
            class.define_singleton_method("decode", ::magnus::function!(Wrapper::decode, 1))?;
            $(class.define_method($method, ::magnus::method!(Wrapper::$function, $args))?;)*
            class.define_method("==", ::magnus::method!(Wrapper::eq, 1))?;
            class.define_method("encoded", ::magnus::method!(Wrapper::encoded, 0))?;
            class.define_method("inspect", ::magnus::method!(Wrapper::inspect, 0))?;
            Ok(())
        }
    };
}
pub(crate) use define;

mod data;
mod gcounter;
mod gset;
mod hybrid;
mod init;
mod logical;
mod lww;
mod orset;
mod pncounter;
mod shape;

use magnus::{Error, Ruby, Symbol, Value, prelude::*};
use matryona_machines::ruby::{Symbols, bytes_with, class_name, symbol};
use merge_machines::Refusal;

/// Where a core frame names the shape it was written at.
pub(crate) const SHAPE: core::ops::Range<usize> = 1..3;

matryona_machines::symbols! {
    /// The symbol each refusal comes back to Ruby as.
    pub(crate) Refusals for Refusal as "a refusal" {
        Full => "full",
        Overflow => "overflow",
        Exhausted => "exhausted",
        Mismatch => "mismatch",
        Truncated => "truncated",
        WrongStructure => "wrong_structure",
        WrongShape => "wrong_shape",
        Malformed => "malformed",
    }
}

pub(crate) fn refusal(ruby: &Ruby, refusal: Refusal) -> Symbol {
    Refusals::symbol(ruby, refusal)
}

/// `honoured` for an honoured unit answer, the refusal's symbol otherwise.
pub(crate) fn unit(ruby: &Ruby, outcome: Result<(), Refusal>, honoured: &str) -> Symbol {
    match outcome {
        Ok(()) => symbol(ruby, honoured),
        Err(refused) => refusal(ruby, refused),
    }
}

/// `:merged` or the refusal's symbol; the `TypeError` naming `what` when
/// `other` held another shape and nothing was folded.
pub(crate) fn merged(
    ruby: &Ruby,
    outcome: Option<Result<(), Refusal>>,
    other: Value,
    what: &str,
) -> Result<Symbol, Error> {
    outcome
        .map(|outcome| unit(ruby, outcome, "merged"))
        .ok_or_else(|| expected(ruby, what, other))
}

/// The `RangeError` for a frame `decode` turned down.
pub(crate) fn refused_frame(ruby: &Ruby, refused: Refusal) -> Error {
    Error::new(
        ruby.exception_range_error(),
        format!("the frame is refused: {}", Refusals::name(refused)),
    )
}

/// A frame for `decode`: the bytes of exactly a String, copied natively.
/// `TypeError` naming the class received (the real class, never an opened
/// singleton), built only on refusal.
pub(crate) fn frame_bytes(frame: Value) -> Result<Vec<u8>, Error> {
    bytes_with(frame, || {
        format!("frame must be a String, not {}", class_name(frame))
    })
}

/// The shape a frame was written at, read from its header.
pub(crate) fn frame_shape(ruby: &Ruby, frame: &[u8]) -> Result<usize, Error> {
    let Some(field) = shape_field(frame) else {
        return Err(refused_frame(ruby, Refusal::Truncated));
    };
    Ok(usize::from(u16::from_le_bytes(field)))
}

/// The two header bytes naming a frame's shape, or `None` for a frame too
/// short to hold them.
pub(crate) fn shape_field(frame: &[u8]) -> Option<[u8; 2]> {
    frame.get(SHAPE).and_then(|field| field.try_into().ok())
}

pub(crate) fn unsupported(ruby: &Ruby, name: &str, given: usize, supported: &[usize]) -> Error {
    Error::new(
        ruby.exception_range_error(),
        format!("{name} {given} is not supported, expected one of {supported:?}"),
    )
}

/// The `TypeError` for an argument of the wrong class.
pub(crate) fn expected(ruby: &Ruby, what: &str, given: Value) -> Error {
    Error::new(ruby.exception_type_error(), expected_message(what, given))
}

/// What [`expected`] says, for a reader that builds its own `TypeError`.
pub(crate) fn expected_message(what: &str, given: Value) -> String {
    format!("expected {what}, got {}", given.inspect())
}

/// The `TypeError` message for a partner state of the wrong class, naming
/// the class received. Built only on refusal.
pub(crate) fn expected_class(what: &str, given: Value) -> String {
    format!("expected {what}, got {}", class_name(given))
}
