//! The shape a class is given, resolved to a monomorphised state.

/// Defines the enum a class holds: one boxed variant per shape `held` is
/// monomorphised at, where `held` is that state at a single shape and `what`
/// names the shape in a `RangeError` ("replicas", "capacity").
///
/// A class is a pointer wide whatever shape it was given. Two states of
/// different shapes are different Rust types, so a pairing of two of them is
/// found by downcasting rather than by a wildcard arm, and comes back as
/// `None` for the class to raise its `TypeError`.
macro_rules! shaped {
    ($name:ident over $held:ident as $what:literal {
        $($variant:ident = $shape:literal),+ $(,)?
    }) => {
        /// A state at one of the monomorphised shapes.
        #[derive(Clone)]
        enum $name {
            $($variant(Box<$held<$shape>>),)+
        }

        impl $name {
            /// The shapes a variant was monomorphised at.
            const SHAPES: &[usize] = &[$($shape),+];

            /// The variant for `shape`, holding nothing: `RangeError` when no
            /// variant was monomorphised at `shape`.
            fn blank(ruby: &::magnus::Ruby, shape: usize) -> Result<Self, ::magnus::Error> {
                match shape {
                    $($shape => Ok(Self::$variant(Box::default())),)+
                    _ => Err($crate::unsupported(ruby, $what, shape, Self::SHAPES)),
                }
            }

            /// The variant for the shape `frame` names, read from `frame`:
            /// `RangeError` for a shape no variant was monomorphised at and
            /// for a frame the core refuses.
            fn decoded(ruby: &::magnus::Ruby, frame: &[u8]) -> Result<Self, ::magnus::Error> {
                let shape = $crate::frame_shape(ruby, frame)?;
                let decoded = match shape {
                    $($shape => {
                        let mut state = Box::<$held<$shape>>::default();
                        state.decode_into(frame).map(|()| Self::$variant(state))
                    })+
                    _ => return Err($crate::unsupported(ruby, $what, shape, Self::SHAPES)),
                };
                decoded.map_err(|refused| $crate::refused_frame(ruby, refused))
            }

            /// The state, for pairing with another of the same shape.
            fn any(&self) -> &dyn ::core::any::Any {
                match self {
                    $(Self::$variant(state) => &**state,)+
                }
            }

            /// Folds `other` in, or `None` when it holds another shape.
            fn absorb(&mut self, other: &Self) -> Option<Result<(), ::merge_machines::Refusal>> {
                let source = other.any();
                match self {
                    $(Self::$variant(target) => source
                        .downcast_ref::<$held<$shape>>()
                        .map(|source| ::merge_machines::Merge::merge(&mut **target, source)),)+
                }
            }

            /// Whether `other` holds the same shape and an equal state.
            fn same(&self, other: &Self) -> bool {
                let other = other.any();
                match self {
                    $(Self::$variant(state) => other
                        .downcast_ref::<$held<$shape>>()
                        .is_some_and(|other| **state == *other),)+
                }
            }
        }
    };
}

pub(crate) use shaped;
