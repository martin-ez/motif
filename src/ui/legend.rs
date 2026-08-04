//! What a page's controls do, drawn where the player can read it.
//!
//! Two halves meet here and neither knows the other. A page declares what its
//! controls mean, in words that belong to the page; a backend says what to call
//! the way each one is reached, in glyphs that belong to the panel. The legend
//! puts the two together, which is what keeps a key out of every page and a
//! meaning out of every backend.
//!
//! What it draws is a picture of the panel: every control is a key with an edge
//! around it, the navigation buttons stand in the cross they are arranged in
//! under the player's thumb, and the encoders and transport run along beside
//! them. A player looking down at the panel and up at the screen sees the same
//! shape twice, which is what makes the legend readable without being read.

use crate::device::{Button, Control, DeviceProfile, Encoder};
use crate::ui::{Cell, Controls, Frame, Hint};

const UNAVAILABLE: &str = "-";

const TOP_LEFT: char = '┌';
const TOP_RIGHT: char = '┐';
const BOTTOM_LEFT: char = '└';
const BOTTOM_RIGHT: char = '┘';
const JOIN_LEFT: char = '├';
const JOIN_RIGHT: char = '┤';
const HORIZONTAL: char = '─';
const VERTICAL: char = '│';

const KEY_WIDTH: usize = 5;
const ENCODER_WIDTH: usize = 7;

const CLUSTER_LEFT_AT: usize = 0;
const CLUSTER_MIDDLE_AT: usize = 6;
const CLUSTER_RIGHT_AT: usize = 12;
const CLUSTER_MEANING_AT: usize = 12;
const KEYS_AT: usize = 22;

const TOP_OF_UP: usize = 0;
const UP: usize = 1;
const TOP_OF_CLUSTER: usize = 2;
const CLUSTER: usize = 3;
const UNDER_CLUSTER: usize = 4;
const CLUSTER_MEANINGS: usize = 5;

/// Which part of the panel picture a button is drawn in.
enum Seat {
    Cluster,
    Row,
}

const fn seat_of(button: Button) -> Seat {
    match button {
        Button::Up | Button::Down | Button::Left | Button::Right => Seat::Cluster,
        Button::Play | Button::Stop | Button::Record => Seat::Row,
    }
}

fn centred(
    frame: &mut Frame,
    row: usize,
    at: usize,
    width: usize,
    count: usize,
    glyphs: impl Iterator<Item = char>,
) {
    let offset = (width - count.min(width)) / 2;

    for (place, glyph) in glyphs.take(width).enumerate() {
        frame.set(at + offset + place, row, Cell::new(glyph));
    }
}

fn centred_text(frame: &mut Frame, row: usize, at: usize, width: usize, text: &str) {
    centred(frame, row, at, width, text.chars().count(), text.chars());
}

fn written(frame: &mut Frame, row: usize, at: usize, ends_at: usize, text: &str) {
    for (place, glyph) in text.chars().enumerate() {
        if at + place >= ends_at {
            break;
        }
        frame.set(at + place, row, Cell::new(glyph));
    }
}

fn edge(frame: &mut Frame, row: usize, at: usize, width: usize, left: char, right: char) {
    frame.set(at, row, Cell::new(left));
    for column in 1..width.saturating_sub(1) {
        frame.set(at + column, row, Cell::new(HORIZONTAL));
    }
    frame.set(at + width.saturating_sub(1), row, Cell::new(right));
}

fn face(frame: &mut Frame, row: usize, at: usize, width: usize, hint: Option<Hint>) {
    frame.set(at, row, Cell::new(VERTICAL));
    frame.set(at + width.saturating_sub(1), row, Cell::new(VERTICAL));

    if let Some(hint) = hint {
        centred(
            frame,
            row,
            at + 1,
            width.saturating_sub(2),
            hint.glyphs().count(),
            hint.glyphs(),
        );
    }
}

/// What each control on the panel does on one page.
///
/// A page answers a handful of controls and ignores the rest, and until it says
/// which, the only way to find out is to press one and watch. Declaring it is
/// what the screen is drawn from, so a control a page does not answer is drawn
/// as unavailable rather than left out — the panel then reads the same
/// everywhere, and a control doing nothing here is a fact on the screen rather
/// than a silence.
///
/// ```
/// use motif::device::{Button, Encoder};
/// use motif::ui::Legend;
///
/// let legend = Legend::blank().naming(Button::Play, "play");
///
/// assert_eq!(legend.meaning(Button::Play), Some("play"));
/// assert_eq!(legend.meaning(Encoder::First), None);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Legend {
    meanings: [Option<&'static str>; Control::ALL.len()],
}

impl Legend {
    /// How many rows a legend takes, along the bottom of the frame.
    ///
    /// It is permanent rather than an overlay a player summons: a legend that
    /// has to be asked for is one more thing to know about before it can help,
    /// and the pages this is drawn for are read while both hands are busy.
    ///
    /// Six rows is what the panel picture costs. Five of them are the
    /// navigation cross — a key is an edge with a glyph inside it, and stacking
    /// two of those with a shared edge is five rows however tightly it is drawn
    /// — and the sixth names the three keys along the bottom of it. The
    /// encoders and the transport run along beside the cross rather than under
    /// it, so they cost no rows of their own.
    ///
    /// A page draws into the whole frame and the legend is drawn over these
    /// rows afterwards, so what a page puts here does not survive the frame.
    pub const ROWS: usize = 6;

    /// A legend for a page that answers nothing yet.
    pub const fn blank() -> Self {
        Self {
            meanings: [None; Control::ALL.len()],
        }
    }

    /// The same legend, with `control` meaning `meaning` on this page.
    ///
    /// Declaring a control twice keeps the later meaning, so a page built on
    /// another page's legend can say what it does differently.
    pub fn naming(mut self, control: impl Into<Control>, meaning: &'static str) -> Self {
        self.meanings[control.into().position()] = Some(meaning);
        self
    }

    /// What `control` does on this page, or `None` where the page ignores it.
    pub fn meaning(&self, control: impl Into<Control>) -> Option<&'static str> {
        self.meanings[control.into().position()]
    }

    /// Draw the panel along the bottom [`ROWS`](Self::ROWS) rows of `frame`,
    /// with each key named the way `panel` reaches it.
    ///
    /// Every key keeps a place of its own whatever it means, so nothing moves
    /// when a page changes what it says; a meaning too long for the key above it
    /// is clipped rather than pushing the next one along. The rows are cleared
    /// first, so nothing drawn underneath shows through the gaps between keys.
    ///
    /// The key at the top of the cross is named beside itself rather than
    /// beneath, because what is beneath it is the rest of the cross.
    pub fn draw(&self, frame: &mut Frame, panel: &impl Controls) {
        let screen = DeviceProfile::TARGET.screen;
        let top = screen.rows.saturating_sub(Self::ROWS);

        for row in top..screen.rows {
            for column in 0..screen.columns {
                frame.set(column, row, Cell::BLANK);
            }
        }

        self.draw_cluster(frame, top, panel);
        self.draw_keys(frame, top, panel);
    }

    fn draw_cluster(&self, frame: &mut Frame, top: usize, panel: &impl Controls) {
        edge(
            frame,
            top + TOP_OF_UP,
            CLUSTER_MIDDLE_AT,
            KEY_WIDTH,
            TOP_LEFT,
            TOP_RIGHT,
        );
        face(
            frame,
            top + UP,
            CLUSTER_MIDDLE_AT,
            KEY_WIDTH,
            panel.hint(Control::Button(Button::Up)),
        );
        written(
            frame,
            top + UP,
            CLUSTER_MEANING_AT,
            KEYS_AT,
            self.said_of(Button::Up),
        );

        edge(
            frame,
            top + TOP_OF_CLUSTER,
            CLUSTER_MIDDLE_AT,
            KEY_WIDTH,
            JOIN_LEFT,
            JOIN_RIGHT,
        );

        for (at, button) in [
            (CLUSTER_LEFT_AT, Button::Left),
            (CLUSTER_MIDDLE_AT, Button::Down),
            (CLUSTER_RIGHT_AT, Button::Right),
        ] {
            if at != CLUSTER_MIDDLE_AT {
                edge(
                    frame,
                    top + TOP_OF_CLUSTER,
                    at,
                    KEY_WIDTH,
                    TOP_LEFT,
                    TOP_RIGHT,
                );
            }
            face(
                frame,
                top + CLUSTER,
                at,
                KEY_WIDTH,
                panel.hint(Control::Button(button)),
            );
            edge(
                frame,
                top + UNDER_CLUSTER,
                at,
                KEY_WIDTH,
                BOTTOM_LEFT,
                BOTTOM_RIGHT,
            );
            centred_text(
                frame,
                top + CLUSTER_MEANINGS,
                at,
                KEY_WIDTH,
                self.said_of(button),
            );
        }
    }

    fn draw_keys(&self, frame: &mut Frame, top: usize, panel: &impl Controls) {
        let mut at = KEYS_AT;

        for encoder in Encoder::ALL {
            self.draw_key(frame, top, at, ENCODER_WIDTH, encoder, panel);
            at += ENCODER_WIDTH;
        }

        for button in Button::ALL {
            if matches!(seat_of(button), Seat::Cluster) {
                continue;
            }
            self.draw_key(frame, top, at, KEY_WIDTH, button, panel);
            at += KEY_WIDTH;
        }
    }

    fn draw_key(
        &self,
        frame: &mut Frame,
        top: usize,
        at: usize,
        width: usize,
        control: impl Into<Control> + Copy,
        panel: &impl Controls,
    ) {
        edge(frame, top, at, width, TOP_LEFT, TOP_RIGHT);
        face(frame, top + 1, at, width, panel.hint(control.into()));
        edge(frame, top + 2, at, width, BOTTOM_LEFT, BOTTOM_RIGHT);
        centred_text(frame, top + 3, at, width, self.said_of(control));
    }

    fn said_of(&self, control: impl Into<Control>) -> &'static str {
        self.meaning(control).unwrap_or(UNAVAILABLE)
    }
}

impl Default for Legend {
    fn default() -> Self {
        Self::blank()
    }
}
