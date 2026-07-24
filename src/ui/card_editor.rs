//! Card editor screen: writes the front and back of a card.
//! Owns the shape of the in-progress draft and saves it through `db/`.

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
/// this screen cannot change the selection or the navigation by accident.
pub(super) fn show(
    ui: &mut egui::Ui,
    connection: &Connection,
    selected_deck: Option<Uuid>,
    draft: &mut CardDraft,
    status: &mut Option<String>,
) {
    ui.heading("Card editor");
    ui.add_space(12.0);

    // Without a deck there is nothing to attach a card to. The `Option` forces
    // this case to be handled instead of guessed.
    let Some(deck_id) = selected_deck else {
        ui.label("Select a deck in the Deck list screen before adding cards.");
        return;
    };

    ui.label("Front");
    ui.add(
        egui::TextEdit::multiline(&mut draft.front)
            .desired_rows(3)
            .desired_width(f32::INFINITY),
    );

    ui.add_space(12.0);

    ui.label("Back");
    ui.add(
        egui::TextEdit::multiline(&mut draft.back)
            .desired_rows(3)
            .desired_width(f32::INFINITY),
    );

    ui.add_space(12.0);

    ui.label("Example (optional)");
    ui.add(
        egui::TextEdit::multiline(&mut draft.example)
            .desired_rows(2)
            .desired_width(f32::INFINITY),
    );

    ui.add_space(16.0);

    let front = draft.front.trim().to_owned();
    let back = draft.back.trim().to_owned();
    let can_save = !front.is_empty() && !back.is_empty();

    if ui
        .add_enabled(can_save, egui::Button::new("Save"))
        .clicked()
    {
        // An empty example is an absent example, not an empty string: the
        // column stores NULL and `Card::example` stays `None`.
        let example = draft.example.trim();
        let example = (!example.is_empty()).then_some(example);

        // Clock read at the edge, then passed down. `date_naive` derives the
        // calendar day from the same instant, so the two cannot disagree.
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
}
