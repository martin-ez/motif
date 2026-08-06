/// A set of named alternatives that is closed, ordered, and lists itself.
///
/// The enum, and a `const ALL` holding every variant in the order they are
/// written. A variant's place in `ALL` is its discriminant, so `variant as
/// usize` indexes an array sized by `ALL.len()`.
///
/// A variant may carry a `#[cfg]`, written after its doc comment. A build the
/// condition excludes has no such variant and no entry for it in `ALL`, so
/// everything after it moves up by one and nothing is left with a gap in it.
///
/// ```
/// motif::closed_set! {
///     /// A reading the instrument can show.
///     enum Reading;
///     /// Every reading this build has, in order.
///     const ALL;
///     /// How loud the input is.
///     Level,
///     /// How much of the frame budget the loop spent.
///     #[cfg(feature = "frame-pace")]
///     Pace,
///     /// How far through the loop the playhead is.
///     Position,
/// }
///
/// assert_eq!(Reading::ALL[0], Reading::Level);
/// assert_eq!(Reading::Position as usize, Reading::ALL.len() - 1);
/// ```
#[macro_export]
macro_rules! closed_set {
    (
        $(#[$set_doc:meta])*
        enum $set:ident;
        $(#[$all_doc:meta])*
        const ALL;
        $(
            $(#[doc = $variant_doc:expr])*
            $(#[cfg($variant_cfg:meta)])?
            $variant:ident,
        )+
    ) => {
        $(#[$set_doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $set {
            $(
                $(#[doc = $variant_doc])*
                $(#[cfg($variant_cfg)])?
                $variant,
            )+
        }

        impl $set {
            const COMPILED_IN: usize = [$($(#[cfg($variant_cfg)])? Self::$variant,)+].len();

            $(#[$all_doc])*
            pub const ALL: [Self; Self::COMPILED_IN] =
                [$($(#[cfg($variant_cfg)])? Self::$variant,)+];
        }
    };
}
