//! Presentation layer: windows, screens and widgets of the desktop app.
//! Renders application state and turns user input into calls to `study`, `db`
//! and `sync`. Never runs SQL or HTTP itself.

use eframe::egui;

/// Root of the application UI and the single owner of all UI state.
///
/// In immediate mode there is no persistent widget tree: every frame is
/// redrawn from this struct, so anything the screen shows must be reachable
/// from here.
///
/// What belongs in this struct is **view state**: which screen is open, what
/// the user typed but has not saved, which row is selected, whether a dialog is
/// open. That state is meaningful only while the window is open and is lost on
/// exit, by design.
///
/// What does **not** belong here is the durable data itself. Decks, cards and
/// review history become owned by `db/` in milestone M3, and `models/` defines
/// their types. From then on this struct holds only a cached copy loaded for
/// display, never the source of truth.
pub struct FerrideckApp {
    /// Which screen is currently visible.
    ///
    /// Placeholder: this becomes a `Screen` enum in the next issue, so that
    /// invalid screen names stop being representable. View state, and it stays
    /// in this struct permanently.
    current_screen: &'static str,

    /// Deck names shown while the UI is built.
    ///
    /// Development-only fake data. In M3 this is replaced by a cached
    /// `Vec<Deck>` loaded through `db/`, refreshed when the data changes rather
    /// than rebuilt every frame.
    decks: Vec<String>,
}

impl Default for FerrideckApp {
    /// Starts on the deck list with fake decks, so the UI has something to show
    /// before `db/` exists.
    fn default() -> Self {
        Self {
            current_screen: "decks",
            decks: vec![
                "English - Vocabulary".to_owned(),
                "English - Phrasal verbs".to_owned(),
                "English - Idioms".to_owned(),
            ],
        }
    }
}

impl eframe::App for FerrideckApp {
    /// Draws one frame. Called by eframe on every repaint, so the work done
    /// here must stay cheap.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::bottom("status_bar").show(ui, |ui| {
            ui.small(format!(
                "{} decks | screen: {} | development data",
                self.decks.len(),
                self.current_screen
            ));
        });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.centered_and_justified(|ui| {
                ui.heading("Ferrideck");
            });
        });
    }
}
