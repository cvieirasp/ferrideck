//! All SQLite access: connection setup, schema migrations and queries.
//! Translates rows into `models` types so no SQL leaks into the rest of the
//! application.

use anyhow::{Context, Result};
use directories::ProjectDirs;
use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
