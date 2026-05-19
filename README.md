# statbb

`statbb` is a Rust CLI baseball simulation and statistics app. It can generate
players, schedule seasons, process games, and inspect standings, batting stats,
and game results through an interactive terminal menu.

## Features

- Generate simulated baseball players
- Schedule game seasons
- Process game rounds
- View standings and batting statistics
- Browse processed games interactively
- Step through game counts with keyboard controls
- Store data in SQLite
- Supports English and Japanese UI text

## Requirements

- Rust 2024 edition
- Cargo
- SQLite database initialized with the project schema

## Installation

```sh
cargo build
```

## Usage

```sh
cargo run -- --help
```

Available options:

```sh
cargo run -- --generate 10   # Generate players
cargo run -- --schedule 1    # Schedule one season
cargo run -- --process 5     # Process five game rounds
cargo run -- --menu          # Open interactive menu
```

## Interactive Menu

The interactive menu lets you view:

- Standings
- Game results
- Batting statistics

When viewing a game, use:

- Left arrow: previous count
- Right arrow: next count
- Esc or Ctrl+C: exit game view

## Data Storage

`statbb` uses a local SQLite database named `statbb.db`.

The database schema documentation is available in:

- [`docs/db/README.md`](docs/db/README.md)

Database migrations and seed data are stored in:

- [`migrations/`](migrations/)

## Project Structure

```text
src/
  adapters/       Terminal presenters and menu UI
  domain/         Baseball simulation and business logic
  repositories/   SQLite persistence layer
  i18n.rs         Localization setup
  main.rs         CLI entry point

docs/
  db/             Generated database documentation

migrations/       SQL schema and seed files
locales/          Fluent translation files
```

## Development

Run tests:

```sh
cargo test
```

Check the CLI:

```sh
cargo run -- --help
```

Format code:

```sh
cargo fmt
```

## Database Docs

The DB docs are generated with [`tbls`](https://github.com/k1LoW/tbls).

```sh
tbls doc
```

## License

TODO
