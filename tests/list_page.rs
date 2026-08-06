//! A page of rows with one of them selected: what moves the selection, where it
//! stops, and what a screen too short for the list shows.
//!
//! The page is driven through [`ScriptedControls`], so every test states what
//! the player did to the panel. Nothing here names a key or a terminal, and the
//! result is read back off the frame the page drew.
//!
//! The page is handed a region shorter than the screen, because that is what a
//! page under chrome gets: a test that gave it the whole frame could not tell a
//! viewport from a screen height.

use motif::device::{Button, DeviceProfile, Encoder, ScreenProfile};
use motif::ui::{
    ControlEvent, Controls, Frame, ListPage, Page, ScriptedControls, Turn, columns_of,
};

const SCREEN: ScreenProfile = DeviceProfile::TARGET.screen;
const MARKER: char = '>';
const WIDE_LABEL: &str = "オーディオ";
const VIEWPORT: usize = 6;

fn pressed(button: Button) -> ControlEvent {
    ControlEvent::Pressed {
        button,
        shifted: false,
    }
}

fn turned(encoder: Encoder, turn: Turn) -> ControlEvent {
    ControlEvent::Turned {
        encoder,
        turn,
        shifted: false,
    }
}

fn numbered(count: usize) -> Vec<String> {
    (0..count).map(|row| format!("row {row}")).collect()
}

fn page_of(count: usize) -> ListPage {
    ListPage::new(numbered(count))
}

fn driven_by(page: &mut ListPage, events: impl IntoIterator<Item = ControlEvent>) {
    let mut controls = ScriptedControls::new(events);
    while let Some(event) = controls.poll() {
        page.control(event);
    }
}

fn moved(count: usize, events: impl IntoIterator<Item = ControlEvent>) -> ListPage {
    let mut page = page_of(count);
    driven_by(&mut page, events);

    page
}

fn repeated(event: ControlEvent, times: usize) -> Vec<ControlEvent> {
    std::iter::repeat_n(event, times).collect()
}

fn row_of(frame: &Frame, row: usize) -> String {
    (0..SCREEN.columns)
        .filter_map(|column| frame.get(column, row))
        .filter(|cell| cell.columns() > 0)
        .map(|cell| cell.glyph())
        .collect()
}

fn drawn_into(page: &mut ListPage, rows: usize) -> Vec<String> {
    let mut frame = Frame::blank();
    let (region, _below) = frame.region().split_top(rows);
    page.draw(region);

    (0..SCREEN.rows)
        .map(|row| row_of(&frame, row).trim_end().to_string())
        .collect()
}

fn drawn(page: &mut ListPage) -> Vec<String> {
    drawn_into(page, VIEWPORT)
}

fn listed(page: &mut ListPage) -> Vec<String> {
    drawn(page)
        .into_iter()
        .filter(|row| !row.is_empty())
        .collect()
}

fn marked(page: &mut ListPage) -> Vec<String> {
    drawn(page)
        .into_iter()
        .filter(|row| row.starts_with(MARKER))
        .collect()
}

#[test]
fn a_new_page_selects_the_first_row() {
    assert_eq!(page_of(4).selected(), Some(0));
}

#[test]
fn an_empty_page_selects_nothing() {
    assert_eq!(page_of(0).selected(), None);
    assert_eq!(page_of(0).selected_row(), None);
}

#[test]
fn a_page_keeps_the_rows_it_was_given() {
    assert_eq!(page_of(3).rows(), ["row 0", "row 1", "row 2"]);
}

#[test]
fn down_moves_the_selection_to_the_next_row() {
    assert_eq!(moved(4, [pressed(Button::Down)]).selected(), Some(1));
}

#[test]
fn up_moves_the_selection_back() {
    let page = moved(
        4,
        [
            pressed(Button::Down),
            pressed(Button::Down),
            pressed(Button::Up),
        ],
    );

    assert_eq!(page.selected(), Some(1));
}

#[test]
fn the_selected_row_is_the_one_the_selection_names() {
    let mut page = moved(4, [pressed(Button::Down)]);

    assert_eq!(page.selected_row(), Some("row 1"));
    assert_eq!(marked(&mut page), ["> row 1"]);
}

#[test]
fn up_at_the_top_stays_at_the_top() {
    let page = moved(4, repeated(pressed(Button::Up), 3));

    assert_eq!(page.selected(), Some(0));
}

#[test]
fn down_at_the_bottom_stays_at_the_bottom() {
    let page = moved(4, repeated(pressed(Button::Down), 9));

    assert_eq!(page.selected(), Some(3));
}

#[test]
fn an_empty_page_ignores_the_arrows() {
    let page = moved(0, [pressed(Button::Down), pressed(Button::Up)]);

    assert_eq!(page.selected(), None);
}

#[test]
fn turning_the_first_encoder_clockwise_moves_down() {
    let page = moved(4, [turned(Encoder::Main, Turn::Clockwise)]);

    assert_eq!(page.selected(), Some(1));
}

#[test]
fn turning_the_first_encoder_anticlockwise_moves_up() {
    let page = moved(
        4,
        [
            turned(Encoder::Main, Turn::Clockwise),
            turned(Encoder::Main, Turn::Clockwise),
            turned(Encoder::Main, Turn::Anticlockwise),
        ],
    );

    assert_eq!(page.selected(), Some(1));
}

#[test]
fn the_encoder_stops_at_the_ends_like_the_arrows() {
    let bottom = moved(4, repeated(turned(Encoder::Main, Turn::Clockwise), 9));
    let top = moved(4, repeated(turned(Encoder::Main, Turn::Anticlockwise), 9));

    assert_eq!(bottom.selected(), Some(3));
    assert_eq!(top.selected(), Some(0));
}

#[test]
fn a_scene_button_leaves_the_selection_alone() {
    let page = moved(
        4,
        [
            pressed(Button::FirstScene),
            pressed(Button::SecondScene),
            pressed(Button::ThirdScene),
            pressed(Button::FourthScene),
        ],
    );

    assert_eq!(page.selected(), Some(0));
}

#[test]
fn the_other_buttons_leave_the_selection_alone() {
    let page = moved(
        4,
        [
            pressed(Button::Down),
            pressed(Button::Left),
            pressed(Button::Right),
            pressed(Button::Play),
            pressed(Button::Stop),
            pressed(Button::Record),
        ],
    );

    assert_eq!(page.selected(), Some(1));
}

#[test]
fn every_row_of_a_short_list_is_drawn() {
    assert_eq!(listed(&mut page_of(3)), ["> row 0", "  row 1", "  row 2"]);
}

#[test]
fn only_the_selected_row_is_marked() {
    let mut page = moved(5, [pressed(Button::Down), pressed(Button::Down)]);

    assert_eq!(marked(&mut page).len(), 1);
}

#[test]
fn an_empty_page_draws_no_rows() {
    assert!(listed(&mut page_of(0)).is_empty());
}

#[test]
fn a_row_wider_than_the_screen_is_clipped() {
    let mut page = ListPage::new(["w".repeat(SCREEN.columns * 2)]);

    assert_eq!(drawn(&mut page)[0].chars().count(), SCREEN.columns);
}

#[test]
fn a_list_longer_than_the_viewport_draws_a_viewport() {
    let mut page = page_of(VIEWPORT * 2);

    assert_eq!(listed(&mut page).len(), VIEWPORT);
}

#[test]
fn the_viewport_is_the_region_the_page_was_given() {
    let mut page = page_of(SCREEN.rows * 2);

    assert_eq!(listed(&mut page).len(), VIEWPORT);
    assert_eq!(
        drawn_into(&mut page, 3)
            .iter()
            .filter(|row| !row.is_empty())
            .count(),
        3
    );
}

#[test]
fn a_list_longer_than_its_region_draws_nothing_below_it() {
    let mut page = page_of(SCREEN.rows * 2);

    let rows = drawn_into(&mut page, VIEWPORT);

    assert!(rows[VIEWPORT..].iter().all(String::is_empty));
}

#[test]
fn a_page_given_one_row_draws_the_selected_row_in_it() {
    let mut page = page_of(SCREEN.rows);
    driven_by(&mut page, repeated(pressed(Button::Down), SCREEN.rows));

    let rows = drawn_into(&mut page, 1);

    assert_eq!(rows[0], format!("> row {}", SCREEN.rows - 1));
    assert!(rows[1..].iter().all(String::is_empty));
}

#[test]
fn a_list_shorter_than_the_viewport_never_scrolls() {
    let mut page = page_of(3);
    driven_by(&mut page, repeated(pressed(Button::Down), 9));

    assert_eq!(listed(&mut page), ["  row 0", "  row 1", "> row 2"]);
}

#[test]
fn moving_below_the_viewport_scrolls_the_next_row_into_view() {
    let last = VIEWPORT;
    let mut page = page_of(last + 4);
    driven_by(&mut page, repeated(pressed(Button::Down), last));

    let rows = listed(&mut page);

    assert_eq!(rows.last(), Some(&format!("> row {last}")));
    assert_eq!(rows.first().map(String::as_str), Some("  row 1"));
}

#[test]
fn moving_back_above_it_scrolls_the_first_row_back() {
    let reach = VIEWPORT + 3;
    let mut page = page_of(VIEWPORT + 4);
    driven_by(&mut page, repeated(pressed(Button::Down), reach));
    driven_by(&mut page, repeated(pressed(Button::Up), reach));

    assert_eq!(
        listed(&mut page).first().map(String::as_str),
        Some("> row 0")
    );
}

#[test]
fn the_selection_is_always_on_screen() {
    let count = VIEWPORT + 7;
    let mut page = page_of(count);

    for _ in 0..count {
        assert_eq!(marked(&mut page).len(), 1);
        driven_by(&mut page, [pressed(Button::Down)]);
    }

    for _ in 0..count {
        assert_eq!(marked(&mut page).len(), 1);
        driven_by(&mut page, [turned(Encoder::Main, Turn::Anticlockwise)]);
    }
}

#[test]
fn a_row_of_wide_glyphs_is_drawn_whole() {
    let mut page = ListPage::new([WIDE_LABEL]);

    assert_eq!(listed(&mut page), vec![format!("> {WIDE_LABEL}")]);
}

#[test]
fn a_row_of_wide_glyphs_stops_at_the_margin() {
    let mut page = ListPage::new([WIDE_LABEL.repeat(20)]);

    let rows = drawn(&mut page);

    assert!(columns_of(&rows[0]) <= SCREEN.columns);
    assert_eq!(rows[1], "");
}
