//! Card editor screen: writes the front and back of a card, next to a live
//! preview of how each field will look while studying.
//! Owns the shape of the in-progress draft and saves it through `db/`.

use super::markdown::{CommonMarkCache, render_markdown_as};
use crate::db;
use chrono::Utc;
use eframe::egui;
use rusqlite::Connection;
use uuid::Uuid;

/// Text the user has typed but not saved yet.
///
/// Pure view state: it lives in memory only, and an empty draft is a valid
/// starting point, which is why `Default` can be derived here.
#[derive(Default)]
pub(super) struct CardDraft {
    front: String,
    back: String,
    example: String,
}

impl CardDraft {
    /// Empties the draft after a successful save.
    fn clear(&mut self) {
        self.front.clear();
        self.back.clear();
        self.example.clear();
    }
}

/// Draws the card editor.
///
/// Receives the selected deck by value and only the draft as mutable state, so
/// this screen cannot change the selection or the navigation by accident. The
/// Markdown cache is borrowed for the frame, the same one the review screen
/// draws with.
pub(super) fn show(
    ui: &mut egui::Ui,
    connection: &Connection,
    selected_deck: Option<Uuid>,
    draft: &mut CardDraft,
    markdown_cache: &mut CommonMarkCache,
    status: &mut Option<String>,
) {
    // Everything the screen draws goes inside the scroll area, not just the
    // fields: three inputs each followed by a preview is already taller than a
    // small window when the two are stacked, and a preview grows with whatever
    // the user types. Same shape as the card box in the review screen.
    //
    // `auto_shrink([false, false])` keeps the viewport at the size the layout
    // gave it. The width matters here: the inputs ask for `f32::INFINITY` and
    // the layout decision below reads `available_width`, so a viewport that
    // shrank around its content would feed both of them a moving target.
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.heading("Card editor");
            ui.add_space(12.0);

            // Without a deck there is nothing to attach a card to. The `Option`
            // forces this case to be handled instead of guessed.
            let Some(deck_id) = selected_deck else {
                ui.label("Select a deck in the Deck list screen before adding cards.");
                return;
            };

            // Drawn as a plain label on purpose. This line is a reminder of the
            // syntax, so it has to show the markers themselves; sending it
            // through the renderer would print "bold italic" as a list item and
            // teach nothing.
            ui.label(
                egui::RichText::new(SYNTAX_REMINDER)
                    .monospace()
                    .small()
                    .weak(),
            );
            ui.add_space(12.0);

            // Each field renders in the style the review screen gives it, so
            // the preview is not merely "the same Markdown", it is the same
            // drawing call with the same arguments.
            field(
                ui,
                markdown_cache,
                "Front",
                &egui::TextStyle::Heading,
                3,
                &mut draft.front,
            );
            ui.add_space(12.0);

            field(
                ui,
                markdown_cache,
                "Back",
                &egui::TextStyle::Body,
                3,
                &mut draft.back,
            );
            ui.add_space(12.0);

            field(
                ui,
                markdown_cache,
                "Example (optional)",
                &egui::TextStyle::Small,
                2,
                &mut draft.example,
            );

            ui.add_space(16.0);

            let front = draft.front.trim().to_owned();
            let back = draft.back.trim().to_owned();
            let can_save = !front.is_empty() && !back.is_empty();

            if ui
                .add_enabled(can_save, egui::Button::new("Save"))
                .clicked()
            {
                // An empty example is an absent example, not an empty string:
                // the column stores NULL and `Card::example` stays `None`.
                let example = draft.example.trim();
                let example = (!example.is_empty()).then_some(example);

                // Clock read at the edge, then passed down. `date_naive`
                // derives the calendar day from the same instant, so the two
                // cannot disagree.
                let now = Utc::now();

                match db::create_card(
                    connection,
                    deck_id,
                    &front,
                    &back,
                    example,
                    now,
                    now.date_naive(),
                ) {
                    Ok(card) => {
                        draft.clear();
                        *status = Some(format!("Card \"{}\" saved", card.front));
                    }
                    Err(error) => super::report_error(status, &error),
                }
            }
        });
}

/// Draws one labelled field: the text input and its rendered preview.
///
/// `text` is the single source for both halves. The input writes into it and
/// the preview reads it back in the same frame, which is the whole mechanism
/// behind "live": there is no event, no listener and nothing to keep in sync,
/// because there is only one value and it is read after it is written.
///
/// `style` is the paragraph style the review screen uses for this field, and
/// `rows` is the height the input starts at, which the preview box matches so
/// the two sides line up before anything is typed.
fn field(
    ui: &mut egui::Ui,
    cache: &mut CommonMarkCache,
    label: &str,
    style: &egui::TextStyle,
    rows: usize,
    text: &mut String,
) {
    ui.label(label);

    if side_by_side(ui.available_width()) {
        // `columns` splits the width the layout currently has into equal parts
        // and hands each one its own `Ui`. Inside a column, `available_width`
        // is that column's width, so the input and the preview each fill their
        // half without either of them knowing the window size.
        ui.columns(2, |columns| {
            input(&mut columns[0], rows, text);
            preview(&mut columns[1], cache, style, rows, text);
        });
    } else {
        input(ui, rows, text);
        ui.add_space(4.0);
        preview(ui, cache, style, rows, text);
    }
}

/// Draws the editable half of a field.
fn input(ui: &mut egui::Ui, rows: usize, text: &mut String) {
    ui.add(
        egui::TextEdit::multiline(text)
            .desired_rows(rows)
            .desired_width(f32::INFINITY),
    );
}

/// Draws the rendered half of a field.
///
/// An empty field is not a mistake, it is where every card starts, so the box
/// stays and shows a hint. Nothing about it says "error": it is the same weak
/// colour the rest of the app uses for secondary text, in italics so it cannot
/// be mistaken for content the user typed.
fn preview(
    ui: &mut egui::Ui,
    cache: &mut CommonMarkCache,
    style: &egui::TextStyle,
    rows: usize,
    text: &str,
) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        // A `Frame` is sized by what it contains, so without this the box would
        // shrink to the width of the hint and jump to a different width as soon
        // as a word is typed. The height is a floor, matching the rows the
        // input starts with, and grows with the rendered content.
        ui.set_width(ui.available_width());
        ui.set_min_height(ui.text_style_height(&egui::TextStyle::Body) * rows as f32);

        // `vertical`, not `vertical_centered`: a rendered document needs its
        // wrapped lines and list bullets to share a left edge.
        ui.vertical(|ui| {
            if shows_hint(text) {
                ui.label(egui::RichText::new(PREVIEW_HINT).italics().weak());
            } else {
                render_markdown_as(ui, cache, style, text);
            }
        });
    });
}

/// Whether a field's preview area shows the hint instead of rendered content.
///
/// Trimmed, so a field holding only spaces or a stray newline shows the hint
/// rather than an empty box that looks broken.
fn shows_hint(text: &str) -> bool {
    text.trim().is_empty()
}

/// Whether a field is drawn as input and preview side by side.
///
/// One comparison against one constant, deliberately. A layout with two states
/// needs a number to switch at, and reading the width the layout actually has
/// is enough to find it; anything more would be machinery for a decision that
/// fits on this line.
fn side_by_side(available_width: f32) -> bool {
    available_width >= SIDE_BY_SIDE_MIN_WIDTH
}

/// Width, in points, from which a field splits into two columns.
///
/// Below it each half would be under 350 points, which is too narrow for both
/// writing and reading: text wraps every few words on both sides. The default
/// window is 900 points wide, so the editor opens side by side and stacks when
/// the user makes the window noticeably smaller.
const SIDE_BY_SIDE_MIN_WIDTH: f32 = 700.0;

/// Placeholder shown in the preview area of an empty field.
const PREVIEW_HINT: &str = "preview";

/// The syntax worth remembering, in the syntax itself.
const SYNTAX_REMINDER: &str = "**bold**   *italic*   - list";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cleared_draft_is_empty() {
        let mut draft = CardDraft {
            front: "front".to_owned(),
            back: "back".to_owned(),
            example: "example".to_owned(),
        };

        draft.clear();

        assert!(draft.front.is_empty());
        assert!(draft.back.is_empty());
        assert!(draft.example.is_empty());
    }

    #[test]
    fn only_blank_fields_show_the_hint() {
        assert!(shows_hint(""));
        assert!(shows_hint("   "));
        assert!(shows_hint("\n\t "));

        assert!(!shows_hint("a"));
        assert!(!shows_hint("  **bold**  "));
    }

    #[test]
    fn the_layout_switches_at_the_threshold() {
        assert!(side_by_side(SIDE_BY_SIDE_MIN_WIDTH));
        assert!(side_by_side(SIDE_BY_SIDE_MIN_WIDTH + 1.0));
        assert!(side_by_side(1920.0));

        assert!(!side_by_side(SIDE_BY_SIDE_MIN_WIDTH - 1.0));
        assert!(!side_by_side(320.0));
        // A collapsed or not yet measured region stacks rather than splitting
        // into two unusable columns.
        assert!(!side_by_side(0.0));
    }
}
