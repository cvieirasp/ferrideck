//! All SQLite access: connection setup, schema migrations and queries.
//! Translates rows into `models` types so no SQL leaks into the rest of the
//! application.

use crate::models::Deck;
use anyhow::{Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};
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
}
