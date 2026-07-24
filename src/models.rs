//! Plain data types shared by the whole application: decks, cards, reviews.
//! Holds structs and enums with `serde` derives and nothing else: no
//! persistence, no scheduling rules, no I/O. Every other module depends on this
//! one, and it depends on none of them.

use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

/// Ease factor every new card starts with, as defined by SM-2.
///
/// Reviews move it up or down, but never below `1.3`, which is the floor the
/// algorithm uses to keep hard cards from collapsing to daily repetitions.
const DEFAULT_EASE_FACTOR: f32 = 2.5;

/// Interval a new card starts with: it is due the day it is created.
const INITIAL_INTERVAL_DAYS: u32 = 0;

/// The scheduling state a review produces.
///
/// Lives here, and not in `study/`, so that `study` can produce it and `db` can
/// store it while both depend only on this module: the dependency direction
/// stays `ui -> study/sync/db -> models`, with no sibling knowing the others.
///
/// This is data, not rules. The rules that fill it in are in `study/`, and the
/// invariants they guarantee (the ease floor, the interval minimums) are
/// documented there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scheduling {
    /// Days until the card is due again. `0` means "still today".
    pub interval_days: u32,
    /// The card's new ease factor, never below the floor SM-2 imposes.
    pub ease_factor: f32,
    /// Calendar day the card becomes due, `today + interval_days`.
    pub due_date: NaiveDate,
}

/// A named collection of cards.
#[derive(Debug, Clone, PartialEq)]
pub struct Deck {
    /// Generated on the client so a deck can be created offline and still keep
    /// a stable identity after syncing.
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    /// Last modification, in UTC. Sync resolves conflicts by comparing this
    /// field across devices (last write wins).
    pub updated_at: DateTime<Utc>,
    /// Soft delete: the row stays so the deletion can be propagated to other
    /// devices instead of silently reappearing on the next sync.
    pub deleted: bool,
}

impl Deck {
    /// Creates a deck with a fresh identity.
    ///
    /// The current time arrives as `now` instead of being read here: models
    /// stay free of I/O, and tests can pass a fixed timestamp.
    pub fn new(name: String, now: DateTime<Utc>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            created_at: now,
            updated_at: now,
            deleted: false,
        }
    }
}

/// A single flashcard and its spaced repetition scheduling state.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    /// Generated on the client, like [`Deck::id`].
    pub id: Uuid,
    /// Deck this card belongs to.
    pub deck_id: Uuid,
    /// Prompt shown first, as Markdown.
    pub front: String,
    /// Answer revealed after grading, as Markdown.
    pub back: String,
    /// Optional sentence showing the word in use.
    ///
    /// `None` means the card has no example, which is different from an example
    /// that happens to be empty text.
    pub example: Option<String>,

    /// Days SM-2 waited before showing this card in the current cycle.
    ///
    /// It is the memory of the last spacing, not a countdown: the next interval
    /// is computed from this value multiplied by [`Card::ease_factor`]. A new
    /// card starts at `0`, meaning nothing has been learned yet, and a failed
    /// review resets it to `0` so the card re-enters the learning cycle.
    pub interval_days: u32,

    /// How easy this specific card is for this user, from SM-2.
    ///
    /// It multiplies the interval on every successful review, so a card at
    /// `2.5` roughly stretches its spacing two and a half times each time it is
    /// recalled. Good answers raise it, poor ones lower it, with `1.3` as the
    /// floor. It measures the card's difficulty for one person, not the card's
    /// content.
    pub ease_factor: f32,

    /// Calendar day on which the card should be studied again.
    ///
    /// A card is due when `due_date <= today`. Stored as a date without time or
    /// timezone: "due on the 25th" is a statement about the user's calendar,
    /// not about an instant in time.
    pub due_date: NaiveDate,

    pub created_at: DateTime<Utc>,
    /// Last modification, in UTC. Used by sync conflict resolution.
    pub updated_at: DateTime<Utc>,
    /// Soft delete, like [`Deck::deleted`].
    pub deleted: bool,
}

impl Card {
    /// Creates a card that is due immediately, with the SM-2 defaults for a
    /// card that has never been reviewed.
    ///
    /// `now` and `today` arrive as parameters: reading the clock is the
    /// caller's job, which keeps this module free of I/O and makes scheduling
    /// deterministic in tests.
    pub fn new(
        deck_id: Uuid,
        front: String,
        back: String,
        example: Option<String>,
        now: DateTime<Utc>,
        today: NaiveDate,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            deck_id,
            front,
            back,
            example,
            interval_days: INITIAL_INTERVAL_DAYS,
            ease_factor: DEFAULT_EASE_FACTOR,
            due_date: today,
            created_at: now,
            updated_at: now,
            deleted: false,
        }
    }
}
