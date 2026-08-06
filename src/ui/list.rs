//! A page of rows with one of them selected, moved by the panel's arrows or by
//! turning the encoder.
//!
//! Two routes to the same movement because the panel has both and a player
//! reaches for whichever is nearer. Neither is a key: mapping an arrow key onto
//! [`Button::Up`] belongs to the terminal backend, and this page never learns
//! that one exists.
//!
//! Moving the selection is all the page does. Acting on the selected row needs a
//! gesture the panel has no button for, so it is a decision the pages that have
//! rows worth acting on make, not one a list widget makes for them — and
//! [`Button::Left`] and [`Button::Right`] are left untouched here so that it
//! stays open.

use crate::device::{Button, Encoder};
use crate::ui::{Cell, ControlEvent, Page, Region, Turn};

const MARKER: char = '>';
const MARKER_COLUMN: usize = 0;
const LABEL_COLUMN: usize = 2;

/// A list the player moves a selection through.
///
/// The selection stops at both ends rather than wrapping: a list that wraps
/// gives the player no way to feel where it ends, and every row past the last
/// one costs a turn of the encoder to undo.
///
/// A list longer than its region scrolls by the least that keeps the selection
/// visible, so the rows around it stay where they were and only the edge moves.
/// How many rows that is depends on what the chrome above the page took, so it
/// is settled when the page draws and not before.
///
/// ```
/// use motif::device::Button;
/// use motif::ui::{ControlEvent, ListPage, Page};
///
/// let mut page = ListPage::new(["CoreAudio", "JACK"]);
/// assert_eq!(page.selected_row(), Some("CoreAudio"));
///
/// page.control(ControlEvent::Pressed { button: Button::Down, shifted: false });
///
/// assert_eq!(page.selected_row(), Some("JACK"));
/// ```
pub struct ListPage {
    rows: Vec<String>,
    selected: usize,
    offset: usize,
}

impl ListPage {
    /// A page listing `rows`, with the first of them selected.
    pub fn new(rows: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            rows: rows.into_iter().map(Into::into).collect(),
            selected: 0,
            offset: 0,
        }
    }

    /// Which row is selected, or `None` when the list is empty.
    ///
    /// An empty list is the ordinary case rather than an edge one — a host that
    /// reports no devices produces one — so it answers with nothing selected
    /// instead of pointing at a row that is not there.
    pub fn selected(&self) -> Option<usize> {
        (!self.rows.is_empty()).then_some(self.selected)
    }

    /// The selected row's label, or `None` when the list is empty.
    pub fn selected_row(&self) -> Option<&str> {
        self.rows.get(self.selected).map(String::as_str)
    }

    /// Every row, in the order they were given.
    pub fn rows(&self) -> &[String] {
        &self.rows
    }

    fn towards_the_end(&mut self) {
        if self.selected + 1 < self.rows.len() {
            self.selected += 1;
        }
    }

    fn towards_the_start(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn keep_the_selection_visible(&mut self, visible: usize) {
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + visible {
            self.offset = self.selected + 1 - visible;
        }
    }
}

impl Page for ListPage {
    fn control(&mut self, event: ControlEvent) {
        match event {
            ControlEvent::Pressed {
                button: Button::Down,
                ..
            }
            | ControlEvent::Turned {
                encoder: Encoder::Main,
                turn: Turn::Clockwise,
                ..
            } => self.towards_the_end(),
            ControlEvent::Pressed {
                button: Button::Up, ..
            }
            | ControlEvent::Turned {
                encoder: Encoder::Main,
                turn: Turn::Anticlockwise,
                ..
            } => self.towards_the_start(),
            _ => {}
        }
    }

    fn draw(&mut self, mut region: Region<'_>) {
        self.keep_the_selection_visible(region.rows());
        let visible = self.rows.iter().enumerate().skip(self.offset);

        for (row, (index, label)) in visible.take(region.rows()).enumerate() {
            if Some(index) == self.selected() {
                region.set(MARKER_COLUMN, row, Cell::new(MARKER));
            }
            region.write(LABEL_COLUMN, row, label);
        }
    }
}
