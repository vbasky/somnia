# Changelog

All notable changes to somnia are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The section header for each release is `## [<version>] - <YYYY-MM-DD>`; the
release workflow extracts the matching section verbatim as the GitHub Release
notes, so keep this format intact.

## [Unreleased]

### Added

- **Control flow (P2)** — `IfExpr` (`IF … THEN … ELSE IF … ELSE … END`) as a
  `DynExpr` usable in any expression position, and a `For` builder
  (`FOR $item IN <array> { <body> }`). Both validated against the live engine.
- **More schema DDL (P2)** — standalone builders `DefineEvent`, `DefineFunction`
  (`fn::`-prefixed, typed args, return type, body), `DefineAnalyzer` (tokenizers +
  filters), and `DefineParam` (`$`-prefixed, raw or typed value), each with a
  `::remove(...)` inverse. New derived field attributes `#[field(assert = "…")]`,
  `#[field(readonly)]`, and `#[field(permissions = "…")]` render into the
  generated `DEFINE FIELD`. Validated against the live engine.
- **`SELECT` extras (P2)** — `VALUE` mode, `OMIT`, `SPLIT`, `WITH INDEX`/`WITH
  NOINDEX` hints, `TIMEOUT`, and `EXPLAIN`[` FULL`] modifiers on `Select`;
  subqueries (`Select<T>` now implements `DynExpr`, rendering parenthesized — use
  it in a `WHERE`, as a scalar operand, or as `FROM (<subquery>)` via
  `from_subquery`); `in_expr()` / `not_in_expr()` (`IN` / `NOT IN`) on columns and
  idents; and `RETURN <projection>` on `INSERT` (now renders the actual field list
  plus a `returning()` enum setter) and `RelateEdge` (`return_field` / `returning`).
  Subquery parameters merge into the parent's `$param` map. `PARALLEL` is omitted —
  SurrealDB 3.1 rejects it on `SELECT`.
- **Transactions (P2)** — a `Transaction` builder wraps pushed statements in
  `BEGIN TRANSACTION; … ; COMMIT TRANSACTION;` (or `CANCEL TRANSACTION` via
  `.cancel()`), giving atomic, roll-back-on-error semantics that the
  `;`-concatenated `Batch` doesn't. Verified atomic against a live engine.
- **Parameters / `LET` (P2)** — an opt-in `$param`-binding mode alongside the
  default inline rendering. `to_surrealql_with_params()` on `Select`/`Create`/
  `Update`/`Delete` (and `then_select_params()` on the mutations) returns
  `(String, BTreeMap<String, serde_json::Value>)`, auto-binding each literal as
  `$p0`, `$p1`, …. `Param::new("name", value)` declares an explicit named
  placeholder for reuse across a query. A `LetVar` builder emits
  `LET $var = <expr>`, and `SomniaClient::query_with_params()` binds and runs the
  pair. The inline `to_surrealql()` path is unchanged.

### Changed

- **README** — documented the P1 features shipped through 0.5.2: a graph-traversal
  example (`Path`, recursive paths), the `#[index(...)]` derive attribute and
  `DefineIndex` builder, and the richer field-type mapping. Docs only.

## [0.5.2] - 2026-06-20

Completes the **P1** roadmap tier. Every change is API-additive (new builders,
methods, a defaulted trait method, and new `SurrealQL` impls), so this stays a
patch release; the only behavioral shift is that collection fields now emit a
more precise `array<…>` type in generated DDL.

### Added (0.5.2)

- **Recursive graph paths** — the `Path` node gained `recurse_all()` (`@.{..}`),
  `recurse_up_to(n)` (`@.{..n}`), `recurse_range(min, max)` (`@.{min..max}`), and
  `recurse_exact(n)` (`@.{n}`), rendering SurrealDB's recursive traversal syntax
  (relative `@.{…}` or, with `from_record`, record-anchored). Validated against a
  live in-memory engine.
- **`DEFINE INDEX`** — a runtime `DefineIndex` builder (plain, composite,
  `unique()`, `search(analyzer)`, `hnsw`/`mtree` vector, `comment`,
  `concurrently`, `overwrite`, and `DefineIndex::remove`) plus a repeatable derive
  attribute `#[index(name = "…", fields = "a, b", unique)]`. Derived indexes flow
  into the new `SurrealSchema::define_indexes()` and are emitted by `up()` after
  the field definitions. A live test confirms a `UNIQUE` index rejects duplicates.
- **Richer field types** — the derive maps Rust types via real recursive
  `syn::Type` analysis instead of substring matching: typed arrays
  (`Vec<T>`/`VecDeque`/`HashSet`/slices → `array<…>`), arrays of records, nested
  `Option`, `duration`, and `decimal`. Adds `SurrealQL` literal impls for
  `Vec<T>` (renders `[…]`) and `std::time::Duration` (renders e.g. `1s500000000ns`).

### Changed (0.5.2)

- `SurrealSchema` gained a defaulted `define_indexes()` method; existing manual
  implementations are unaffected.
- The derive emits a precise `array<…>` element type for collection fields
  (previously a bare `array`). Generated DDL only; no Rust API change.

## [0.5.1] - 2026-06-20

### Changed (0.5.1)

- **README** — bumped the install snippet and status line from `0.4` to `0.5`.
  The `0.5.0` crates.io README still showed `somnia = "0.4"` (a constraint that
  won't resolve to `0.5.0`); this corrects it. Docs only; no code changes.

## [0.5.0] - 2026-06-20

### Added (0.5.0)

- **Graph traversal in `SELECT`** — a typed `Path` expression node for querying
  across edges created with `RELATE` (previously a drop to `Raw`). Build hops with
  `Path::out::<E>()` / `inn` / `both` (raw variants `out_edge` / `in_edge` /
  `both_edge`), constrain the destination with `.to::<T>()` / `.to_table(...)`,
  chain multi-hop paths with `.then_out::<E>()` / `.then_in::<E>()`, filter a hop's
  edge with `.where_(expr)` (`->(edge WHERE …)->table`), append a `.field("…")` or
  `.all()` (`.*`) accessor, and anchor to a record with `.from_record(thing)` /
  `.from_expr(...)`. A `Path` is a `DynExpr`, so it works as a `SELECT` projection
  (`Table::project_path(path, alias)`, `Select::with_path(path, alias)` for
  `SELECT *, <path> AS …`) or inside a `WHERE` filter (with `.contains(...)`,
  `.eq_expr(...)`, `.and(...)` / `.or(...)`). Recursive `{..}` paths and `.{…}`
  destructuring are not yet covered.

## [0.4.1] - 2026-06-06

### Added (0.4.1)

- **`UPSERT` support** — `Table::upsert()` builds an `UPSERT` statement (update the
  matching record, or create it if it doesn't exist) with the same builder surface
  as `update()`: `record`/`set`/`set_lit`/`set_expr`/`merge`/`content`/`filter`/
  `returning`/`then_select`.

### Changed (0.4.1)

- **README** — bumped version strings and badge references from `0.3` to `0.4`;
  added `then_select` usage example. (crates.io README was stale in 0.4.0.)

## [0.4.0] - 2026-06-06

### Added (0.4.0)

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

### Fixed (0.3.0)

- **Record-id key escaping** — `Thing` literals and `RELATE` now backtick-quote
  UUID and non-identifier string keys (e.g. `asset:` `` `0190a-…` ``), so they
  parse as a record id instead of an arithmetic expression. Integer and
  simple-identifier keys are unchanged.
- **`INSERT` renders inline** — `Insert::to_surrealql` now serializes the queued
  record(s) as object literals (`INSERT INTO t { … }` / `[ … ]`) instead of an
  unbound `$data` placeholder that never resolved.

### Changed (0.3.0)

- **(Breaking) Geometry is GeoJSON** — `Point`/`LineString`/`Polygon` now
  serialize/deserialize as GeoJSON objects (`{ "type", "coordinates" }`) instead
  of bare coordinate arrays, so SurrealDB stores them as `geometry`. They also
  gained query-literal support. Any data persisted in the old array form must be
  migrated.
- **(Breaking) `Insert::to_surrealql` now requires `T: Serialize`** (it
  serializes the record inline).

### Added (0.3.0)

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
