//! The largest value seen recently, kept across the frames that draw it.
//!
//! A maximum that fell the moment the value did would be gone before anyone
//! looked: the reader here is a person watching a screen rather than another
//! loop, so a spike has to stay up long enough to be seen at all. One held
//! across a window and one being filled, rather than a ring of every reading,
//! which is what bounds this to two words whatever the window spans.

use std::time::Duration;

use crate::device::DeviceProfile;

pub(crate) const FRAMES_IN_A_SECOND: usize = frames_in(Duration::from_secs(1));

const fn frames_in(span: Duration) -> usize {
    let budget = DeviceProfile::TARGET.screen.frame_budget();
    if budget.is_zero() {
        return 0;
    }

    (span.as_nanos() / budget.as_nanos()) as usize
}

#[derive(Debug)]
pub(crate) struct Window {
    span: usize,
    covered: usize,
    holding: f32,
    held: f32,
}

impl Window {
    pub(crate) const fn spanning(span: usize) -> Self {
        Self {
            span,
            covered: 0,
            holding: 0.0,
            held: 0.0,
        }
    }

    pub(crate) fn holding(&mut self, value: f32) -> f32 {
        self.holding = self.holding.max(value);
        self.covered += 1;

        let peak = self.holding.max(self.held);
        if self.covered >= self.span {
            self.held = self.holding;
            self.holding = 0.0;
            self.covered = 0;
        }

        peak
    }
}
