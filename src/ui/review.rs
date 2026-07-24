//! Review screen: runs a study session, one card at a time.
//! Owns the session queue while it lasts; the scheduling decisions come from
//! `study/` and the writes go through `db/`.

use crate::db;
use crate::models::Card;
use crate::study::{self, Rating};
use chrono::Utc;
use eframe::egui;
use rusqlite::Connection;
use std::collections::VecDeque;
use uuid::Uuid;

/// A study session in progress.
///
/// Exists only while the user is reviewing: its absence is represented by
/// `Option<ReviewSession>` being `None`, not by a flag inside the struct.
pub(super) struct ReviewSession {
    /// Cards still to be answered, current one at the front.
    ///
    /// A **stable snapshot** taken when the session started. The database is
    /// deliberately not queried again until the session ends: `apply_review`
    /// pushes the card's `due_date` into the future, so a live query would make
    /// the card the user just answered disappear from under them, and a card
    /// rated `Again` would never come back.
    queue: VecDeque<Card>,

    /// Whether the back of the current card is visible.
    ///
    /// Resets to `false` every time the queue advances, so a new card always
    /// starts hidden.
    revealed: bool,
}

impl ReviewSession {
    /// Starts a session from the cards due right now.
    fn new(due: Vec<Card>) -> Self {
        Self {
            queue: VecDeque::from(due),
            revealed: false,
        }
    }
}

/// Draws the review screen: either the start screen or the session in progress.
pub(super) fn show(
    ui: &mut egui::Ui,
    connection: &Connection,
    selected_deck: Option<Uuid>,
    session: &mut Option<ReviewSession>,
    status: &mut Option<String>,
) {
    // Taking the session out avoids borrowing `session` twice: the running
    // session is owned locally for this frame and put back unless it ended.
    match session.take() {
        Some(mut active) => {
            let finished = run_session(ui, connection, &mut active, status);

            if !finished {
                *session = Some(active);
            }
            // TODO(#35): when the session ends, show a summary of the answers
            // instead of dropping straight back to the start screen.
        }
        None => {
            *session = start_screen(ui, connection, selected_deck, status);
        }
    }
}

/// Draws the start screen and returns a session if the user pressed Start.
fn start_screen(
    ui: &mut egui::Ui,
    connection: &Connection,
    selected_deck: Option<Uuid>,
    status: &mut Option<String>,
) -> Option<ReviewSession> {
    ui.vertical_centered(|ui| {
        ui.add_space(32.0);

        let Some(deck_id) = selected_deck else {
            ui.label("Select a deck in the Deck list screen to start reviewing.");
            return None;
        };

        // Known simplification: the due count is queried on every frame, like
        // the deck list. Cheap and always fresh, which is what this screen
        // wants. Once the session starts, the opposite rule applies: see
        // `ReviewSession::queue`.
        //
        // The calendar day is derived here, at the edge, and passed down: the
        // scheduling code never reads a clock.
        let due = match db::due_cards(connection, deck_id, Utc::now().date_naive()) {
            Ok(cards) => cards,
            Err(error) => {
                super::report_error(status, &error);
                return None;
            }
        };

        // `set_min_size` gives the box its shape without `centered_and_justified`,
        // which would expand the content to fill the whole panel and push
        // everything below it out of view.
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_size(CARD_BOX_SIZE);
            ui.vertical_centered(|ui| {
                ui.add_space(CARD_BOX_SIZE.y / 2.0 - 16.0);

                if due.is_empty() {
                    ui.label("Nothing due in this deck today.");
                } else {
                    ui.heading(format!("{} cards due today", due.len()));
                }
            });
        });

        ui.add_space(16.0);

        let started = ui
            .add_enabled(!due.is_empty(), egui::Button::new("Start"))
            .clicked();

        started.then(|| ReviewSession::new(due))
    })
    .inner
}

/// Draws the current card and applies the rating the user gives it.
///
/// Returns `true` when the queue runs out and the session is over.
fn run_session(
    ui: &mut egui::Ui,
    connection: &Connection,
    session: &mut ReviewSession,
    status: &mut Option<String>,
) -> bool {
    // Keyboard state is read once, before anything is drawn, so both the
    // buttons and the shortcuts feed the same two decisions below.
    let (reveal_shortcut, rating_shortcut) = read_shortcuts(ui.ctx(), session.revealed);

    let mut reveal = reveal_shortcut;
    let mut rating = rating_shortcut;

    let drawn = ui
        .vertical_centered(|ui| {
            let Some(card) = session.queue.front() else {
                return false;
            };

            ui.add_space(16.0);
            ui.small(format!("{} cards left", session.queue.len()));
            ui.add_space(16.0);

            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_size(CARD_BOX_SIZE);
                ui.vertical_centered(|ui| {
                    ui.add_space(16.0);
                    ui.heading(&card.front);

                    if session.revealed {
                        ui.add_space(12.0);
                        ui.separator();
                        ui.add_space(12.0);
                        ui.label(&card.back);

                        if let Some(example) = &card.example {
                            ui.add_space(12.0);
                            ui.small(example);
                        }
                    }
                });
            });

            ui.add_space(16.0);

            if session.revealed {
                ui.horizontal(|ui| {
                    for (index, (label, choice)) in RATINGS.iter().enumerate() {
                        if ui.button(format!("{} ({})", label, index + 1)).clicked() {
                            rating = Some(*choice);
                        }
                    }
                });
                ui.add_space(8.0);
                ui.small("Press 1, 2, 3 or 4 to rate");
            } else {
                if ui.button("Show answer").clicked() {
                    reveal = true;
                }
                ui.add_space(8.0);
                ui.small("Press Space to reveal");
            }

            true
        })
        .inner;

    if !drawn {
        return true;
    }

    if reveal {
        session.revealed = true;
    }

    if let Some(rating) = rating {
        answer(connection, session, rating, status);
    }

    session.queue.is_empty()
}

/// Minimum size of the box holding a card, so the layout does not jump between
/// a short front and a long one.
const CARD_BOX_SIZE: egui::Vec2 = egui::vec2(420.0, 180.0);

/// Buttons offered after the answer is revealed, in keyboard-shortcut order.
const RATINGS: [(&str, Rating); 4] = [
    ("Again", Rating::Again),
    ("Hard", Rating::Hard),
    ("Good", Rating::Good),
    ("Easy", Rating::Easy),
];

/// Applies a rating to the card at the front of the queue.
///
/// `study::schedule` decides, `db::apply_review` persists, and only then does
/// the queue advance: if the write fails, the card stays where it was and the
/// user can try again.
fn answer(
    connection: &Connection,
    session: &mut ReviewSession,
    rating: Rating,
    status: &mut Option<String>,
) {
    let Some(mut card) = session.queue.pop_front() else {
        return;
    };

    // Clock read at the edge, then passed down as parameters.
    let now = Utc::now();
    let scheduling = study::schedule(&card, rating, now.date_naive());

    match db::apply_review(connection, card.id, &scheduling, now) {
        Ok(()) => {
            if rating == Rating::Again {
                // The card comes back later in this session carrying the state
                // that was just written, so rating it a second time compounds
                // from the new values instead of from the stale ones.
                card.interval_days = scheduling.interval_days;
                card.ease_factor = scheduling.ease_factor;
                card.due_date = scheduling.due_date;
                card.updated_at = now;

                session.queue.push_back(card);
            }

            session.revealed = false;
        }
        Err(error) => {
            // Nothing was persisted: put the card back where it was.
            session.queue.push_front(card);
            super::report_error(status, &error);
        }
    }
}

/// Reads the keyboard shortcuts available in the current state.
///
/// Everything is read inside a single `input` call: that closure holds a lock
/// on the context, so calling other context methods from within it would
/// deadlock.
///
/// The `revealed` guard is what makes the shortcuts safe. Space is only
/// accepted while the answer is hidden and the digits only while it is shown,
/// so a key held down across the transition cannot reveal a card and rate it in
/// the same breath.
fn read_shortcuts(ctx: &egui::Context, revealed: bool) -> (bool, Option<Rating>) {
    ctx.input(|input| {
        if !revealed {
            return (input.key_pressed(egui::Key::Space), None);
        }

        let rating = if input.key_pressed(egui::Key::Num1) {
            Some(Rating::Again)
        } else if input.key_pressed(egui::Key::Num2) {
            Some(Rating::Hard)
        } else if input.key_pressed(egui::Key::Num3) {
            Some(Rating::Good)
        } else if input.key_pressed(egui::Key::Num4) {
            Some(Rating::Easy)
        } else {
            None
        };

        (false, rating)
    })
}
