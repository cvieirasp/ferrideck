//! Card editor screen: writes the front and back of a card.
//! Owns the shape of the in-progress draft; persisting it becomes a call into
//! `db/` in milestone M3.

use eframe::egui;

/// Text the user has typed but not saved yet.
///
/// Pure view state: it lives in memory only, and an empty draft is a valid
/// starting point, which is why `Default` can be derived here.
#[derive(Default)]
pub(super) struct CardDraft {
    front: String,
    back: String,
}

/// Draws the card editor.
///
/// Receives only the draft instead of the whole application state, so this
/// screen cannot touch decks or navigation even by accident.
pub(super) fn show(ui: &mut egui::Ui, draft: &mut CardDraft) {
    ui.heading("Card editor");
    ui.add_space(12.0);

    ui.label("Front");
    ui.add(
        egui::TextEdit::multiline(&mut draft.front)
            .desired_rows(4)
            .desired_width(f32::INFINITY),
    );

    ui.add_space(12.0);

    ui.label("Back");
    ui.add(
        egui::TextEdit::multiline(&mut draft.back)
            .desired_rows(4)
            .desired_width(f32::INFINITY),
    );

    ui.add_space(16.0);

    // Disabled until saving is implemented, so the button cannot lie about
    // what it does.
    ui.add_enabled(false, egui::Button::new("Save"));
}
