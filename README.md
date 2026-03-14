# pkgcheck

A fast, cross-ecosystem CLI tool that checks whether all packages required by
the current project are installed.  Written in Rust for speed and reliability.

## Features

- **Multi-ecosystem support** — detects and checks Node.js (`package.json`),
  Python (`requirements.txt` / `pyproject.toml` / `Pipfile`), Rust
  (`Cargo.toml`), Go (`go.mod`), and Ruby (`Gemfile`) projects.
- **Live status indicator** — a blinking green dot while scanning, then a
  solid green / orange / red dot summarising overall health.
- **Compact terminal output** — status lines use carriage-return rewriting and
  never exceed 25 % of the terminal height.
- **Usage analysis** — scans source files for import statements and reports
  what percentage of the project uses each package.
- **Summary table** — presents a formatted table with package name,
  description, language, version, status, and usage percentage.
- **Graceful error handling** — per-ecosystem failures are reported inline;
  the tool never panics.

## Requirements

- **Rust 1.70+** (to build)
- Optional runtime tools (only needed for the ecosystems you use):
  - `pip` — for Python package inspection
  - `go` — for Go module inspection

## Build

```sh
cargo build --release
```

The binary will be at `target/release/pkgcheck`.

## Usage

Run from any project directory:

```sh
pkgcheck
```

Or, during development:

```sh
cargo run
```

### Example output

```
  Status:  ● all packages installed
  Node.js:   12/12 installed
  Rust:      6/6 installed

╭──────────┬──────────────────────────────┬─────────┬─────────┬─────────────┬─────────╮
│ Package  │ Description                  │ Language │ Version │ Status      │ Usage % │
├──────────┼──────────────────────────────┼─────────┼─────────┼─────────────┼─────────┤
│ express  │ Fast, unopinionated, minim…  │ Node.js │ 4.18.2  │ ✓ installed │ 45.0%   │
│ serde    │ N/A                          │ Rust    │ 1.0.197 │ ✓ installed │ 80.0%   │
│ ...      │ ...                          │ ...     │ ...     │ ...         │ ...     │
╰──────────┴──────────────────────────────┴─────────┴─────────┴─────────────┴─────────╯
```

### Status indicator

| Indicator | Meaning |
|-----------|---------|
| ● (blinking green) | Scanning in progress |
| ● (solid green) | All packages installed |
| ● (orange) | Some packages missing or outdated |
| ● (red) | No packages installed |

## Architecture

```
src/
├── main.rs            Entry point and orchestration
├── detect.rs          Scan CWD for manifest files
├── display.rs         Terminal UI (blink thread, table)
├── scanner.rs         Import-usage analysis
├── types.rs           Shared data types
└── ecosystems/
    ├── mod.rs         Ecosystem trait + registry
    ├── node.rs        Node.js (npm / yarn / pnpm)
    ├── python.rs      Python (pip / pipenv / poetry)
    ├── rust_lang.rs   Rust (cargo)
    ├── go_lang.rs     Go (modules)
    └── ruby.rs        Ruby (bundler)
```

## Limitations

- **Semver range checking** is not yet implemented; any installed version is
  treated as satisfying the requirement.
- **Python import-name mapping** uses a heuristic (hyphens → underscores).
  Packages with non-obvious import names (e.g. `Pillow` → `PIL`) are not
  currently handled.
- **Package descriptions** are only available where local metadata exists
  (e.g. `node_modules`, `pip show`).  Ecosystems without local description
  sources show "N/A".

## License

MIT
