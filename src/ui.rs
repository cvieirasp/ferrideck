//! Presentation layer: windows, screens and widgets of the desktop app.
//! Renders application state and turns user input into calls to `study`, `db`
//! and `sync`. Never runs SQL or HTTP itself.
//!
//! This module owns the application state and the navigation between screens;
//! each screen draws itself in its own submodule.

mod card_editor;
mod deck_list;
mod markdown;
mod review;

use card_editor::CardDraft;
use eframe::egui;
use markdown::CommonMarkCache;
use review::SessionState;
use rusqlite::Connection;
use uuid::Uuid;

/// The screen currently shown in the main area.
///
/// The set of screens is closed and known at compile time, so an enum makes any
/// other value impossible to represent and forces every `match` to stay
/// exhaustive when a screen is added.
#[derive(Clone, Copy, PartialEq, Default)]
enum Screen {
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

/// View state of the application.
///
/// Everything here is meaningful only while the window is open and is lost on
/// exit, by design. The durable data lives in the database and is read through
/// `db/`; this struct only remembers what the user is doing right now.
///
/// Kept apart from the connection so screens can borrow the two independently:
/// see the note on [`FerrideckApp::ui`].
#[derive(Default)]
struct AppState {
    /// Screen currently visible in the central area.
    current_screen: Screen,

    /// Deck the user is working on, shared by every screen.
    ///
    /// `None` means nothing is selected yet, which the card editor and the
    /// review screen treat as "nothing to do here yet" rather than an error.
    selected_deck: Option<Uuid>,

    /// Name being typed in the deck list, before the deck is created.
    new_deck_name: String,

    /// Card being written in the editor, kept here so switching screens does
    /// not discard what the user typed.
    card_draft: CardDraft,

    /// Where the review screen is: idle, reviewing, or showing a summary.
    ///
    /// Each state carries only its own data, so "reviewing with no queue" or
    /// "idle with a summary" cannot be represented. Survives switching screens:
    /// leaving the review tab and coming back resumes where the user was.
    session: SessionState,

    /// Last message for the user: a confirmation or a database failure.
    ///
    /// `None` means there is nothing to report.
    status: Option<String>,
}

/// Root of the application UI and the single owner of all UI state.
///
/// In immediate mode there is no persistent widget tree: every frame is
/// redrawn from this struct, so anything the screen shows must be reachable
/// from here.
pub(crate) struct FerrideckApp {
    /// The open database, owned for the whole life of the window.
    ///
    /// Screens receive it as `&Connection` for the duration of a frame; nothing
    /// else in the application holds on to it.
    connection: Connection,

    /// What the Markdown renderer remembers from one frame to the next.
    ///
    /// Immediate mode throws away every widget at the end of a frame, so a
    /// renderer that needs continuity has nowhere to keep it: a cache created
    /// inside a draw call would be born empty and dropped microseconds later.
    /// It therefore lives here, like every other piece of cross-frame state.
    ///
    /// What it actually holds: a flag saying the image and file loaders have
    /// already been installed into the egui context, so that one-time setup does
    /// not run 60 times a second; the click state of rendered links, which is
    /// written when a link is clicked and read on a later frame; the heading a
    /// fragment link asked to scroll to; and layout caches for scrollable
    /// documents. It is not a cache of parsed Markdown - the source is parsed
    /// again on every frame, which is what makes live preview work.
    ///
    /// Shared by every screen on purpose: one cache serves any number of
    /// documents, keyed internally, so there is no reason to keep several.
    markdown_cache: CommonMarkCache,

    state: AppState,
}

impl FerrideckApp {
    /// Takes ownership of the database connection opened at startup.
    pub(crate) fn new(connection: Connection) -> Self {
        Self {
            connection,
            markdown_cache: CommonMarkCache::default(),
            state: AppState::default(),
        }
    }
}

impl eframe::App for FerrideckApp {
    /// Draws one frame. Called by eframe on every repaint, so the work done
    /// here must stay cheap.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Destructured once so the connection and the individual pieces of view
        // state are borrowed separately. Passing `&mut self` to a screen while
        // also handing it `&self.connection` would be a second borrow of the
        // same value and would not compile.
        let Self {
            connection,
            markdown_cache,
            state,
        } = self;

        egui::Panel::top("nav_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                for screen in Screen::ALL {
                    let is_active = state.current_screen == screen;
                    if ui.selectable_label(is_active, screen.title()).clicked() {
                        state.current_screen = screen;
                    }
                }
            });
        });

        egui::Panel::bottom("status_bar").show(ui, |ui| {
            ui.small(state.status.as_deref().unwrap_or("Ready"));
        });

        egui::CentralPanel::default().show(ui, |ui| {
            // Each screen receives only the state it needs, never the whole
            // application state. The match stays exhaustive, so a new variant
            // cannot be forgotten here.
            match state.current_screen {
                Screen::Review => review::show(
                    ui,
                    connection,
                    state.selected_deck,
                    &mut state.session,
                    markdown_cache,
                    &mut state.status,
                ),
                Screen::CardEditor => card_editor::show(
                    ui,
                    connection,
                    state.selected_deck,
                    &mut state.card_draft,
                    markdown_cache,
                    &mut state.status,
                ),
                Screen::DeckList => deck_list::show(
                    ui,
                    connection,
                    &mut state.selected_deck,
                    &mut state.new_deck_name,
                    &mut state.status,
                ),
            }
        });
    }
}

/// Puts a failed database call on the status line.
///
/// `{:#}` renders the whole `anyhow` chain on one line, so the user sees the
/// action that failed and the underlying cause instead of just the outermost
/// message.
fn report_error(status: &mut Option<String>, error: &anyhow::Error) {
    *status = Some(format!("{error:#}"));
}
