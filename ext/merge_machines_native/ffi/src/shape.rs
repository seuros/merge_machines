//! The shape a class is given, resolved to a monomorphised state.

use magnus::{Error, Ruby};
use merge_machines::CodecError;

use crate::codec_error;

/// A state at one const generic, and the core frame it encodes to.
pub trait Framed: Sized {
    /// Length of the core frame this state encodes to.
    const FRAME_LEN: usize;

    /// The state a class starts out holding.
    fn blank() -> Self;

    /// Reads a core frame over a state the caller already holds.
    ///
    /// # Errors
    ///
    /// [`CodecError::Truncated`] when `frame` is too short,
    /// [`CodecError::Tag`] when it holds another structure,
    /// [`CodecError::Parameters`] when it was written at another shape.
    fn read_frame(&mut self, frame: &[u8]) -> Result<(), CodecError>;
}

/// Length of the core frame `state` encodes to.
pub const fn frame_len_of<T: Framed>(_state: &T) -> usize {
    T::FRAME_LEN
}

/// Each shape is constructed behind its own call frame.
#[inline(never)]
pub fn boxed_new<T: Framed>() -> Box<T> {
    Box::new(T::blank())
}

/// Each shape is read behind its own call frame.
#[inline(never)]
pub fn boxed_decode<T: Framed>(ruby: &Ruby, frame: &[u8]) -> Result<Box<T>, Error> {
    let mut state = boxed_new::<T>();
    state
        .read_frame(frame)
        .map_err(|error| codec_error(ruby, error))?;
    Ok(state)
}

/// Defines the enum a class holds: one boxed variant per shape `held` is
/// monomorphised at, where `held` is that state at a single shape, built by
/// `held::new` and read by `held::decode_into`.
///
/// A class is a pointer wide whatever shape it was given, and every variant is
/// built and read through [`boxed_new`] and [`boxed_decode`]. A shape no
/// variant was monomorphised at, and a pairing of two different shapes, come
/// back as `None` for the class to turn into the error its own callers expect.
macro_rules! shaped {
    ($name:ident over $held:ident { $($variant:ident = $shape:literal),+ $(,)? }) => {
        impl<const SHAPE: usize> $crate::shape::Framed for $held<SHAPE> {
            const FRAME_LEN: usize = Self::ENCODED_LEN;

            fn blank() -> Self {
                Self::new()
            }

            fn read_frame(&mut self, frame: &[u8]) -> Result<(), ::merge_machines::CodecError> {
                self.decode_into(frame)
            }
        }

        /// A state at one of the monomorphised shapes.
        #[derive(Clone)]
        enum $name {
            $($variant(Box<$held<$shape>>),)+
        }

        impl $name {
            /// The variant for `shape`, holding nothing, or `None` when no
            /// variant was monomorphised at `shape`.
            fn blank(shape: usize) -> Option<Self> {
                match shape {
                    $($shape => Some(Self::$variant($crate::shape::boxed_new())),)+
                    _ => None,
                }
            }

            /// The variant for `shape`, read from `frame`, or `None` when no
            /// variant was monomorphised at `shape`.
            fn decoded(
                ruby: &::magnus::Ruby,
                shape: usize,
                frame: &[u8],
            ) -> Option<Result<Self, ::magnus::Error>> {
                match shape {
                    $($shape => Some(
                        $crate::shape::boxed_decode(ruby, frame).map(Self::$variant),
                    ),)+
                    _ => None,
                }
            }

            /// Folds `other` in, or `None` when it holds another shape.
            fn absorb(
                &mut self,
                other: &Self,
            ) -> Option<Result<(), ::merge_machines::CapacityError>> {
                match (self, other) {
                    $((Self::$variant(target), Self::$variant(source)) => Some(
                        <$held<$shape> as ::merge_machines::Merge>::merge(target, source),
                    ),)+
                    _ => None,
                }
            }
        }
    };
}

pub(crate) use shaped;
