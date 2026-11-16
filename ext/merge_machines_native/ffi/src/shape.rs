//! The shape a class is given, resolved to a monomorphised state.

/// Defines the enum a class holds: one boxed variant per shape `held` is
/// monomorphised at, where `held` is that state at a single shape and `what`
/// names the shape in a `RangeError` ("replicas", "capacity").
///
/// A class is a pointer wide whatever shape it was given. Two states of
/// different shapes are different Rust types, so a pairing of two of them is
/// found by downcasting rather than by a wildcard arm, and comes back as
/// `None` for the class to raise its `TypeError`.
///
/// It also defines `by!(state, |inner| body)`, which runs `body` against
/// whichever variant `state` holds. The leading `$` is passed in for that
/// inner macro's own metavariables.
macro_rules! shaped {
    ($d:tt $name:ident by $by:ident over $held:ident as $what:literal {
        $($variant:ident = $shape:literal),+ $(,)?
    }) => {
        /// A state at one of the monomorphised shapes.
        #[derive(Clone)]
        enum $name {
            $($variant(Box<$held<$shape>>),)+
        }

        /// Runs `body` against whichever variant is held.
        macro_rules! $by {
            ($d state:expr, |$d inner:ident| $d body:expr) => {
                match $d state {
                    $($name::$variant($d inner) => $d body,)+
                }
            };
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

        }

        /// Equal when `other` holds the same shape and an equal state.
        impl PartialEq for $name {
            fn eq(&self, other: &Self) -> bool {
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

/// A shaped class's common methods, inside its `impl`: `new` taking the
/// shape (named `$what` in its errors), `decode` at the shape a frame names,
/// the `$what` reader, `encoded`, and `inspect` showing the shape and
/// `$shown`. `$state` is the [`shaped!`] enum, `$by` its dispatch macro and
/// `$held` the state at one shape.
macro_rules! shaped_methods {
    ($class:literal, $state:ident by $by:ident over $held:ident,
     $what:ident, $shown:ident) => {
        #[doc = concat!("`", $class, "` at the ", stringify!($what), " given.")]
        fn new(ruby: &::magnus::Ruby, $what: ::magnus::Value) -> Result<Self, ::magnus::Error> {
            let $what: usize = ::matryona_machines::ruby::integer($what, stringify!($what))?;
            $state::blank(ruby, $what).map(|held| Self(::std::cell::RefCell::new(held)))
        }

        #[doc = concat!("`", $class, "` decoded, at the ", stringify!($what), " the frame names.")]
        fn decode(ruby: &::magnus::Ruby, frame: ::magnus::Value) -> Result<Self, ::magnus::Error> {
            let frame = $crate::frame_bytes(frame)?;
            $state::decoded(ruby, &frame).map(|held| Self(::std::cell::RefCell::new(held)))
        }

        fn $what(&self) -> usize {
            const fn shape_of<const S: usize>(_: &$held<S>) -> usize {
                S
            }
            $by!(&*self.0.borrow(), |held| shape_of(&**held))
        }

        /// The state's frame.
        fn encoded(ruby: &::magnus::Ruby, rb_self: &Self) -> ::magnus::RString {
            let frame = $by!(&*rb_self.0.borrow(), |held| held.encoded());
            ruby.str_from_slice(&frame)
        }

        fn inspect(&self) -> String {
            format!(
                concat!(
                    "#<",
                    $class,
                    " ",
                    stringify!($what),
                    "={} ",
                    stringify!($shown),
                    "={}>"
                ),
                self.$what(),
                self.$shown()
            )
        }
    };
}

pub(crate) use shaped_methods;

/// A set's readers over unsigned 64-bit elements, inside its `impl`:
/// `include?`, `to_a`, `size` and `empty?`, through the dispatch macro `$by`.
macro_rules! set_readers {
    ($by:ident) => {
        fn includes(&self, element: ::magnus::Value) -> Result<bool, ::magnus::Error> {
            let element: u64 = ::matryona_machines::ruby::integer(element, "element")?;
            Ok($by!(&*self.0.borrow(), |set| set.contains(&element)))
        }

        /// The elements, collected.
        fn to_a(&self) -> Vec<u64> {
            $by!(&*self.0.borrow(), |set| set.iter().collect())
        }

        fn size(&self) -> usize {
            $by!(&*self.0.borrow(), |set| set.len())
        }

        fn is_empty(&self) -> bool {
            $by!(&*self.0.borrow(), |set| set.is_empty())
        }
    };
}

pub(crate) use set_readers;

/// A counter's step `$method(replica, by)`, inside its `impl`: `$honoured`
/// or the refusal's symbol, through the dispatch macro `$by`.
macro_rules! counter_step {
    ($by:ident, $method:ident => $honoured:literal) => {
        #[doc = concat!("`:", $honoured, "` or the refusal's symbol.")]
        fn $method(
            ruby: &::magnus::Ruby,
            rb_self: ::magnus::typed_data::Obj<Self>,
            replica: ::magnus::Value,
            by: ::magnus::Value,
        ) -> Result<::magnus::Symbol, ::magnus::Error> {
            rb_self.check_frozen()?;
            let replica: ::merge_machines::ReplicaId =
                ::matryona_machines::ruby::integer(replica, "replica")?;
            let by: u64 = ::matryona_machines::ruby::integer(by, "by")?;
            let outcome = $by!(&mut *rb_self.0.borrow_mut(), |counter| counter
                .$method(replica, by));
            Ok($crate::unit(ruby, outcome, $honoured))
        }
    };
}

pub(crate) use counter_step;
