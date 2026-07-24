# CLAUDE.md - Ferrideck

Project context and conventions for Claude Code. Keep this file short; details live in `docs/` and in `.claude/skills/`.

## About this project

Ferrideck is a spaced repetition flashcard desktop app built in Rust, focused on English learning. It is also a **learning project**: the maintainer is learning Rust and AWS through it. When helping:

- **Explain the "why"**, not just the "what". Prefer teaching over just producing code.
- When multiple approaches exist, mention the trade-off briefly before choosing.
- Prefer standard-library and idiomatic solutions before adding dependencies.
- **Prose style:** use plain hyphens, never em dashes (—). Applies to everything written for this repo: docs, ADRs, code comments, commit messages, PR descriptions and issues.

## Tech stack

- **App:** Rust (stable), egui or Iced (see `docs/decisions/0001-gui-framework.md`), SQLite via `rusqlite`/`sqlx`, `rodio` for audio, `serde` for JSON.
- **Backend:** AWS Lambda in Rust (`cargo-lambda`), RDS Postgres (private subnet, never exposed), S3 with pre-signed URLs, ElevenLabs for TTS (called from Lambda only - never embed API keys in the desktop app).
- **Sync model:** offline-first; client-generated UUIDs, `updated_at` timestamps, soft deletes, last-write-wins.

## Code design

- Run `cargo fmt` and `cargo clippy -- -D warnings` before every commit; fix, don't suppress, clippy lints (use `#[allow]` only with a comment explaining why).
- Errors: use `Result` everywhere; `thiserror` for library-style modules, `anyhow` only at the application edge (`main`, UI handlers). Never `unwrap()`/`expect()` outside tests, except for truly impossible states (comment required).
- Keep functions small and single-purpose; prefer returning data over mutating through `&mut` parameters when practical.
- No `unsafe` code in this project.
- Public items get doc comments (`///`) with an example when non-obvious.
- Naming: `snake_case` items, `CamelCase` types, `SCREAMING_SNAKE_CASE` consts. English only, including comments.

## Architecture

- Module boundaries: `ui/` (presentation only - no SQL, no HTTP), `models/` (plain data types, serde derives), `db/` (all SQLite access), `study/` (SM-2 and scheduling - pure functions, no I/O), `sync/` (HTTP client to Lambda, conflict resolution).
- Dependency direction: `ui → study/sync/db → models`. Never the reverse.
- `study/` must stay pure (deterministic, no clock/network/file access injected implicitly) so it is trivially testable. Pass `today: NaiveDate` as a parameter instead of reading the system clock inside the algorithm.
- All persisted content (card fronts/backs) is Markdown stored as plain text.
- Significant decisions get an ADR in `docs/decisions/NNNN-title.md`.

## Git conventions

- **Branches:** `main` is always buildable. Work on short-lived branches: `feat/<slug>`, `fix/<slug>`, `docs/<slug>`, `chore/<slug>`. Merge via Pull Request, even solo (PRs document the journey and trigger CI).
- **Commits:** Conventional Commits - `feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `chore:`, `ci:`. Imperative mood, ≤ 72 chars subject, body explains *why* when non-trivial. One logical change per commit.
- **Issues/PRs:** every PR references its issue (`Closes #12`) and its milestone. Keep the issue checklists updated.
- Never commit: `/target`, `.env`, credentials, AWS keys, ElevenLabs keys, personal audio files. Secrets live in environment variables locally and in AWS SSM/Secrets Manager in the cloud.
- `Cargo.lock` **is committed** (this is a binary crate).

## Testing

- Unit tests live next to the code (`#[cfg(test)] mod tests`). Priority targets: `study/` (SM-2 intervals, edge cases) and `sync/` (conflict resolution).
- Integration tests in `tests/` for the db layer (use an in-memory SQLite database).
- CI (GitHub Actions) runs `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` on every PR.

## Commands

```bash
cargo run                 # run the app
cargo test                # run all tests
cargo fmt && cargo clippy -- -D warnings   # pre-commit check
cargo lambda watch        # (later) run the Lambda locally
```
