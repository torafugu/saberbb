# saberbb

`saberbb` is a Rust CLI baseball simulation and statistics app. It generates
players, schedules seasons, simulates games, stores results in SQLite, and lets
you inspect standings and game results in a terminal UI.

## Features

- Generate simulated baseball players with batting, pitching, fielding, and
  running attributes
- Schedule seasons and process game rounds
- Persist players, schedules, games, counts, and player game stats in SQLite
- View standings and processed game results in an interactive TUI
- Step through individual game counts with keyboard controls
- Support English and Japanese UI text through Fluent locale files

## Requirements

- Rust toolchain with Cargo
- A SQLite database initialized with the project schema and seed data

The project uses Rust edition 2024. If your installed Rust version is old,
update it with `rustup update`.

## Build

```sh
cargo build
```

Run tests:

```sh
cargo test
```

Format code:

```sh
cargo fmt
```

## Usage

Show CLI help:

```sh
cargo run -- --help
```

Available options:

```sh
cargo run -- --generate 10     # Generate 10 players
cargo run -- --schedule 1      # Schedule 1 season
cargo run -- --process 5       # Process 5 game rounds
cargo run -- --maintenance     # Run SQLite VACUUM and PRAGMA optimize
cargo run -- --view            # Open the interactive terminal UI
```

Short flags are also available:

```sh
cargo run -- -g 10
cargo run -- -s 1
cargo run -- -p 5
cargo run -- -m
cargo run -- -v
```

Typical flow:

```sh
cargo run -- --generate 100
cargo run -- --schedule 1
cargo run -- --process 10
cargo run -- --view
```

## Interactive TUI

Open the TUI with:

```sh
cargo run -- --view
```

The menu includes standings and game results. Default controls are:

| Key | Action |
| --- | --- |
| Up / Down | Move selection |
| Enter | Confirm selection |
| Left / Right | Previous / next count in game detail |
| Esc / Backspace | Back |
| q / Ctrl+C / Ctrl+D | Quit |
| Ctrl+Z | Suspend |

## Configuration

Configuration is loaded with `confy` under the app name `saberbb`. On first run,
missing settings use these defaults:

```toml
version = 1
language = "en-US"
tick_rate = 1.0
frame_rate = 1.0
```

The default database path is `saberbb.db` inside the platform-specific config
directory for `jp.cosmi.saberbb`. You can override it in the generated config
file with:

```toml
database_path = "/absolute/path/to/saberbb.db"
```

Set `language` to `en-US` or `ja-JP` to switch UI text.

Logs are written as daily JSON log files under the platform-specific data
directory for `jp.cosmi.saberbb`.

## Database

The SQLite schema and seed SQL files live in:

- [`migrations/`](migrations/)

Database documentation is generated in:

- [`docs/db/README.md`](docs/db/README.md)

The generated docs include the table list and ER diagram.

## Database Docs

The DB docs are generated with [`tbls`](https://github.com/k1LoW/tbls):

```sh
tbls doc
```

## Project Structure

```text
src/
  adapters/       Terminal presenters and TUI components
  domain/         Baseball simulation and business logic
  repositories/   SQLite persistence layer
  config.rs       App configuration defaults and loading
  i18n.rs         Localization setup
  main.rs         CLI entry point

docs/
  db/             Generated database documentation
  wiki/           Domain notes

migrations/       SQL schema and seed files
locales/          Fluent translation files
tests/            Integration and simulation tests
```

## License

TODO
