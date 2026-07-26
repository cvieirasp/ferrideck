//! Review screen: runs a study session, one card at a time, and reports how it
//! went. Owns the session queue while it lasts; the scheduling decisions come
//! from `study/` and the writes go through `db/`.

use super::markdown::{CommonMarkCache, render_markdown, render_markdown_as};
use crate::db;
use crate::models::{Card, Scheduling};
use crate::study::{self, Rating};
use chrono::{DateTime, Utc};
use eframe::egui;
use rusqlite::Connection;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Where the review screen is in its life cycle.
///
/// Three states, so an enum rather than an `Option`: `Option` can only say
/// "something or nothing", and a session that just ended is neither. Each
/// variant also carries exactly the data that state needs and no other, which
/// is why there is no way to read a summary while a session is running or a
/// queue after it ended.
#[derive(Default)]
pub(super) enum SessionState {
    /// No session: the start screen is showing.
    #[default]
    Idle,
    /// A session in progress.
    Active(ReviewSession),
    /// A session that just finished, showing its summary.
    Done(SessionSummary),
}

/// A study session in progress.
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

    /// Cards that left the session for good.
    ///
    /// Counts each card once, no matter how many times it was rated `Again`
    /// along the way: it answers "how much did I get through", not "how many
    /// times did I press a button".
    studied: u32,

    /// How many times a card was rated `Again` and sent back into the queue.
    relearns: u32,

    /// When the session started, on the monotonic clock.
    started: Instant,
}

/// What a finished session is worth reporting.
pub(super) struct SessionSummary {
    studied: u32,
    relearns: u32,
    duration: Duration,
}

impl ReviewSession {
    /// Starts a session from the cards due right now.
    fn new(due: Vec<Card>) -> Self {
        Self {
            queue: VecDeque::from(due),
            revealed: false,
            studied: 0,
            relearns: 0,
            // `Instant` is a monotonic reading, not a calendar date: it only
            // ever moves forward and is immune to the user or NTP changing the
            // system clock mid-session. Reading it here is fine because this is
            // the UI edge, the same place `Utc::now()` is read; `study/` still
            // receives every date as a parameter and never reads a clock.
            started: Instant::now(),
        }
    }

    /// The card being reviewed, if any.
    fn current(&self) -> Option<&Card> {
        self.queue.front()
    }

    /// Records the outcome of rating the current card, which has already been
    /// persisted by the caller.
    ///
    /// `Again` sends the card back to the end of the queue carrying the state
    /// that was just written, so rating it a second time compounds from the new
    /// values. Every other rating retires the card and counts it as studied.
    fn record(
        &mut self,
        mut card: Card,
        rating: Rating,
        scheduling: &Scheduling,
        now: DateTime<Utc>,
    ) {
        if rating == Rating::Again {
            self.relearns += 1;

            card.interval_days = scheduling.interval_days;
            card.ease_factor = scheduling.ease_factor;
            card.due_date = scheduling.due_date;
            card.updated_at = now;

            self.queue.push_back(card);
        } else {
            // Counted here, where the card leaves for good, so a card relearned
            // three times still adds exactly one to the total.
            self.studied += 1;
        }

        self.revealed = false;
    }

    /// Whether every card has left the queue.
    fn is_finished(&self) -> bool {
        self.queue.is_empty()
    }

    /// Closes the session into the numbers worth showing.
    fn summary(&self) -> SessionSummary {
        SessionSummary {
            studied: self.studied,
            relearns: self.relearns,
            // Elapsed monotonic time: what a stopwatch would have measured,
            // regardless of what happened to the calendar clock.
            duration: self.started.elapsed(),
        }
    }
}

/// Draws the review screen in whichever state it is in.
pub(super) fn show(
    ui: &mut egui::Ui,
    connection: &Connection,
    selected_deck: Option<Uuid>,
    session: &mut SessionState,
    markdown_cache: &mut CommonMarkCache,
    status: &mut Option<String>,
) {
    // Moving the state out avoids borrowing `session` twice: the current state
    // is owned locally for this frame and the next one is written back. This is
    // what `Option::take` does, generalized to any enum.
    match std::mem::take(session) {
        SessionState::Idle => {
            if let Some(active) = start_screen(ui, connection, selected_deck, status) {
                // A new session starts with a clean status line: whatever was
                // reported before belongs to the previous activity.
                *status = None;
                *session = SessionState::Active(active);
            }
        }

        SessionState::Active(mut active) => {
            run_session(ui, connection, &mut active, markdown_cache, status);

            *session = if active.is_finished() {
                SessionState::Done(active.summary())
            } else {
                SessionState::Active(active)
            };
        }

        SessionState::Done(summary) => {
            if !summary_screen(ui, &summary) {
                *session = SessionState::Done(summary);
            }
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
        // wants: coming back from a finished session shows the new count with
        // no invalidation code. Once the session starts, the opposite rule
        // applies: see `ReviewSession::queue`.
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
fn run_session(
    ui: &mut egui::Ui,
    connection: &Connection,
    session: &mut ReviewSession,
    markdown_cache: &mut CommonMarkCache,
    status: &mut Option<String>,
) {
    // Keyboard state is read once, before anything is drawn, so both the
    // buttons and the shortcuts feed the same two decisions below. Reading it
    // here also puts it ahead of every widget in the frame, including the
    // Markdown viewer: see `read_shortcuts` for why that ordering matters.
    let (reveal_shortcut, rating_shortcut) = read_shortcuts(ui.ctx(), session.revealed);

    let mut reveal = reveal_shortcut;
    let mut rating = rating_shortcut;

    if session.current().is_none() {
        return;
    }

    // The controls are claimed *before* the card area is drawn. A bottom panel
    // takes its height off the region first and hands what is left to whatever
    // comes after, so the scroll area below can never grow into this space and
    // push the buttons out of the window. Reversing these two blocks would undo
    // the whole point of the scroll area.
    egui::Panel::bottom("review_controls").show(ui, |ui| {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
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
        });
        ui.add_space(8.0);
    });

    ui.vertical_centered(|ui| {
        ui.add_space(16.0);
        ui.small(format!("{} cards left", session.queue.len()));
        ui.add_space(16.0);
    });

    // `auto_shrink([false, false])` keeps the viewport at the size the layout
    // gave it instead of collapsing around short content, so the card box does
    // not jump up and down as cards of different lengths go by. That is the
    // same reason `CARD_BOX_SIZE` exists.
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            show_card(ui, session, markdown_cache);
        });

    if reveal {
        session.revealed = true;
    }

    if let Some(rating) = rating {
        answer(connection, session, rating, status);
    }
}

/// Draws the card box: the front always, the back and example once revealed.
///
/// Every field goes through the Markdown renderer, because Markdown is the
/// format they are stored in. Text with no markers renders as exactly that
/// text, so a card written in plain prose looks the same as it did before.
fn show_card(ui: &mut egui::Ui, session: &ReviewSession, markdown_cache: &mut CommonMarkCache) {
    let Some(card) = session.current() else {
        return;
    };

    egui::Frame::group(ui.style()).show(ui, |ui| {
        // A `Frame` is sized by what it contains, so without this the box would
        // be as wide as the longest line and the text would wrap early inside a
        // narrow column. Claiming the available width first turns it into the
        // full-width reading area a card wants; the height stays a floor, so
        // short cards keep the box from collapsing.
        ui.set_width(ui.available_width());
        ui.set_min_height(CARD_BOX_SIZE.y);

        // `vertical`, not `vertical_centered`: centring puts every widget on
        // its own centred row, which is fine for a one-line label and wrong for
        // a rendered document, where wrapped paragraphs and list bullets have
        // to share a left edge.
        ui.vertical(|ui| {
            ui.add_space(16.0);
            render_markdown_as(ui, markdown_cache, &egui::TextStyle::Heading, &card.front);

            if session.revealed {
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(12.0);
                render_markdown(ui, markdown_cache, &card.back);

                if let Some(example) = &card.example {
                    ui.add_space(12.0);
                    render_markdown_as(ui, markdown_cache, &egui::TextStyle::Small, example);
                }
            }
        });
    });
}

/// Draws the end-of-session summary. Returns `true` when the user dismisses it.
fn summary_screen(ui: &mut egui::Ui, summary: &SessionSummary) -> bool {
    ui.vertical_centered(|ui| {
        ui.add_space(32.0);

        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_size(CARD_BOX_SIZE);
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.heading("Session complete");
                ui.add_space(16.0);

                ui.label(format!("{} studied", cards_label(summary.studied)));
                ui.label(relearns_label(summary.relearns));
                ui.label(format!("Time: {}", format_duration(summary.duration)));
            });
        });

        ui.add_space(16.0);

        ui.button("Back to the deck").clicked()
    })
    .inner
}

/// Minimum size of the box holding a card, so the layout does not jump between
/// a short front and a long one.
///
/// The start and summary screens use both dimensions, since their content is a
/// single centred line. The card box takes only the height: its width comes
/// from the space available, so long text has somewhere to go.
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
/// the session advance: if the write fails, the card stays where it was, the
/// counters are untouched and the user can try again.
fn answer(
    connection: &Connection,
    session: &mut ReviewSession,
    rating: Rating,
    status: &mut Option<String>,
) {
    let Some(card) = session.queue.pop_front() else {
        return;
    };

    // Clock read at the edge, then passed down as parameters.
    let now = Utc::now();
    let scheduling = study::schedule(&card, rating, now.date_naive());

    match db::apply_review(connection, card.id, &scheduling, now) {
        Ok(()) => session.record(card, rating, &scheduling, now),
        Err(error) => {
            // Nothing was persisted: put the card back where it was.
            session.queue.push_front(card);
            super::report_error(status, &error);
        }
    }
}

/// Formats a session duration as `mm:ss`.
///
/// Minutes are not wrapped at 60: a 75 minute session reads `75:00`, which is
/// more useful than `01:15:00` for a study session and never ambiguous.
fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

/// Pluralizes the studied count.
fn cards_label(studied: u32) -> String {
    if studied == 1 {
        "1 card".to_owned()
    } else {
        format!("{studied} cards")
    }
}

/// Pluralizes the relearn count, including the case worth celebrating.
fn relearns_label(relearns: u32) -> String {
    match relearns {
        0 => "No relearns".to_owned(),
        1 => "1 relearn".to_owned(),
        count => format!("{count} relearns"),
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
///
/// Rendering the card as Markdown does not interfere. egui delivers a key press
/// to the widget that holds keyboard focus, and focus is only granted to a
/// widget that asks for it: text fields, buttons reached with Tab, and so on.
/// The Markdown viewer emits labels, which never request focus, so nothing on
/// this screen claims the keyboard while a card is being read. What is read here
/// is the frame's raw event list, which is filled before any widget runs, and
/// this call is the first thing `run_session` does - so even a focused widget
/// consuming an event later in the frame cannot take these keys away.
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone as _};

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 24, 10, 0, 0)
            .single()
            .expect("valid timestamp")
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, 24).expect("valid date")
    }

    fn card(front: &str) -> Card {
        Card::new(
            Uuid::nil(),
            front.to_owned(),
            "back".to_owned(),
            None,
            now(),
            today(),
        )
    }

    /// A session of `count` cards, named "0", "1", and so on.
    fn session(count: usize) -> ReviewSession {
        ReviewSession::new((0..count).map(|index| card(&index.to_string())).collect())
    }

    /// The scheduling a rating would have produced, computed by the real
    /// algorithm so the tests stay honest about what gets written back.
    fn scheduling_for(card: &Card, rating: Rating) -> Scheduling {
        study::schedule(card, rating, today())
    }

    #[test]
    fn a_new_session_starts_with_zeroed_counters() {
        let session = session(3);

        assert_eq!(session.studied, 0);
        assert_eq!(session.relearns, 0);
        assert!(!session.revealed);
        assert_eq!(session.queue.len(), 3);
    }

    #[test]
    fn good_retires_the_card_and_counts_it_as_studied() {
        let mut session = session(2);
        let card = session.queue.pop_front().expect("a card");
        let scheduling = scheduling_for(&card, Rating::Good);

        session.record(card, Rating::Good, &scheduling, now());

        assert_eq!(session.studied, 1);
        assert_eq!(session.relearns, 0);
        assert_eq!(session.queue.len(), 1);
    }

    #[test]
    fn again_counts_a_relearn_and_sends_the_card_to_the_back() {
        let mut session = session(2);
        let card = session.queue.pop_front().expect("a card");
        let front = card.front.clone();
        let scheduling = scheduling_for(&card, Rating::Again);

        session.record(card, Rating::Again, &scheduling, now());

        assert_eq!(session.studied, 0);
        assert_eq!(session.relearns, 1);
        assert_eq!(session.queue.len(), 2);

        let requeued = session.queue.back().expect("the requeued card");
        assert_eq!(requeued.front, front);

        // The card carries what was persisted, so a second rating compounds
        // from the new values instead of the stale ones.
        assert_eq!(requeued.interval_days, scheduling.interval_days);
        assert_eq!(requeued.due_date, scheduling.due_date);
        assert!((requeued.ease_factor - scheduling.ease_factor).abs() < 1e-4);
        assert_eq!(requeued.updated_at, now());
    }

    #[test]
    fn a_relearned_card_is_still_counted_only_once() {
        let mut session = session(1);

        let card = session.queue.pop_front().expect("a card");
        let scheduling = scheduling_for(&card, Rating::Again);
        session.record(card, Rating::Again, &scheduling, now());

        let card = session.queue.pop_front().expect("the requeued card");
        let scheduling = scheduling_for(&card, Rating::Good);
        session.record(card, Rating::Good, &scheduling, now());

        assert_eq!(session.studied, 1);
        assert_eq!(session.relearns, 1);
        assert!(session.is_finished());
    }

    #[test]
    fn advancing_hides_the_answer_again() {
        let mut session = session(2);
        session.revealed = true;

        let card = session.queue.pop_front().expect("a card");
        let scheduling = scheduling_for(&card, Rating::Good);
        session.record(card, Rating::Good, &scheduling, now());

        assert!(!session.revealed);
    }

    #[test]
    fn a_session_is_finished_only_when_the_queue_empties() {
        let mut session = session(2);
        assert!(!session.is_finished());

        for _ in 0..2 {
            let card = session.queue.pop_front().expect("a card");
            let scheduling = scheduling_for(&card, Rating::Good);
            session.record(card, Rating::Good, &scheduling, now());
        }

        assert!(session.is_finished());
    }

    #[test]
    fn the_summary_carries_the_counters() {
        let mut session = session(2);

        let card = session.queue.pop_front().expect("a card");
        let scheduling = scheduling_for(&card, Rating::Again);
        session.record(card, Rating::Again, &scheduling, now());

        let summary = session.summary();

        assert_eq!(summary.studied, 0);
        assert_eq!(summary.relearns, 1);
    }

    #[test]
    fn durations_are_formatted_as_minutes_and_seconds() {
        assert_eq!(format_duration(Duration::from_secs(0)), "00:00");
        assert_eq!(format_duration(Duration::from_secs(9)), "00:09");
        assert_eq!(format_duration(Duration::from_secs(59)), "00:59");
        assert_eq!(format_duration(Duration::from_secs(60)), "01:00");
        assert_eq!(format_duration(Duration::from_secs(605)), "10:05");
        // Minutes keep counting past an hour instead of wrapping.
        assert_eq!(format_duration(Duration::from_secs(3661)), "61:01");
        // Sub-second remainders are truncated, never rounded up.
        assert_eq!(format_duration(Duration::from_millis(1999)), "00:01");
    }

    #[test]
    fn labels_are_pluralized() {
        assert_eq!(cards_label(0), "0 cards");
        assert_eq!(cards_label(1), "1 card");
        assert_eq!(cards_label(2), "2 cards");

        assert_eq!(relearns_label(0), "No relearns");
        assert_eq!(relearns_label(1), "1 relearn");
        assert_eq!(relearns_label(3), "3 relearns");
    }
}
