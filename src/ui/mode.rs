//! The screens the instrument has, and the order they sit in.

use crate::closed_set::closed_set;

closed_set! {
    /// A screen the instrument can be showing.
    ///
    /// A closed set rather than a count, for the reason the panel's controls
    /// are one: a mode the application does not have cannot be named, and a
    /// `match` over the set stops compiling when a screen is added. A shell
    /// showing one of several pages is checked by the compiler rather than by
    /// whoever keeps an index in step with an array.
    ///
    /// Only screens that exist are here. A variant named for a page nobody has
    /// built draws nothing, and the first thing done with it is put a row on a
    /// menu that leads nowhere.
    enum Mode;
    /// Every mode, in the order the instrument holds them.
    ///
    /// Order is part of the set rather than an accident of who steps through
    /// it: a scheme that moves to the next mode needs to know what the next one
    /// is, and that is a fact about the instrument. A mode's position here is
    /// its discriminant, so `mode as usize` indexes an array sized by
    /// `ALL.len()` — a page per mode, held in order.
    const ALL;
    /// The looper, where a loop is recorded, layered and played.
    Looper,
}
