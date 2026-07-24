//! Deck list screen: browse the available decks and create new ones.
//! Reads and writes through `db/`; it holds no data of its own.

use crate::db;
use chrono::Utc;
use eframe::egui;
use rusqlite::Connection;
use uuid::Uuid;

/// Draws the deck list.
///
/// Owns the selection: this is the only screen that changes which deck the rest
/// of the application is working on.
pub(super) fn show(
    ui: &mut egui::Ui,
    connection: &Connection,
    selected_deck: &mut Option<Uuid>,
    new_deck_name: &mut String,
    status: &mut Option<String>,
) {
    ui.heading("Decks");
    ui.add_space(12.0);

    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(new_deck_name)
                .hint_text("New deck name")
                .desired_width(260.0),
        );

        // Trimmed, so a name made only of spaces cannot be created.
        let name = new_deck_name.trim().to_owned();
        let can_add = !name.is_empty();

        if ui.add_enabled(can_add, egui::Button::new("Add")).clicked() {
            // The clock is read here, at the edge of the application, and the
            // instant travels down as a parameter. Nothing below this line
            // reads the current time.
            match db::create_deck(connection, &name, Utc::now()) {
                Ok(deck) => {
                    *status = Some(format!("Deck \"{}\" created", deck.name));
                    *selected_deck = Some(deck.id);
                    new_deck_name.clear();
                }
                Err(error) => super::report_error(status, &error),
            }
        }
    });

    ui.add_space(12.0);
    ui.separator();
    ui.add_space(12.0);

    // Known simplification: the deck list is queried on every frame. SQLite
    // reads from a local file and the list is tiny, so this is not measurable
    // today. If it ever shows up in a profile, the fix is to cache the list in
    // `AppState` and invalidate it after create/rename/delete.
    match db::list_decks(connection) {
        Ok(decks) if decks.is_empty() => {
            ui.label("No decks yet. Type a name above and press Add.");
        }
        Ok(decks) => {
            for deck in decks {
                let is_selected = *selected_deck == Some(deck.id);
                if ui.selectable_label(is_selected, &deck.name).clicked() {
                    *selected_deck = Some(deck.id);
                    *status = Some(format!("Deck \"{}\" selected", deck.name));
                }
            }
        }
        Err(error) => super::report_error(status, &error),
    }
}
