# somnia roadmap

somnia today (0.8.x) is a **typed query builder + derive + schema + migrations +
thin client** over SurrealDB — covering CRUD, graph traversal, transactions,
schema DDL, control flow, auth, live queries, and full-text/vector search — with
`Raw(...)` / `field("…", "alias")` as a first-class escape hatch for everything
it doesn't model yet. This document tracks the path from the pragmatic 0.2.x
beginnings to the current 0.8.x and on toward a 1.0 that freezes the public
surface — without losing the escape-hatch philosophy.

Priorities are ordered by value × how often the gap forces a drop to raw
SurrealQL. Checkboxes track status; nothing here is a commitment to a date.

## Status snapshot

**Covered:** `SELECT` (projections, `WHERE`, `ORDER BY`, `LIMIT`, `START`,
`FETCH`, `GROUP BY`/`GROUP ALL`, `count()`), graph traversal in `SELECT`
(`->edge->table`, recursive `@.{..}` paths), `CREATE`, `INSERT`, `UPDATE`
(`SET`/`MERGE`/`CONTENT`), `UPSERT`, `DELETE`, `RELATE` (+ edge content), `Batch`,
transactions (`BEGIN`/`COMMIT`/`CANCEL`);
`then_select()` mutate-and-reselect on `CREATE`/`UPDATE`/`DELETE`;
`$param` binding (`to_surrealql_with_params()`, `Param<V>`), `LET $var`;
comparison/logical operators, `type::record(...)` links, generic function calls;
`DEFINE TABLE`/`DEFINE FIELD`/`DEFINE INDEX`/`REMOVE TABLE` via derive; diesel-style migrations;
literals for string/int/float/bool/`datetime`(`d'…'`)/`uuid`(`u'…'`)/`duration`/array/object/record/`Option`;
typed auth (root/namespace/database/record `SIGNIN`/`SIGNUP`/`authenticate`/`invalidate`, `Credentials`);
live queries (`LIVE SELECT` → `LiveQueryStream<Notification<T>>`);
full-text + vector search helpers (`Table::search`/`nearest`); closures and record `REFERENCE`s.

**Status:** all P0–P3 tiers below have shipped (through 0.8.0); the remaining
work is the 1.0 stabilization pass.

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

## P2 — completeness — feature items ✅ (1.0 cleanup pending)

P2 closes the gap between "everything you need for typical CRUD + graph reads"
and "everything SurrealDB's query surface can express." Each item below turns a
frequent drop to `Raw` into a typed node.

- [x] **Transactions.** A `Transaction` builder wraps pushed statements in
  `BEGIN TRANSACTION; … ; COMMIT TRANSACTION;` (or `CANCEL TRANSACTION` via
  `.cancel()`). Unlike the `;`-concatenated `Batch`, the block is atomic —
  SurrealDB rolls every statement back if any errors. Verified against the live
  engine. **(0.6.0)** Still open as a path to typed control flow: a
  closure/block body and `RETURN`-inside-transaction surfaces.

- [x] **Parameters / `LET`.** Today somnia inlines all values as escaped
  literals — the output of `to_surrealql()` is a self-contained string ready to
  send. That's transparent but wastes wire bytes for repeated statements and
  forces re-escaping of binary blobs. Add an optional `$param`-binding mode: a
  typed `ToParams` trait that emits `{ key: value }` pairs alongside
  `to_surrealql_with_params() -> (String, BTreeMap<String, Value>)`. The
  existing inlining path stays the default; opt-in binding is additive.
  SurrealDB's `LET $var = …` also needs a builder for session-scoped variables.
  **(0.6.0)**

- [x] **`SELECT` extras.** Subqueries (`Select<T>` is a `DynExpr`, rendered
  parenthesized — usable in `WHERE x IN (<subquery>)`, as a scalar operand, and as
  `FROM (<subquery>)` via `from_subquery`), `IN`/`NOT IN` operators,
  `VALUE`-mode projections, `SPLIT`, `OMIT`, `WITH INDEX`/`WITH NOINDEX` hints,
  `TIMEOUT` / `EXPLAIN`[` FULL`] modifiers, and `RETURN <projection>` on `INSERT`
  / `RELATE`. **(0.6.0)** `PARALLEL` is intentionally omitted — SurrealDB 3.1
  rejects it as a `SELECT` clause (parse error).

- [x] **More schema DDL.** Standalone builders `DefineEvent`, `DefineFunction`,
  `DefineAnalyzer`, `DefineParam` (each with a matching `::remove`), and new
  field attributes `#[field(assert = "…")]`, `#[field(readonly)]`,
  `#[field(permissions = "…")]` that render into the derived `DEFINE FIELD`.
  Validated against the live engine (ASSERT rejects invalid writes, the function
  is callable, the param resolves). **(0.6.0)**

- [x] **Control flow.** `IfExpr` is a `DynExpr` rendering
  `IF … THEN … ELSE IF … ELSE … END` (usable in `SET`/projection/`RETURN`/`WHERE`),
  and `For` builds `FOR $item IN <array> { <body> }`. Both validated against the
  live engine. **(0.6.0)**

- [x] **Edge derive.** `#[derive(SurrealEdge)]` generates the `impl SurrealEdge`
  (edge name from `#[table(...)]`), so it no longer needs to be hand-written —
  derive it alongside `SurrealRecord`. The `#[field(flexible)]` drive-by is
  covered: the derive renders `DEFINE FIELD … FLEXIBLE TYPE …` (regression test
  added). **(0.6.0)**

- [x] **1.0 cleanup.** Removed the unused `_field` parameter from `Table::count()`
  (now `count()`), and widened the builder string args to `impl Into<String>` so
  the older `Select`/`Create`/`Update` methods match the newer builders (record
  targets already accept any `SurrealQL` key incl. `Thing<T>`). **(0.7.0,
  breaking)** Deeper public-trait-surface stabilization continues toward 1.0.

## P3 — advanced / specialized — ✅ shipped in 0.8.0

These are high-value but each touches a subsystem (client auth, streaming,
SurrealDB 3.x type system) that benefited from doing P2 first.

- [x] **Live queries.** `SomniaClient::live_select::<T>()` returns a
  `LiveQueryStream<T>` — a `futures::Stream` of `Notification<T>` carrying the
  `action` (`Create`/`Update`/`Delete`), `query_id`, and the deserialized record
  (decoded through `serde_json::Value` so `Thing<T>` resolves). It wraps
  surrealdb's `select().live()` subscription; dropping the stream handle issues
  `KILL`. Verified live on `mem://` (create/update/delete notifications).
  **(0.8.0)** Still open: filtered/record-scoped live selects and a
  standalone `LIVE SELECT` SurrealQL node (the client path builds it internally).

- [x] **Auth.** A `Credentials` enum (`Root`/`Namespace`/`Database`/`Token`)
  drives `SomniaClient::connect_with(endpoint, ns, db, creds)` alongside the
  original root-only `connect`; `connect_anonymous` connects without signin (for
  embedded engines / deferred auth). On a live connection: `signin(&creds)`,
  record/scope `signin_record` / `signup_record` (params are any `Serialize`
  value), `authenticate(token)` to attach a pre-issued JWT, and `invalidate()`.
  Signin/signup return the issued access token; failures surface as
  `SomniaError::Auth`. Verified against the live engine (record signup → signin →
  invalidate, wrong-password rejection). **(0.8.0)** Still open: namespace/
  database user `SIGNUP` (SurrealDB only allows `SIGNUP` on record access), and
  refresh-token rotation.

- [x] **Vector / full-text search helpers.** `Table::search(field, query) →
  Search<T>` (full-text `@@`, with `search::score` projection + relevance order)
  and `Table::nearest(field, vec) → VectorSearch<T>` (KNN `<|k,ef|>` HNSW or
  `<|k,METRIC|>` brute force, with `vector::distance::knn()` projection +
  nearest-first order), backed by composable `MatchesExpr`/`KnnExpr` nodes and
  `Column::matches`. The index side was also corrected: `DefineIndex::search()`
  now emits `FULLTEXT ANALYZER …` (3.x renamed `SEARCH`). Verified live (ranked
  full-text, HNSW ordering). **(0.8.0)**

- [x] **SurrealDB 3.x type-system features.** Closures (the typed `Closure`
  node: `|$x: int| -> int $x * 2`), `references` (the derive's
  `#[field(reference[ = "cascade"])]` emitting `REFERENCE [ON DELETE …]`), and
  union / literal field types (via `#[field(ty = "int | string")]` /
  `#[field(ty = "'a' | 'b'")]`) are covered and verified live. `future<T>` is
  intentionally omitted — the keyword was removed in SurrealDB 3.x (use a
  computed `#[field(value = "…")]`). **(0.8.0)** Anonymous-function /
  union / literal *types* in the type mapper remain expressible through the `ty`
  override rather than dedicated syntax, by design.

## Design principles (for contributors)

1. **Escape hatch first.** Every feature starts with `Raw` as a baseline. The
   typed node is added when the pattern repeats and the drop-to-raw cost
   (typos, missing escaping, schema-drift) justifies the abstraction. If there
   isn't a clear typed API that's better than `Raw`, we don't add one.

2. **Inline by default, bind when asked.** `to_surrealql()` always returns a
   ready-to-run string with literals inlined. Binding (`$param`) is opt-in and
   additive — it never replaces the inlined path. This keeps the builder
   transparent, easy to log, and safe against injection (all values are
   properly escaped).

3. **Statements own their structure.** The builder renders complete SurrealQL
   statements; it does not produce fragments for manual splicing. Table names
   and record links are always rendered through typed paths (never raw strings
   from user input).

4. **No ORM lifecycle.** somnia is a query builder + thin client. It does not
   track dirty state, manage sessions, or provide `save()` / `load()` methods.
   That layer can be built on top; it stays out of scope for the core crates.

5. **SurrealDB 3.x as the target.** SurrealDB 2.x is end-of-life (no security
   patches). All new surface targets 3.x syntax and semantics. The in-memory
   engine used in tests pins `surrealdb = "3"`.

## Version history & targets

Versions **0.3–0.8 have shipped**; **1.0** is the remaining target.

| Version | Focus | Key deliverables |
|---------|-------|-----------------|
| **0.3–0.5** | P0/P1 — correctness + high-value | record-id escaping, geometry, graph traversal in `SELECT`, `DEFINE INDEX`, `UPSERT`, richer type mapping |
| **0.6** | P2 — transactions, params, SELECT extras, schema DDL, control flow | `BEGIN`/`COMMIT`/`CANCEL`, `$param` + `LET`, subqueries/`VALUE`/`SPLIT`/`OMIT`/`WITH`/`TIMEOUT`/`EXPLAIN`, `DEFINE EVENT`/`FUNCTION`/`ANALYZER`/`PARAM`, field `ASSERT`/`READONLY`/`PERMISSIONS`, `IF`/`FOR`, `#[derive(SurrealEdge)]` |
| **0.7** | Pre-1.0 cleanup (breaking) | `count()` arg dropped, builder string args unified, release-pipeline hardening |
| **0.8** | P3 — auth, live queries, search, 3.x type-system | `Credentials` + `connect_with`, record `SIGNIN`/`SIGNUP`/`authenticate`/`invalidate`, `LIVE SELECT` stream, `Table::search`/`nearest`, `Closure`, `#[field(reference)]` |
| **1.0** | Stabilization | Audit public surface, remove vestigial APIs, freeze trait bounds, ratify MSRV bump policy |

The shipped rows record what each release *included*; features ship when they're
ready, so a feature's release isn't necessarily the one that first *introduced*
it.

## Non-goals

- Replacing raw SurrealQL. `Raw(...)` / `field(...)` stay first-class; somnia owns
  statement structure, table names, and record links, and you drop to raw for
  anything unmodeled.
- An ODM/active-record runtime. somnia stays a builder + thin client; persistence
  ergonomics can be layered on top, they don't replace explicit queries.
- SurrealDB 2.x compatibility. 2.x is end-of-life; all new features target 3.x.
