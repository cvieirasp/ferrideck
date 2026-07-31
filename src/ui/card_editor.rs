//! Card editor screen: writes the front and back of a card, next to a live
//! preview of how each field will look while studying.
//! Owns the shape of the in-progress draft and saves it through `db/`.

use super::formatting;
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
            // through the renderer would eat the asterisks, print "bold" in bold
            // and teach nothing.
            //
            // It lists exactly what the editor offers: the two shortcuts. Every
            // other CommonMark construct still renders if it is typed, but the
            // editor does not advertise what it cannot write for the user.
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
///
/// `show` is used instead of `ui.add` because the formatting shortcuts need
/// more than the click response: they need the selection and the widget's
/// stored cursor state, and [`egui::TextEdit::show`] is what hands those out.
fn input(ui: &mut egui::Ui, rows: usize, text: &mut String) {
    let output = egui::TextEdit::multiline(text)
        .desired_rows(rows)
        .desired_width(f32::INFINITY)
        .show(ui);

    apply_formatting_shortcut(ui, text, output);
}

/// Applies Ctrl+B or Ctrl+I to this field, if it is the focused one.
///
/// # How the selection is read and written
///
/// In immediate mode the widget does not exist between frames; what survives is
/// an [`egui::text_edit::TextEditState`] in the context's memory, keyed by the
/// widget id.
/// It holds the cursor, and the cursor is a `CCursorRange`: a `primary` and a
/// `secondary` [`egui::text::CCursor`], each a *character* index, in the order
/// the user dragged them rather than in text order.
///
/// `TextEdit::show` returns a [`egui::text_edit::TextEditOutput`] carrying both
/// halves of that: `cursor_range`, the selection as it stands after the widget
/// handled this frame's input, and `state`, the value it is about to remember.
/// So the round trip is:
///
/// 1. read `output.cursor_range` and sort it into a plain `Range<usize>` with
///    `as_sorted_char_range`;
/// 2. compute the new text and the new selection with
///    [`formatting::toggle_wrap`], which is where all the actual logic lives;
/// 3. write the selection back into `state.cursor` and `store` the state under
///    `output.response.id`, which is what makes the widget pick it up on the
///    next frame instead of dropping the cursor to where the old text ended.
///
/// # Why the focus check comes first
///
/// `consume_key` removes the key press from the input queue, so whichever field
/// asks first gets it. Asking only when this field has focus means the three
/// fields cannot fight over one Ctrl+B, and that with no field focused nobody
/// consumes it and nothing happens: no edit, and no cursor to unwrap.
fn apply_formatting_shortcut(
    ui: &egui::Ui,
    text: &mut String,
    output: egui::text_edit::TextEditOutput,
) {
    if !output.response.has_focus() {
        return;
    }

    // A focused field that was never clicked into has no cursor yet. There is
    // no position to insert at, so the shortcut is left for nobody.
    let Some(cursor_range) = output.cursor_range else {
        return;
    };

    let Some(marker) = pressed_marker(ui) else {
        return;
    };

    // `CharIndex` is egui's newtype for "this is a count of characters, not of
    // bytes". Unwrapping it here is safe in the only sense that matters:
    // `toggle_wrap` counts characters too, and says so.
    let selection = cursor_range.as_sorted_char_range();
    let (new_text, new_selection) =
        formatting::toggle_wrap(text, selection.start.0..selection.end.0, marker);

    *text = new_text;

    let mut state = output.state;
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(new_selection.start),
            egui::text::CCursor::new(new_selection.end),
        )));
    state.store(ui.ctx(), output.response.id);

    // The widget drew this frame with the text as it was before the edit, since
    // it had already laid itself out by the time we got here. One more frame is
    // what shows the markers and the restored selection.
    ui.ctx().request_repaint();
}

/// The marker whose shortcut was pressed this frame, if any.
///
/// `Modifiers::COMMAND` is Ctrl everywhere and Cmd on macOS, which is what the
/// user of either platform expects from a text editor.
fn pressed_marker(ui: &egui::Ui) -> Option<&'static str> {
    ui.input_mut(|input| {
        if input.consume_key(egui::Modifiers::COMMAND, egui::Key::B) {
            Some(formatting::BOLD)
        } else if input.consume_key(egui::Modifiers::COMMAND, egui::Key::I) {
            Some(formatting::ITALIC)
        } else {
            None
        }
    })
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

/// The syntax worth remembering, in the syntax itself, with the shortcuts that
/// write it.
const SYNTAX_REMINDER: &str = "**bold** Ctrl+B   *italic* Ctrl+I";

/// Tests of the wiring itself, driving a real `TextEdit` without a window.
///
/// The string logic is covered in `ui/formatting.rs`; what is checked here is
/// only what that module cannot see: that the shortcut reaches the right field,
/// that the selection this screen writes back is the one the widget picks up on
/// the next frame, and that an unfocused editor stays untouched.
///
/// `Context::run_ui` runs one whole frame headlessly, so a test can hand egui a
/// key press and read back what the widget did with it.
#[cfg(test)]
mod wiring_tests {
    use super::*;

    /// Runs one frame containing a single multiline field, feeding it `events`.
    ///
    /// Focus is requested before the widget is drawn, which is the only way to
    /// have it focused during the same frame without simulating a click.
    /// Returns the widget id, which is also the key its state is stored under.
    fn frame(
        ctx: &egui::Context,
        text: &mut String,
        events: Vec<egui::Event>,
        focus: Option<egui::Id>,
    ) -> egui::Id {
        let mut widget_id = egui::Id::NULL;
        let raw = egui::RawInput {
            events,
            ..Default::default()
        };

        // The shapes the frame produced are dropped: these tests read the state
        // the widget stored, not the pixels it would have painted.
        let _ = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                if let Some(id) = focus {
                    ui.memory_mut(|memory| memory.request_focus(id));
                }
                let output = egui::TextEdit::multiline(text).show(ui);
                widget_id = output.response.id;
                apply_formatting_shortcut(ui, text, output);
            });
        });

        widget_id
    }

    fn ctrl(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }
    }

    /// Puts a selection on the field, the way a click and drag would.
    fn select(ctx: &egui::Context, id: egui::Id, range: std::ops::Range<usize>) {
        let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(range.start),
                egui::text::CCursor::new(range.end),
            )));
        state.store(ctx, id);
    }

    /// The selection the field is holding, in character indices.
    fn selection(ctx: &egui::Context, id: egui::Id) -> Option<std::ops::Range<usize>> {
        let state = egui::text_edit::TextEditState::load(ctx, id)?;
        let range = state.cursor.char_range()?.as_sorted_char_range();
        Some(range.start.0..range.end.0)
    }

    #[test]
    fn ctrl_b_wraps_the_selection_and_a_second_press_undoes_it() {
        let ctx = egui::Context::default();
        let mut text = "the cat sat".to_owned();

        let id = frame(&ctx, &mut text, vec![], None);
        select(&ctx, id, 4..7);

        frame(&ctx, &mut text, vec![ctrl(egui::Key::B)], Some(id));
        assert_eq!(text, "the **cat** sat");
        // The word is still selected, which is what makes the second press a
        // toggle rather than a second wrap.
        assert_eq!(selection(&ctx, id), Some(6..9));

        frame(&ctx, &mut text, vec![ctrl(egui::Key::B)], Some(id));
        assert_eq!(text, "the cat sat");
        assert_eq!(selection(&ctx, id), Some(4..7));
    }

    #[test]
    fn ctrl_i_uses_the_italic_marker() {
        let ctx = egui::Context::default();
        let mut text = "the cat sat".to_owned();

        let id = frame(&ctx, &mut text, vec![], None);
        select(&ctx, id, 4..7);

        frame(&ctx, &mut text, vec![ctrl(egui::Key::I)], Some(id));

        assert_eq!(text, "the *cat* sat");
    }

    #[test]
    fn an_unfocused_field_is_left_alone() {
        let ctx = egui::Context::default();
        let mut text = "the cat sat".to_owned();

        let id = frame(&ctx, &mut text, vec![], None);
        select(&ctx, id, 4..7);

        // The same key press, with nothing focused. No edit, no panic.
        frame(&ctx, &mut text, vec![ctrl(egui::Key::B)], None);
        frame(&ctx, &mut text, vec![ctrl(egui::Key::I)], None);

        assert_eq!(text, "the cat sat");
    }
}

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
