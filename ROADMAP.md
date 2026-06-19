# somnia roadmap

somnia today is a **typed query builder + derive + schema + migrations** over the
common slice of SurrealQL, with `Raw(...)` / `field("…", "alias")` as a
first-class escape hatch for everything it doesn't model yet. This document
tracks the path from that pragmatic 0.2.x toward a 1.0 that covers SurrealDB's
feature surface — without losing the escape-hatch philosophy.

Priorities are ordered by value × how often the gap forces a drop to raw
SurrealQL. Checkboxes track status; nothing here is a commitment to a date.

## Status snapshot

**Covered:** `SELECT` (projections, `WHERE`, `ORDER BY`, `LIMIT`, `START`,
`FETCH`, `GROUP BY`/`GROUP ALL`, `count()`), graph traversal in `SELECT`
(`->edge->table`, recursive `@.{..}` paths), `CREATE`, `INSERT`, `UPDATE`
(`SET`/`MERGE`/`CONTENT`), `UPSERT`, `DELETE`, `RELATE` (+ edge content), `Batch`;
`then_select()` mutate-and-reselect on `CREATE`/`UPDATE`/`DELETE`;
comparison/logical operators, `type::record(...)` links, generic function calls;
`DEFINE TABLE`/`DEFINE FIELD`/`DEFINE INDEX`/`REMOVE TABLE` via derive; diesel-style migrations;
literals for string/int/float/bool/`datetime`(`d'…'`)/`uuid`(`u'…'`)/`duration`/array/object/record/`Option`.

**Not covered:** see tiers below.

---

## P0 — correctness fixes (small, do first) — ✅ shipped in 0.3.0

- [x] **Record-id key escaping.** `Thing` literals and `RELATE` now backtick-quote
  UUID and non-identifier string keys so they parse as a record id instead of
  an arithmetic expression. (0.3.0)
- [x] **`INSERT` `$data` binding.** `INSERT INTO t …` now serializes the queued
  record(s) inline as object literals instead of an unbound `$data`
  placeholder. (0.3.0)
- [x] **Geometry serialization.** `Point`/`LineString`/`Polygon` now serialize as
  GeoJSON (`{ type, coordinates }`) so SurrealDB stores them as `geometry`, plus
  query-literal support. (0.3.0, breaking)

## P1 — highest-value features — ✅ complete

- [x] **Graph traversal in `SELECT`.** The typed
  [`Path`](crates/somnia-core/src/expr.rs) node renders `->edge->table`,
  `<-edge<-table`, `<->edge<->table`, multi-hop chains, `.field`/`.*` accessors,
  per-hop `WHERE` filters, record anchoring, and recursive paths
  (`@.{..}`/`{..N}`/`{M..N}`/`{N}`) — usable as a `SELECT` projection
  (`project_path`/`with_path`) or in a `WHERE`. (0.5.2)
- [x] **`DEFINE INDEX`.** Plain/composite/`UNIQUE` indexes plus full-text
  (`SEARCH`) and vector (`HNSW`/`MTREE`) variants, via the runtime
  [`DefineIndex`](crates/somnia-core/src/query.rs) builder and the derive's
  repeatable `#[index(...)]` (folded into `SurrealSchema::define_indexes()` and
  `up()`). (0.5.2)
- [x] **`UPSERT`.** First-class statement via `Table::upsert()`, sharing the
  `UPDATE` builder surface. (0.4.1)
- [x] **Richer type mapping.** The derive now maps Rust types by real recursive
  `syn::Type` analysis (no more substring matching): typed arrays
  (`Vec<T>`/sets/slices → `array<…>`), arrays of records, nested `Option`,
  `duration`, and `decimal`, with `SurrealQL` literal impls for `Vec<T>` and
  `Duration`. (0.5.2) Remaining nuance: `bytes` and array/object record-id key
  types still map structurally but lack dedicated literal support.

## P2 — completeness

- [ ] **Transactions.** Real `BEGIN`/`COMMIT`/`CANCEL` (today `Batch` only
  `;`-joins statements).
- [ ] **Parameters / `LET`.** Optional `$param` binding instead of always
  inlining literals — enables statement reuse and binary-safe values.
- [ ] **`SELECT` extras.** Subqueries, `VALUE`, `SPLIT`, `OMIT`, `WITH` index
  hints, `PARALLEL`, `TIMEOUT`, `EXPLAIN`; `RETURN <projection>`.
- [ ] **More schema DDL.** `DEFINE EVENT`, `DEFINE FUNCTION`, `DEFINE ANALYZER`,
  `DEFINE PARAM`; field-level `ASSERT`, `READONLY`, and `PERMISSIONS`.
- [ ] **Control flow.** Typed `IF/ELSE` and `FOR` (today raw-only).

## P3 — advanced / specialized

- [ ] **Live queries.** `LIVE SELECT` + a notification stream API.
- [ ] **Auth.** Namespace/database user auth, record/scope access
  (`SIGNUP`/`SIGNIN`), token/JWT auth, `authenticate`/`invalidate`; the
  client currently does root signin + ns/db selection only.
- [ ] **Vector / full-text search** query helpers (depends on `DEFINE INDEX`).
- [ ] **Futures, closures, union/`literal` types, `references`** (SurrealDB 3.x).

## Non-goals

- Replacing raw SurrealQL. `Raw(...)` / `field(...)` stay first-class; somnia owns
  statement structure, table names, and record links, and you drop to raw for
  anything unmodeled.
- An ODM/active-record runtime. somnia stays a builder + thin client; persistence
  ergonomics layer on top, they don't replace explicit queries.
