# Architecture

Ferrideck is a single binary crate. Each top-level module owns one concern and lives in one file under `src/`, declared in `src/main.rs`.

## Modules

| Module | File | Responsibility |
|---|---|---|
| `ui` | `src/ui.rs` | Presentation only: windows, screens and widgets. Renders state and turns user input into calls to the layers below. Never runs SQL or HTTP itself. |
| `study` | `src/study.rs` | Spaced repetition scheduling (SM-2): ease factors, intervals and due dates. Pure functions, no I/O. |
| `sync` | `src/sync.rs` | Offline-first synchronization with the AWS Lambda backend: pushes local changes, applies remote ones, resolves conflicts. |
| `db` | `src/db.rs` | All SQLite access: connection setup, schema migrations and queries. Converts rows into `models` types. |
| `models` | `src/models.rs` | Plain data types shared by everything else (decks, cards, reviews) with `serde` derives. No behaviour, no I/O. |

## Dependency direction

```
ui  ->  study / sync / db  ->  models
```

`ui → study/sync/db → models`, never the reverse. Concretely:

- `models` depends on nothing inside the crate. It is the only module every other one is allowed to import.
- `study`, `sync` and `db` depend on `models` and stay unaware of each other: they are siblings, not a chain.
- `ui` is the only module allowed to orchestrate the three, and it is the top of  the graph: nothing imports `ui`.

Rust will not enforce this by itself. Privacy (`pub`) controls what is visible, not who may look, so a `use crate::db::...` inside `models` would compile happily. Keeping the direction is a review rule, not a compiler guarantee.

## Why this shape

- **`study` is pure so it is trivially testable.** No clock, no network, no file access. The current date arrives as a parameter (`today: NaiveDate`), so an SM-2 test is a plain input/output assertion with no fixtures and no mocking.
- **`db` isolates SQL.** Query strings and `rusqlite`/`sqlx` types stay behind the module boundary; the rest of the app sees only `models` types. Changing the storage engine touches one module.
- **`ui` holds no rules.** Anything the UI needs to decide is a function call into `study`, `db` or `sync`, which keeps business logic out of code that is hard to test.
- **`models` has no behaviour** so that depending on it never drags I/O along.

## Conventions

- Modules use the file-per-module layout (`src/db.rs`). When one grows submodules it becomes a `src/db/` directory with `src/db.rs` still acting as its root; `mod.rs` is not used.
- Persisted card content is Markdown stored as plain text.
- Significant decisions get an ADR in `docs/decisions/NNNN-title.md`.
