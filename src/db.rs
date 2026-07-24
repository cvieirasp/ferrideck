//! All SQLite access: connection setup, schema migrations and queries.
//! Translates rows into `models` types so no SQL leaks into the rest of the
//! application.

use crate::models::{Card, Deck, Scheduling};
use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use directories::ProjectDirs;
use rusqlite::{Connection, Row, params};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

/// Reverse-domain parts used to locate the per-user data directory.
const APP_QUALIFIER: &str = "com";
const APP_ORGANIZATION: &str = "cvieirasp";
const APP_NAME: &str = "ferrideck";

/// Database file name inside the data directory.
const DATABASE_FILE: &str = "ferrideck.db";

/// Schema version this build expects. Bumped when a migration is added.
const SCHEMA_VERSION: i64 = 1;

/// Tables of the current schema.
///
/// `IF NOT EXISTS` makes running this on every start harmless; real changes to
/// existing tables will need numbered migrations driven by `schema_version`.
const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS decks (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    deleted     INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS cards (
    id            TEXT PRIMARY KEY,
    deck_id       TEXT NOT NULL REFERENCES decks(id),
    front         TEXT NOT NULL,
    back          TEXT NOT NULL,
    example       TEXT,
    interval_days INTEGER NOT NULL,
    ease_factor   REAL NOT NULL,
    due_date      TEXT NOT NULL,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    deleted       INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER NOT NULL
);
";

/// Opens the local database, creating the file and the schema on first run.
///
/// Called once at startup, before the window opens, so a broken database fails
/// with a readable message instead of an empty UI.
pub fn init() -> Result<Connection> {
    let directory = data_directory()?;

    fs::create_dir_all(&directory)
        .with_context(|| format!("creating the data directory {}", directory.display()))?;

    let path = directory.join(DATABASE_FILE);
    let connection = Connection::open(&path)
        .with_context(|| format!("opening the database at {}", path.display()))?;

    // Off by default in SQLite, and per connection: see the module notes.
    connection
        .pragma_update(None, "foreign_keys", true)
        .context("enabling foreign key enforcement")?;

    create_schema(&connection)?;

    Ok(connection)
}

/// Per-user data directory for this application.
///
/// On Windows this resolves to `%APPDATA%\cvieirasp\ferrideck\data`.
fn data_directory() -> Result<PathBuf> {
    let project_dirs = ProjectDirs::from(APP_QUALIFIER, APP_ORGANIZATION, APP_NAME)
        .context("could not determine a per-user data directory for this platform")?;

    Ok(project_dirs.data_dir().to_path_buf())
}

/// Creates every table and seeds the schema version.
///
/// Shared by [`init`] and the tests, so what is tested is the same SQL the
/// application runs. Idempotent: safe to call on every start.
fn create_schema(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(SCHEMA_SQL)
        .context("creating the database schema")?;

    let versions: i64 = connection
        .query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))
        .context("reading the schema version")?;

    if versions == 0 {
        connection
            .execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                [SCHEMA_VERSION],
            )
            .context("seeding the schema version")?;
    }

    Ok(())
}

/// Inserts a new deck and returns it.
///
/// The identity and the timestamps are decided by [`Deck::new`], so the value
/// returned here is exactly what was written: no second read is needed.
pub fn create_deck(connection: &Connection, name: &str, now: DateTime<Utc>) -> Result<Deck> {
    let deck = Deck::new(name.to_owned(), now);

    connection
        .execute(
            "INSERT INTO decks (id, name, created_at, updated_at, deleted)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                deck.id.to_string(),
                deck.name,
                format_timestamp(deck.created_at),
                format_timestamp(deck.updated_at),
                deck.deleted,
            ],
        )
        .with_context(|| format!("inserting deck {}", deck.id))?;

    Ok(deck)
}

/// Lists the decks that have not been deleted, ordered by name.
///
/// `COLLATE NOCASE` keeps the list in the order a reader expects: SQLite's
/// default collation compares bytes, which would sort every capitalized name
/// before every lowercase one.
pub fn list_decks(connection: &Connection) -> Result<Vec<Deck>> {
    let mut statement = connection
        .prepare(
            "SELECT id, name, created_at, updated_at, deleted
             FROM decks
             WHERE deleted = 0
             ORDER BY name COLLATE NOCASE",
        )
        .context("preparing the deck list query")?;

    let decks = statement
        .query_map([], row_to_deck)
        .context("running the deck list query")?
        .collect::<rusqlite::Result<Vec<Deck>>>()
        .context("reading decks")?;

    Ok(decks)
}

/// Renames a deck and refreshes its `updated_at`.
///
/// A missing id is not an error: see [`delete_deck`] for the reasoning.
pub fn rename_deck(
    connection: &Connection,
    id: Uuid,
    new_name: &str,
    now: DateTime<Utc>,
) -> Result<()> {
    connection
        .execute(
            "UPDATE decks SET name = ?2, updated_at = ?3 WHERE id = ?1",
            params![id.to_string(), new_name, format_timestamp(now)],
        )
        .with_context(|| format!("renaming deck {id}"))?;

    Ok(())
}

/// Soft-deletes a deck: the row stays, flagged and with a fresh `updated_at`.
///
/// Deleting an id that does not exist succeeds and changes nothing. Deletion is
/// idempotent by nature, and sync makes that concrete: the same delete can
/// arrive twice, or after the row was already removed locally, and neither case
/// is a failure the user should see. `execute` still reports how many rows
/// changed, so a caller that needs to distinguish the two can be served later
/// without touching this SQL.
pub fn delete_deck(connection: &Connection, id: Uuid, now: DateTime<Utc>) -> Result<()> {
    connection
        .execute(
            "UPDATE decks SET deleted = 1, updated_at = ?2 WHERE id = ?1",
            params![id.to_string(), format_timestamp(now)],
        )
        .with_context(|| format!("deleting deck {id}"))?;

    Ok(())
}

/// Inserts a new card with the SM-2 defaults and returns it.
///
/// `now` and `today` are separate on purpose: one records when the row was
/// written, the other decides the calendar day the card becomes due.
pub fn create_card(
    connection: &Connection,
    deck_id: Uuid,
    front: &str,
    back: &str,
    example: Option<&str>,
    now: DateTime<Utc>,
    today: NaiveDate,
) -> Result<Card> {
    let card = Card::new(
        deck_id,
        front.to_owned(),
        back.to_owned(),
        example.map(String::from),
        now,
        today,
    );

    connection
        .execute(
            "INSERT INTO cards (
                 id, deck_id, front, back, example,
                 interval_days, ease_factor, due_date,
                 created_at, updated_at, deleted
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                card.id.to_string(),
                card.deck_id.to_string(),
                card.front,
                card.back,
                card.example,
                card.interval_days,
                card.ease_factor,
                format_date(card.due_date),
                format_timestamp(card.created_at),
                format_timestamp(card.updated_at),
                card.deleted,
            ],
        )
        .with_context(|| format!("inserting card {}", card.id))?;

    Ok(card)
}

/// Lists the cards of a deck that have not been deleted, oldest first.
pub fn list_cards(connection: &Connection, deck_id: Uuid) -> Result<Vec<Card>> {
    let mut statement = connection
        .prepare(
            "SELECT id, deck_id, front, back, example,
                    interval_days, ease_factor, due_date,
                    created_at, updated_at, deleted
             FROM cards
             WHERE deck_id = ?1 AND deleted = 0
             ORDER BY created_at",
        )
        .context("preparing the card list query")?;

    let cards = statement
        .query_map(params![deck_id.to_string()], row_to_card)
        .context("running the card list query")?
        .collect::<rusqlite::Result<Vec<Card>>>()
        .context("reading cards")?;

    Ok(cards)
}

/// Lists the cards of a deck that are due on or before `today`.
///
/// The comparison is a plain text comparison: ISO-8601 dates sort
/// lexicographically in the same order they sort chronologically.
pub fn due_cards(connection: &Connection, deck_id: Uuid, today: NaiveDate) -> Result<Vec<Card>> {
    let mut statement = connection
        .prepare(
            "SELECT id, deck_id, front, back, example,
                    interval_days, ease_factor, due_date,
                    created_at, updated_at, deleted
             FROM cards
             WHERE deck_id = ?1 AND deleted = 0 AND due_date <= ?2
             ORDER BY due_date",
        )
        .context("preparing the due cards query")?;

    let cards = statement
        .query_map(
            params![deck_id.to_string(), format_date(today)],
            row_to_card,
        )
        .context("running the due cards query")?
        .collect::<rusqlite::Result<Vec<Card>>>()
        .context("reading due cards")?;

    Ok(cards)
}

/// Edits the content of a card, leaving its scheduling untouched.
///
/// `interval_days`, `ease_factor` and `due_date` are deliberately absent: they
/// belong to the SM-2 algorithm in `study/`, and fixing a typo on the back of a
/// card must not reset how well it is known.
///
/// **Write contract:** this is the only function that writes card content.
/// Scheduling is written only by [`apply_review`]. Neither should ever grow the
/// other one's columns.
///
/// A missing id is not an error, as in [`delete_deck`].
pub fn update_card(
    connection: &Connection,
    id: Uuid,
    front: &str,
    back: &str,
    example: Option<&str>,
    now: DateTime<Utc>,
) -> Result<()> {
    connection
        .execute(
            "UPDATE cards
             SET front = ?2, back = ?3, example = ?4, updated_at = ?5
             WHERE id = ?1",
            params![id.to_string(), front, back, example, format_timestamp(now)],
        )
        .with_context(|| format!("updating card {id}"))?;

    Ok(())
}

/// Soft-deletes a card, with the same semantics as [`delete_deck`].
pub fn delete_card(connection: &Connection, id: Uuid, now: DateTime<Utc>) -> Result<()> {
    connection
        .execute(
            "UPDATE cards SET deleted = 1, updated_at = ?2 WHERE id = ?1",
            params![id.to_string(), format_timestamp(now)],
        )
        .with_context(|| format!("deleting card {id}"))?;

    Ok(())
}

/// Stores the outcome of a review: the new scheduling of a card.
///
/// The `Scheduling` comes from [`crate::study::schedule`], which computed it
/// from the card and the user's rating. This function only writes it down: it
/// makes no scheduling decision of its own.
///
/// **Write contract:** this is the only function that writes `interval_days`,
/// `ease_factor` and `due_date`. Card content is written only by
/// [`update_card`]. Keeping the two apart is what guarantees that reviewing a
/// card cannot alter its text and that editing its text cannot erase how well
/// it is known.
///
/// A missing id is not an error, as in [`delete_deck`]: a review of a card that
/// was deleted on another device is a no-op, not a failure.
pub fn apply_review(
    connection: &Connection,
    card_id: Uuid,
    scheduling: &Scheduling,
    now: DateTime<Utc>,
) -> Result<()> {
    connection
        .execute(
            "UPDATE cards
             SET interval_days = ?2, ease_factor = ?3, due_date = ?4, updated_at = ?5
             WHERE id = ?1",
            params![
                card_id.to_string(),
                scheduling.interval_days,
                scheduling.ease_factor,
                format_date(scheduling.due_date),
                format_timestamp(now),
            ],
        )
        .with_context(|| format!("applying a review to card {card_id}"))?;

    Ok(())
}

/// Builds a [`Deck`] from a row of the `decks` table.
///
/// Every `SELECT` on decks goes through this function, so the column order and
/// the text formats are defined in exactly one place.
fn row_to_deck(row: &Row<'_>) -> rusqlite::Result<Deck> {
    let id: String = row.get("id")?;
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;

    Ok(Deck {
        id: parse_uuid(&id)?,
        name: row.get("name")?,
        created_at: parse_timestamp(&created_at)?,
        updated_at: parse_timestamp(&updated_at)?,
        deleted: row.get("deleted")?,
    })
}

/// Builds a [`Card`] from a row of the `cards` table.
///
/// Every `SELECT` on cards goes through this function, so the column order and
/// the text formats are defined in exactly one place.
fn row_to_card(row: &Row<'_>) -> rusqlite::Result<Card> {
    let id: String = row.get("id")?;
    let deck_id: String = row.get("deck_id")?;
    let due_date: String = row.get("due_date")?;
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;

    Ok(Card {
        id: parse_uuid(&id)?,
        deck_id: parse_uuid(&deck_id)?,
        front: row.get("front")?,
        back: row.get("back")?,
        example: row.get("example")?,
        interval_days: row.get("interval_days")?,
        ease_factor: row.get("ease_factor")?,
        due_date: parse_date(&due_date)?,
        created_at: parse_timestamp(&created_at)?,
        updated_at: parse_timestamp(&updated_at)?,
        deleted: row.get("deleted")?,
    })
}

/// Formats a calendar date as `YYYY-MM-DD`, the only format the date columns
/// hold, so that text order matches calendar order.
fn format_date(value: NaiveDate) -> String {
    value.format("%Y-%m-%d").to_string()
}

/// Parses a date written by [`format_date`].
fn parse_date(value: &str) -> rusqlite::Result<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(conversion_failure)
}

/// Formats a timestamp the way every timestamp column stores it.
///
/// Always UTC, always the same width, always ending in `Z`, so that comparing
/// and ordering the text gives the same answer as comparing the instants.
fn format_timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// Parses a timestamp written by [`format_timestamp`].
fn parse_timestamp(value: &str) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|parsed| parsed.with_timezone(&Utc))
        .map_err(conversion_failure)
}

/// Parses a UUID stored as text.
fn parse_uuid(value: &str) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(value).map_err(conversion_failure)
}

/// Wraps a parse failure as a rusqlite error, so row mapping can use `?`.
///
/// Reports the stored text as unreadable rather than pretending the row is
/// fine: a malformed date or UUID means the database was corrupted or written
/// by something else.
fn conversion_failure<E>(error: E) -> rusqlite::Error
where
    E: std::error::Error + Send + Sync + 'static,
{
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;

    /// A fixed instant, so tests never read the clock.
    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, day, hour, 0, 0)
            .single()
            .expect("valid timestamp")
    }

    /// A fixed calendar day in the same month as [`at`].
    fn on(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, day).expect("valid date")
    }

    /// A database with one deck, for the card tests.
    fn connection_with_deck() -> (Connection, Uuid) {
        let connection = test_connection();
        let deck = create_deck(&connection, "Deck", at(24, 10)).expect("creating a deck");
        (connection, deck.id)
    }

    /// Opens an isolated in-memory database with the production schema.
    fn test_connection() -> Connection {
        let connection = Connection::open_in_memory().expect("in-memory database");
        create_schema(&connection).expect("schema creation");
        connection
    }

    /// Names of the tables in a database, sorted.
    fn table_names(connection: &Connection) -> Vec<String> {
        let mut statement = connection
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .expect("querying sqlite_master");

        statement
            .query_map([], |row| row.get(0))
            .expect("reading table names")
            .collect::<rusqlite::Result<Vec<String>>>()
            .expect("collecting table names")
    }

    #[test]
    fn creates_every_table() {
        let connection = test_connection();

        assert_eq!(
            table_names(&connection),
            vec![
                "cards".to_owned(),
                "decks".to_owned(),
                "schema_version".to_owned()
            ]
        );
    }

    #[test]
    fn seeds_the_schema_version() {
        let connection = test_connection();

        let version: i64 = connection
            .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .expect("reading the schema version");

        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn running_twice_leaves_a_single_version_row() {
        let connection = test_connection();
        create_schema(&connection).expect("second schema creation");

        let versions: i64 = connection
            .query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))
            .expect("counting schema versions");

        assert_eq!(versions, 1);
    }

    #[test]
    fn cards_reference_a_deck() {
        let connection = test_connection();
        connection
            .pragma_update(None, "foreign_keys", true)
            .expect("enabling foreign keys");

        let orphan = connection.execute(
            "INSERT INTO cards (
                 id, deck_id, front, back, example,
                 interval_days, ease_factor, due_date, created_at, updated_at
             ) VALUES (
                 'card-1', 'missing-deck', 'front', 'back', NULL,
                 0, 2.5, '2026-07-24', '2026-07-24T00:00:00Z', '2026-07-24T00:00:00Z'
             )",
            [],
        );

        assert!(
            orphan.is_err(),
            "a card pointing at a missing deck must be rejected"
        );
    }

    #[test]
    fn create_then_list_returns_the_same_deck() {
        let connection = test_connection();
        let now = at(24, 10);

        let created = create_deck(&connection, "English - Idioms", now).expect("creating a deck");
        let listed = list_decks(&connection).expect("listing decks");

        assert_eq!(listed, vec![created.clone()]);
        assert_eq!(created.name, "English - Idioms");
        assert_eq!(created.created_at, now);
        assert_eq!(created.updated_at, now);
        assert!(!created.deleted);
    }

    #[test]
    fn list_orders_by_name_ignoring_case() {
        let connection = test_connection();
        let now = at(24, 10);

        create_deck(&connection, "Banana", now).expect("creating Banana");
        create_deck(&connection, "apple", now).expect("creating apple");

        let names: Vec<String> = list_decks(&connection)
            .expect("listing decks")
            .into_iter()
            .map(|deck| deck.name)
            .collect();

        // A byte-wise ORDER BY would put "Banana" first, because 'B' < 'a'.
        assert_eq!(names, vec!["apple".to_owned(), "Banana".to_owned()]);
    }

    #[test]
    fn list_excludes_deleted_decks() {
        let connection = test_connection();

        let kept = create_deck(&connection, "Kept", at(24, 10)).expect("creating the kept deck");
        let removed =
            create_deck(&connection, "Removed", at(24, 10)).expect("creating the removed deck");

        delete_deck(&connection, removed.id, at(25, 9)).expect("deleting a deck");

        let listed = list_decks(&connection).expect("listing decks");

        assert_eq!(listed, vec![kept]);
    }

    #[test]
    fn rename_changes_the_name_and_the_timestamp() {
        let connection = test_connection();
        let created_at = at(24, 10);
        let renamed_at = at(25, 15);

        let deck = create_deck(&connection, "Old name", created_at).expect("creating a deck");
        rename_deck(&connection, deck.id, "New name", renamed_at).expect("renaming a deck");

        let listed = list_decks(&connection).expect("listing decks");
        let stored = listed.first().expect("one deck");

        assert_eq!(stored.name, "New name");
        assert_eq!(stored.updated_at, renamed_at);
        assert_ne!(stored.updated_at, deck.updated_at);
        // Creation time is history and must survive an edit.
        assert_eq!(stored.created_at, created_at);
    }

    #[test]
    fn deleting_a_missing_deck_succeeds_and_changes_nothing() {
        let connection = test_connection();
        let kept = create_deck(&connection, "Kept", at(24, 10)).expect("creating a deck");

        delete_deck(&connection, Uuid::new_v4(), at(25, 9)).expect("deleting a missing deck");

        assert_eq!(list_decks(&connection).expect("listing decks"), vec![kept]);
    }

    #[test]
    fn create_card_applies_the_sm2_defaults() {
        let (connection, deck_id) = connection_with_deck();

        let card = create_card(
            &connection,
            deck_id,
            "to gather",
            "reunir",
            None,
            at(24, 11),
            on(24),
        )
        .expect("creating a card");

        assert_eq!(card.interval_days, 0);
        assert_eq!(card.ease_factor, 2.5);
        assert_eq!(card.due_date, on(24));

        let stored = list_cards(&connection, deck_id).expect("listing cards");
        assert_eq!(stored, vec![card]);
    }

    #[test]
    fn due_cards_includes_today_and_the_past_but_not_the_future() {
        let (connection, deck_id) = connection_with_deck();
        let today = on(25);

        let yesterday_card = create_card(&connection, deck_id, "y", "y", None, at(24, 9), on(24))
            .expect("yesterday");
        let today_card =
            create_card(&connection, deck_id, "t", "t", None, at(24, 10), on(25)).expect("today");
        create_card(&connection, deck_id, "m", "m", None, at(24, 11), on(26)).expect("tomorrow");

        let due = due_cards(&connection, deck_id, today).expect("listing due cards");

        assert_eq!(due, vec![yesterday_card, today_card]);
    }

    #[test]
    fn update_card_edits_content_without_touching_scheduling() {
        let (connection, deck_id) = connection_with_deck();

        let card = create_card(
            &connection,
            deck_id,
            "front",
            "back",
            Some("old example"),
            at(24, 10),
            on(24),
        )
        .expect("creating a card");

        update_card(
            &connection,
            card.id,
            "new front",
            "new back",
            Some("new example"),
            at(26, 8),
        )
        .expect("updating a card");

        let stored = list_cards(&connection, deck_id).expect("listing cards");
        let stored = stored.first().expect("one card");

        assert_eq!(stored.front, "new front");
        assert_eq!(stored.back, "new back");
        assert_eq!(stored.example.as_deref(), Some("new example"));
        assert_eq!(stored.updated_at, at(26, 8));

        // Scheduling belongs to `study/`: editing text must not reset it.
        assert_eq!(stored.interval_days, card.interval_days);
        assert_eq!(stored.ease_factor, card.ease_factor);
        assert_eq!(stored.due_date, card.due_date);
        assert_eq!(stored.created_at, card.created_at);
    }

    #[test]
    fn deleted_cards_are_excluded_from_both_queries() {
        let (connection, deck_id) = connection_with_deck();

        let kept = create_card(
            &connection,
            deck_id,
            "kept",
            "kept",
            None,
            at(24, 10),
            on(24),
        )
        .expect("creating the kept card");
        let removed = create_card(
            &connection,
            deck_id,
            "removed",
            "removed",
            None,
            at(24, 11),
            on(24),
        )
        .expect("creating the removed card");

        delete_card(&connection, removed.id, at(25, 9)).expect("deleting a card");

        assert_eq!(
            list_cards(&connection, deck_id).expect("listing cards"),
            vec![kept.clone()]
        );
        assert_eq!(
            due_cards(&connection, deck_id, on(25)).expect("listing due cards"),
            vec![kept]
        );
    }

    #[test]
    fn example_survives_a_round_trip_in_both_shapes() {
        let (connection, deck_id) = connection_with_deck();

        create_card(
            &connection,
            deck_id,
            "with",
            "with",
            Some("She gathered the papers."),
            at(24, 10),
            on(24),
        )
        .expect("creating a card with an example");
        create_card(
            &connection,
            deck_id,
            "without",
            "without",
            None,
            at(24, 11),
            on(24),
        )
        .expect("creating a card without an example");

        let examples: Vec<Option<String>> = list_cards(&connection, deck_id)
            .expect("listing cards")
            .into_iter()
            .map(|card| card.example)
            .collect();

        assert_eq!(
            examples,
            vec![Some("She gathered the papers.".to_owned()), None]
        );
    }

    /// Scheduling values are floats: comparing them with `==` would be fragile,
    /// so ease assertions allow a slack far smaller than any real change.
    fn assert_ease(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1e-4,
            "expected ease {expected}, got {actual}"
        );
    }

    #[test]
    fn apply_review_writes_scheduling_and_leaves_content_alone() {
        let (connection, deck_id) = connection_with_deck();

        let card = create_card(
            &connection,
            deck_id,
            "to gather",
            "reunir",
            Some("She gathered the papers."),
            at(24, 10),
            on(24),
        )
        .expect("creating a card");

        let scheduling = Scheduling {
            interval_days: 6,
            ease_factor: 2.6,
            due_date: on(30),
        };

        apply_review(&connection, card.id, &scheduling, at(24, 18)).expect("applying a review");

        let stored = list_cards(&connection, deck_id).expect("listing cards");
        let stored = stored.first().expect("one card");

        assert_eq!(stored.interval_days, 6);
        assert_ease(stored.ease_factor, 2.6);
        assert_eq!(stored.due_date, on(30));
        assert_eq!(stored.updated_at, at(24, 18));

        // Content is off limits to this function.
        assert_eq!(stored.front, card.front);
        assert_eq!(stored.back, card.back);
        assert_eq!(stored.example, card.example);
        assert_eq!(stored.created_at, card.created_at);
        assert!(!stored.deleted);
    }

    #[test]
    fn applying_a_review_to_a_missing_card_succeeds_and_changes_nothing() {
        let (connection, deck_id) = connection_with_deck();

        let card = create_card(
            &connection,
            deck_id,
            "front",
            "back",
            None,
            at(24, 10),
            on(24),
        )
        .expect("creating a card");

        let scheduling = Scheduling {
            interval_days: 6,
            ease_factor: 2.6,
            due_date: on(30),
        };

        apply_review(&connection, Uuid::new_v4(), &scheduling, at(24, 18))
            .expect("reviewing a missing card");

        assert_eq!(
            list_cards(&connection, deck_id).expect("listing cards"),
            vec![card]
        );
    }

    #[test]
    fn a_second_review_overwrites_the_first() {
        let (connection, deck_id) = connection_with_deck();

        let card = create_card(
            &connection,
            deck_id,
            "front",
            "back",
            None,
            at(24, 10),
            on(24),
        )
        .expect("creating a card");

        let first = Scheduling {
            interval_days: 6,
            ease_factor: 2.6,
            due_date: on(30),
        };
        apply_review(&connection, card.id, &first, at(24, 18)).expect("first review");

        // A failed answer the next day: the interval collapses and the card is
        // due again immediately.
        let second = Scheduling {
            interval_days: 0,
            ease_factor: 2.4,
            due_date: on(25),
        };
        apply_review(&connection, card.id, &second, at(25, 9)).expect("second review");

        let stored = list_cards(&connection, deck_id).expect("listing cards");
        let stored = stored.first().expect("one card");

        assert_eq!(stored.interval_days, 0);
        assert_ease(stored.ease_factor, 2.4);
        assert_eq!(stored.due_date, on(25));
        assert_eq!(stored.updated_at, at(25, 9));
    }
}
