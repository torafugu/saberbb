# Project guidance

`saberbb` is a Rust CLI baseball simulation and statistics application with
SQLite persistence and an interactive terminal UI.

## Commit and pull request language

- Write commit messages in English, including their subject and body.
- Write pull request titles, descriptions, review comments, and replies in English.

## Database path

- Use the `SABERBB_DB` environment variable to provide the database path.
- Do not pass database paths through command-line arguments such as `--db` or
  `--pitch-db`, or introduce new database-path arguments.
- Follow this convention in new or updated code, scripts, tests, documentation,
  and command examples, even where older examples still use CLI arguments.
- When a command requires a database, report a clear error if `SABERBB_DB` is
  unset or blank.

Example:

```sh
export SABERBB_DB="/path/to/saberbb.db"
# Run database-dependent commands without a database-path argument.
```

## Development

- Keep changes focused on the requested task and preserve unrelated local edits.
- Follow the existing separation between `src/domain/` (simulation logic),
  `src/repositories/` (persistence), and `src/adapters/` (terminal UI).
- Use `cargo fmt` for Rust formatting and run relevant tests with `cargo test`.
- Schema and seed files live in `migrations/`; preserve existing data when
  preparing upgrades for existing databases.
