# Ferrideck

A spaced repetition flashcard desktop app built in Rust, focused on language learning.

Ferrideck helps you memorize English vocabulary and sentences using the SM-2 spaced repetition algorithm, the same family of algorithms behind Anki. Cards support Markdown formatting (bold/italic for tricky words) and audio pronunciation.

## Why this project exists

Ferrideck is a deliberate learning journey, built to study three things at once:

- **Rust**: ownership, modules, async, desktop UI
- **AWS**: serverless architecture with Lambda, RDS Postgres, and S3
- **English**: the app itself is my daily study tool

Because of that, the codebase favors clarity over cleverness, decisions are documented in [`docs/decisions/`](docs/decisions/), and the commit history tells the story of what was learned.

## Planned tech stack

| Layer | Technology |
|---|---|
| Desktop app | Rust + egui/Iced (see ADR 0001) |
| Local storage | SQLite (offline-first) |
| Card content | Markdown (bold/italic rendering) |
| Audio playback | rodio |
| Backend API | AWS Lambda (Rust, cargo-lambda) |
| Cloud database | AWS RDS Postgres (private, accessed only by Lambda) |
| Audio storage | AWS S3 (pre-signed URLs) |
| Text-to-speech | ElevenLabs (called from Lambda) |

## Roadmap

Development is organized in 12 milestones, from local-only MVP to full cloud sync.
See the [milestones page](../../milestones) for progress.

## Getting started

```bash
git clone https://github.com/cvieirasp/ferrideck.git
cd ferrideck
cargo run
```

Requirements: Rust stable (install via [rustup](https://rustup.rs)).

## Development

```bash
cargo fmt && cargo clippy -- -D warnings   # before every commit
cargo test                                  # run tests
```

Conventions (Conventional Commits, branch naming, architecture rules) are documented in [`CLAUDE.md`](CLAUDE.md).

## License

MIT - see [LICENSE](LICENSE).