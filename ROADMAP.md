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
(`SET`/`MERGE`/`CONTENT`), `UPSERT`, `DELETE`, `RELATE` (+ edge content), `Batch`,
transactions (`BEGIN`/`COMMIT`/`CANCEL`);
`then_select()` mutate-and-reselect on `CREATE`/`UPDATE`/`DELETE`;
`$param` binding (`to_surrealql_with_params()`, `Param<V>`), `LET $var`;
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

## What's next (P2 — completeness)

P2 closes the gap between "everything you need for typical CRUD + graph reads"
and "everything SurrealDB's query surface can express." Each item below turns a
frequent drop to `Raw` into a typed node.

- [x] **Transactions.** A `Transaction` builder wraps pushed statements in
  `BEGIN TRANSACTION; … ; COMMIT TRANSACTION;` (or `CANCEL TRANSACTION` via
  `.cancel()`). Unlike the `;`-concatenated `Batch`, the block is atomic —
  SurrealDB rolls every statement back if any errors. Verified against the live
  engine. **(unreleased)** Still open as a path to typed control flow: a
  closure/block body and `RETURN`-inside-transaction surfaces.

- [x] **Parameters / `LET`.** Today somnia inlines all values as escaped
  literals — the output of `to_surrealql()` is a self-contained string ready to
  send. That's transparent but wastes wire bytes for repeated statements and
  forces re-escaping of binary blobs. Add an optional `$param`-binding mode: a
  typed `ToParams` trait that emits `{ key: value }` pairs alongside
  `to_surrealql_with_params() -> (String, BTreeMap<String, Value>)`. The
  existing inlining path stays the default; opt-in binding is additive.
  SurrealDB's `LET $var = …` also needs a builder for session-scoped variables.
  **(unreleased)**

- [ ] **`SELECT` extras.** Subqueries (a `Select<T>` used as a `DynExpr` inside
  `WHERE x IN (<subquery>)` / scalar subqueries / `FROM (<subquery>)`),
  `VALUE`-mode projections (drops field-wrapping objects, returns bare values),
  `SPLIT` (split a single row into multiple output rows by an array field),
  `OMIT` (exclude specific fields from `*`), `WITH` index hints (force/ignore a
  specific index), `PARALLEL` (parallel graph fetches), `TIMEOUT` / `EXPLAIN`
  statement modifiers, and `RETURN <projection>` on `INSERT` / `RELATE` (today
  only `RETURN NONE|BEFORE|AFTER|DIFF` is modeled).

- [ ] **More schema DDL.** `DEFINE EVENT` (table-level `WHEN … THEN …` triggers
  that fire on `CREATE`/`UPDATE`/`DELETE`), `DEFINE FUNCTION` (user-defined
  SurrealQL functions with typed argument lists), `DEFINE ANALYZER` (custom
  tokenizer + filter pipelines for full-text search), `DEFINE PARAM` (schema-
  scoped default parameters). On fields: `ASSERT` (validation expressions),
  `READONLY`, and fine-grained `PERMISSIONS FOR select|create|update|delete`.

- [ ] **Control flow.** Typed `IF … THEN … ELSE IF … ELSE … END` and
  `FOR $item IN <array>` builders that compose as `DynExpr` nodes. SurrealDB
  blocks (`{ … }`) are already partially covered by the `Block` lowering inside
  expressions, but the higher-level control-flow constructs need purpose-built
  AST nodes so you can write a `FOR` loop inside a `SET` clause or an `IF`
  inside a `RETURN` without dropping to `Raw`.

- [ ] **Edge derive.** A `#[derive(SurrealEdge)]` proc-macro (or an attribute on
  `SurrealRecord`) so that `impl SurrealEdge for MyEdge { fn edge_name() …
  }` is generated rather than hand-written. Drive-by: the derive's internal
  `_flexible` flag should plumb through to `DEFINE FIELD … FLEXIBLE` (today the
  keyword is parsed from `#[field(flexible)]` but only surfaces in the
  container-level `schemaless` / field-level type `TYPE flexible` path; the
  dedicated `FLEXIBLE` keyword on the field itself wants its own render).

- [ ] **1.0 cleanup.** Before 1.0: remove the unused `_field: &str` parameter
  from `Table::count()` (vestigial API, always ignored), audit the builder
  surfaces for consistency (some methods take `&str`, others `impl Into<String>`;
  some accept `Thing<T>`, others a string key), and stabilize the public trait
  surface after the P2 feature set settles.

## P3 — advanced / specialized

These are high-value but each touches a subsystem (client auth, streaming,
SurrealDB 3.x type system) that benefits from doing P2 first.

- [ ] **Live queries.** `LIVE SELECT` statement node + a
  `LiveQueryStream<Item = Notification<T>>` returned by `SomniaClient`. The
  notification payload should carry `action` (`CREATE`/`UPDATE`/`DELETE`) and
  the deserialized `result: T`. The client layer needs to multiplex the
  WebSocket subscription onto a `tokio::sync::broadcast` or an async channel so
  the caller can iterate notifications as a stream. `KILL <query-id>` should be
  handled by dropping the stream handle.

- [ ] **Auth.** The current `SomniaClient::connect(root, pass, ns, db)` only
  handles root-level signin + namespace/database selection. Add typed builders
  for namespace/database user `SIGNIN` / `SIGNUP`, record-level
  `authenticate()` / `invalidate()` (scope auth), and token/JWT handling — both
  for issuing tokens and for attaching them to subsequent requests. The `connect`
  path should grow a `Credentials` enum so the client can be constructed with
  root, namespace, database, scope, or token credentials.

- [ ] **Vector / full-text search helpers.** `DEFINE INDEX … HNSW` and
  `… SEARCH` are already covered by `DefineIndex`. What's missing are the
  *query* helpers: typed `SELECT … WHERE vector::similarity::cosine(field, $q)`
  or `… WHERE field @@ 'query'` builders that wrap the low-level index
  primitives into a discoverable `Table::search() → Search<T>` or
  `Table::nearest() → VectorSearch<T>` surface.

- [ ] **SurrealDB 3.x type-system features.** Futures (`future<T>`), closures /
  anonymous functions, union types, `literal` types (TypeScript-style string
  literal narrowing), and `references` (typed foreign-key-like links). These
  are primarily additive to the expression tree and the derive's type mapper;
  none affect existing P0–P2 surfaces. Tracked separately because SurrealDB
  3.x itself is evolving these features and their syntax may shift.

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

## Version targets (aspirational)

| Version | Focus | Key deliverables |
|---------|-------|-----------------|
| **0.6** | Transactions + parameters | `BEGIN`/`COMMIT`/`CANCEL` builders, `$param` binding mode, `LET` builder |
| **0.7** | SELECT completeness | Subqueries, `VALUE`/`SPLIT`/`OMIT`, `RETURN <projection>`, `WITH`, `PARALLEL`, `TIMEOUT`, `EXPLAIN` |
| **0.8** | Schema DDL + control flow | `DEFINE EVENT`/`FUNCTION`/`ANALYZER`/`PARAM`, field `ASSERT`/`READONLY`/`PERMISSIONS`, `IF`/`FOR` builders, `#[derive(SurrealEdge)]` |
| **0.9** | Auth + live queries | `SIGNIN`/`SIGNUP`/`authenticate`, JWT support, `LIVE SELECT` + notification stream, search/vector query helpers |
| **1.0** | Stabilization | Audit public surface, remove vestigial APIs, freeze trait bounds, MSRV bump policy ratifies |

These are not commitments — they're a shared understanding of the logical
order. Features ship when they're ready; the version above is just the
release that *includes* the feature, not necessarily the one that *introduces*
it.

## Non-goals

- Replacing raw SurrealQL. `Raw(...)` / `field(...)` stay first-class; somnia owns
  statement structure, table names, and record links, and you drop to raw for
  anything unmodeled.
- An ODM/active-record runtime. somnia stays a builder + thin client; persistence
  ergonomics can be layered on top, they don't replace explicit queries.
- SurrealDB 2.x compatibility. 2.x is end-of-life; all new features target 3.x.
