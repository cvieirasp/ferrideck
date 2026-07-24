//! Deck list screen: browse the available decks.
//! Renders whatever deck names it is given; loading them is `db/`'s job from
//! milestone M3 on.

use eframe::egui;

/// Draws the deck list.
///
/// Takes a shared slice because the screen only reads: it cannot add, rename or
/// remove a deck, and the signature says so.
pub(super) fn show(ui: &mut egui::Ui, decks: &[String]) {
    ui.heading("Decks");
    ui.add_space(12.0);

    if decks.is_empty() {
        ui.label("No decks yet.");
        return;
    }

    for deck in decks {
        ui.label(deck);
    }
}
