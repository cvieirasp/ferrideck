# 0002 - Local persistence

## Status

Accepted (2026-07-24)

## Context

Ferrideck is offline-first: the desktop app must work with no network, so every card, deck and review lives in a local SQLite database that is the source of truth for the client. Sync with the cloud comes later and never becomes a requirement for studying.

The access pattern is small and predictable. A study session asks for the cards due today, ordered by due date; the editor writes one row at a time. There is no concurrency beyond a single user on a single machine, and the working set fits in memory.

As with the GUI choice, the deciding factor is the learning goal: the maintainer is learning Rust fundamentals - ownership, error handling, traits - and async Rust is a second large topic that is better learned when a real problem demands it.

## Options considered

**rusqlite** - a thin, synchronous binding over the SQLite C library. Ordinary blocking function calls, no runtime, no `async`/`await`, no `Future`. Errors are plain `Result`, which is exactly the material being learned right now.

**sqlx** - async, database-agnostic, with queries verified against a real schema at compile time. It is also the crate the Lambda backend will use against Postgres, so choosing it here would mean one crate for both sides. The cost is that every query becomes `async`, which drags in a runtime (tokio) and the whole `Future` model into the first data code written in this project.

**Plain JSON files** - rejected. Spaced repetition is a query problem: "cards due on or before today, in this deck, ordered by due date". Doing that over JSON means loading everything into memory and filtering by hand, with no indexes, no transactions and no migrations. The format would also have to be versioned by hand as the card structure evolves.

## Decision

Use **rusqlite** with the `bundled` feature for the desktop app.

Async and sqlx are deferred to M7/M8, where the Lambda backend makes them unavoidable and where the reason for them is concrete rather than anticipatory.

## Consequences

- Every query blocks the calling thread, and the immediate mode UI redraws on that same thread. Database calls made from a frame must therefore stay short. Anything slow (bulk import, a full-deck statistics pass) has to move off the UI thread and cache its result in application state, the same rule the immediate mode ADR already imposes.
- The desktop app and the Lambda backend will use different database crates and different SQL dialects. This is accepted, and partly the point: it means learning the synchronous model first and the async one later, each in the context where it fits, instead of forcing one shape onto both.
- Migrating to sqlx later stays realistic because the SQL statements are the asset, not the crate calling them. Keeping all queries inside `db/`, as the architecture requires, means a migration touches one module.
- The `bundled` feature compiles SQLite from source into the binary. First builds are slower and a C compiler is required on the development machine, in exchange for a self-contained executable that needs no `sqlite3.dll` installed on the user's system and no version drift between machines.
- Schema changes need explicit migrations from the start, since there is no ORM generating them. This is a cost, but a visible and reviewable one.
