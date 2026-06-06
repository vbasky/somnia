# Changelog

All notable changes to somnia are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The section header for each release is `## [<version>] - <YYYY-MM-DD>`; the
release workflow extracts the matching section verbatim as the GitHub Release
notes, so keep this format intact.

## [Unreleased]

## [0.4.0] - 2026-06-06

### Added

- **`then_select` on `CREATE`/`UPDATE`/`DELETE`** — replaces the manual
  `Batch::new().push(mut).push(select).to_surrealql()` pattern with a single
  method chain: `create.then_select(select)`. Joins any mutation statement with
  a follow-up `SELECT` as a `;`-separated batch. Available on `Create<T>`,
  `Update<T>`, and `Delete<T>`.

## [0.3.1] - 2026-06-06

### Changed (0.3.1)

- **crates.io README** — the `somnia` crate's own `README.md` (what crates.io
  renders) now mirrors the repo README: banner, current `somnia = "0.3"` usage,
  and absolute URLs for the banner/links so they resolve on crates.io. Docs only;
  no code changes.

## [0.3.0] - 2026-06-06

The P0 correctness fixes from the [roadmap](ROADMAP.md). Two are **breaking** at
the serialization/type-bound level, hence the minor bump.

### Fixed

- **Record-id key escaping** — `Thing` literals and `RELATE` now backtick-quote
  UUID and non-identifier string keys (e.g. `asset:` `` `0190a-…` ``), so they
  parse as a record id instead of an arithmetic expression. Integer and
  simple-identifier keys are unchanged.
- **`INSERT` renders inline** — `Insert::to_surrealql` now serializes the queued
  record(s) as object literals (`INSERT INTO t { … }` / `[ … ]`) instead of an
  unbound `$data` placeholder that never resolved.

### Changed

- **(Breaking) Geometry is GeoJSON** — `Point`/`LineString`/`Polygon` now
  serialize/deserialize as GeoJSON objects (`{ "type", "coordinates" }`) instead
  of bare coordinate arrays, so SurrealDB stores them as `geometry`. They also
  gained query-literal support. Any data persisted in the old array form must be
  migrated.
- **(Breaking) `Insert::to_surrealql` now requires `T: Serialize`** (it
  serializes the record inline).

### Added

- **Homebrew tap** — releases now publish a formula to `vbasky/homebrew-somnia`
  (`brew tap vbasky/somnia && brew install somnia`).

## [0.2.2] - 2026-06-06

### Changed (0.2.2)

- **License is now Apache-2.0 only** (was the dual `MIT OR Apache-2.0`).
  Consolidated to a single `LICENSE` file so GitHub detects the license, and
  updated the `license` field and all READMEs accordingly.

## [0.2.1] - 2026-06-06

### Added (0.2.1)

- `SurrealRecord` derive macro is now re-exported from the `somnia` umbrella
  crate (`somnia::SurrealRecord`). Users no longer need `somnia-derive` as an
  explicit dependency — a single `somnia = "0.2"` in Cargo.toml is sufficient.

### Changed (0.2.1)

- README examples now use domain-agnostic `Post`/`Comment` models instead of
  media-specific `Asset`/`AssetVersion`.

## [0.2.0] - 2026-06-06

### Fixed (0.2.0)

- **SurrealDB datetime/uuid literal compatibility** — datetime values now render
  as `d'…'` and UUIDs as `u'…'`, matching SurrealDB 2.0+ syntax. Previously they
  rendered as bare quoted strings, so a filter like `created_at > <datetime>`
  compared a `datetime` field against a `string` and silently mismatched.
- **`somnia-cli` exit codes** — running `somnia` with no subcommand (or
  `somnia migration` with no subcommand) now prints help and exits **0** instead
  of clap's default exit code 2. Showing help/usage is not an error; genuine
  parse errors (unknown subcommand, missing required argument) still exit 2.

### Changed (0.2.0)

- **(Breaking) `Key` conversions are now idiomatic** — the key-inference logic
  moved into `From<&str>`/`From<String>` and a new `FromStr` impl (so
  `"abc".parse::<Key>()` works). The inherent `Key::from_str` method (and its
  `#[allow(clippy::should_implement_trait)]`) was removed; use `Key::from(...)`
  or `.parse()` instead. Inference behaviour is unchanged.

## [0.1.1] - 2026-06-05

Release-engineering and metadata patch — no library API changes.

### Changed (0.1.1)

- Corrected the declared minimum supported Rust version to **1.95** (was an
  optimistic 1.75). The SurrealDB 3.x dependency tree requires it: `roaring`
  declares 1.90 (the highest *declared* MSRV), but `diskann` does not actually
  compile below 1.95 (it hits rust-lang/rust#100013 on older toolchains), so
  1.95 is the true floor.
- Fixed the `repository`/`homepage` URLs to `https://github.com/vbasky/somnia`.

### Fixed (0.1.1)

- Green CI: applied `rustfmt`, and configured `cargo-deny` for the dependency
  tree — scoped BUSL-1.1 exceptions for the SurrealDB crates and a documented
  ignore for RUSTSEC-2023-0071 (transitive `rsa`, no upstream fix).

## [0.1.0] - 2026-06-05

Initial release — a type-safe SurrealDB ORM for Rust.

### Added (0.1.0)

- **Typed query builder** — compose `SELECT`/`CREATE`/`UPDATE`/`DELETE`
  SurrealQL from Rust with compile-time-checked fields and expressions.
- **`#[derive(SurrealRecord)]`** (`somnia-derive`) — derives typed records,
  field accessors, and schema metadata for a struct.
- **Schema generation** — emit `DEFINE TABLE` / `DEFINE FIELD` statements from
  derived record types.
- **Diesel-style migrations** — versioned, ordered migrations against a
  SurrealDB instance.
- **`somnia-cli`** — a diesel-cli-style migration runner binary (`somnia`) with
  `migration generate` / `run` / `revert` / `redo` / `list`.
- Workspace split into `somnia-core` (query builder + expression tree),
  `somnia-derive` (proc-macros), `somnia` (the facade users depend on), and
  `somnia-cli` (the migration CLI).
