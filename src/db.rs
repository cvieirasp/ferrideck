//! All SQLite access: connection setup, schema migrations and queries.
//! Translates rows into `models` types so no SQL leaks into the rest of the
//! application.
