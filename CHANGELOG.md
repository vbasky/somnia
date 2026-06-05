# Changelog

All notable changes to somnia are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The section header for each release is `## [<version>] - <YYYY-MM-DD>`; the
release workflow extracts the matching section verbatim as the GitHub Release
notes, so keep this format intact.

## [Unreleased]

## [0.1.0] - 2026-06-05

Initial release — a type-safe SurrealDB ORM for Rust.

### Added

- **Typed query builder** — compose `SELECT`/`CREATE`/`UPDATE`/`DELETE`
  SurrealQL from Rust with compile-time-checked fields and expressions.
- **`#[derive(SurrealRecord)]`** (`somnia-derive`) — derives typed records,
  field accessors, and schema metadata for a struct.
- **Schema generation** — emit `DEFINE TABLE` / `DEFINE FIELD` statements from
  derived record types.
- **Diesel-style migrations** — versioned, ordered migrations against a
  SurrealDB instance.
- Workspace split into `somnia-core` (query builder + expression tree),
  `somnia-derive` (proc-macros), and `somnia` (the facade users depend on).
