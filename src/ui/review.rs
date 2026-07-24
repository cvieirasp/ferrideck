//! Review screen: shows one card at a time during a study session.
//! Reports how many cards are due; the session itself needs the SM-2
//! scheduling that arrives with `study/` in milestone M4.

use crate::db;
use chrono::Utc;
use eframe::egui;
use rusqlite::Connection;
use uuid::Uuid;

/// Draws the review screen.
///
/// Reads only: it takes the selected deck by value and cannot change it.
pub(super) fn show(
    ui: &mut egui::Ui,
    connection: &Connection,
    selected_deck: Option<Uuid>,
    status: &mut Option<String>,
) {
    ui.vertical_centered(|ui| {
        ui.add_space(32.0);

        let Some(deck_id) = selected_deck else {
            ui.label("Select a deck in the Deck list screen to start reviewing.");
            return;
        };

        // Known simplification: the due count is queried on every frame, like
        // the deck list. Same reasoning, same fix if it ever matters.
        //
        // The calendar day is derived here, at the edge, and passed down: the
        // scheduling code never reads a clock.
        let due = match db::due_cards(connection, deck_id, Utc::now().date_naive()) {
            Ok(cards) => cards,
            Err(error) => {
                super::report_error(status, &error);
                return;
            }
        };

        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_size(egui::vec2(420.0, 200.0));
            ui.centered_and_justified(|ui| {
                if due.is_empty() {
                    ui.label("Nothing due in this deck today.");
                } else {
                    ui.heading(format!("{} cards due today", due.len()));
                }
            });
        });

        ui.add_space(16.0);

        // Still inert: revealing an answer needs the session state that arrives
        // with `study/`.
        let _ = ui.add_enabled(!due.is_empty(), egui::Button::new("Show answer"));
    });
}
