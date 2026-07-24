//! Presentation layer: windows, screens and widgets of the desktop app.
//! Renders application state and turns user input into calls to `study`, `db`
//! and `sync`. Never runs SQL or HTTP itself.

use eframe::egui;

/// The screen currently shown in the main area.
///
/// The set of screens is closed and known at compile time, so an enum makes any
/// other value impossible to represent and forces every `match` to stay
/// exhaustive when a screen is added.
#[derive(Clone, Copy, PartialEq, Default)]
pub enum Screen {
    /// Study session: the app opens here, since reviewing is the daily task.
    #[default]
    Review,
    /// Create and edit cards.
    CardEditor,
    /// Browse decks.
    DeckList,
}

impl Screen {
    /// Every screen, in navigation bar order.
    ///
    /// Adding a variant without adding it here compiles, so this array is the
    /// one place that has to be kept in sync by hand.
    const ALL: [Self; 3] = [Self::Review, Self::CardEditor, Self::DeckList];

    /// Name shown to the user in the navigation bar.
    fn title(self) -> &'static str {
        match self {
            Self::Review => "Review",
            Self::CardEditor => "Card editor",
            Self::DeckList => "Deck list",
        }
    }
}

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
    /// Screen currently visible in the central area.
    ///
    /// View state: it belongs to this struct permanently and is never
    /// persisted.
    current_screen: Screen,

    /// Deck names shown while the UI is built.
    ///
    /// Development-only fake data. In M3 this is replaced by a cached
    /// `Vec<Deck>` loaded through `db/`, refreshed when the data changes rather
    /// than rebuilt every frame.
    decks: Vec<String>,
}

impl Default for FerrideckApp {
    /// Starts on the default screen with fake decks, so the UI has something to
    /// show before `db/` exists.
    fn default() -> Self {
        Self {
            current_screen: Screen::default(),
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
        egui::Panel::top("nav_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                for screen in Screen::ALL {
                    let is_active = self.current_screen == screen;
                    if ui.selectable_label(is_active, screen.title()).clicked() {
                        self.current_screen = screen;
                    }
                }
            });
        });

        egui::Panel::bottom("status_bar").show(ui, |ui| {
            ui.small(format!("{} decks (development data)", self.decks.len()));
        });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.centered_and_justified(|ui| {
                // One arm per screen: each becomes a real screen as milestones
                // land. The match stays exhaustive, so a new variant cannot be
                // forgotten here.
                match self.current_screen {
                    Screen::Review => ui.heading("Review session"),
                    Screen::CardEditor => ui.heading("Card editor"),
                    Screen::DeckList => ui.heading("Deck list"),
                };
            });
        });
    }
}
