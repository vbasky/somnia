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
`FETCH`, `GROUP BY`/`GROUP ALL`, `count()`), `CREATE`, `INSERT`, `UPDATE`
(`SET`/`MERGE`/`CONTENT`), `DELETE`, `RELATE` (+ edge content), `Batch`;
comparison/logical operators, `type::record(...)` links, generic function calls;
`DEFINE TABLE`/`DEFINE FIELD`/`REMOVE TABLE` via derive; diesel-style migrations;
literals for string/int/float/bool/`datetime`(`d'…'`)/`uuid`(`u'…'`)/object/record/`Option`.

**Not covered:** see tiers below.

---

## P0 — correctness fixes (small, do first)

- [ ] **Record-id key escaping.** `Thing::render_literal` and `RELATE` emit a bare
      `table:key` ([query.rs](crates/somnia-core/src/query.rs)), so UUID or
      special-character ids can mis-parse. Wrap non-simple keys (`⟨…⟩` or typed
      `u'…'`) or route through `type::record(...)` consistently.
- [ ] **`INSERT` `$data` binding.** `INSERT INTO t $data` relies on a `$data`
      bind the typed layer never provides; either render the record inline or
      thread the binding through the client.
- [ ] **Geometry serialization.** `Point`/`LineString`/`Polygon` derive plain
      array `Serialize`; emit GeoJSON (`{ type, coordinates }`) so SurrealDB
      stores them as `geometry`, and add query-literal support.

## P1 — highest-value features

- [ ] **Graph traversal in `SELECT`.** Path expressions (`->edge->table`,
      `<-in<-`, `.{…}`, recursive `{..}` paths). somnia can create edges via
      `RELATE` but can't query across them — the biggest "drop to raw" today.
- [ ] **`DEFINE INDEX`.** Unique constraints + plain/composite indexes, plus the
      search/vector index variants. Unlocks uniqueness, full-text, and vector
      search. Surface it through the derive (`#[index(...)]`) and migrations.
- [ ] **`UPSERT`.** First-class statement (currently approximated with
      `UPDATE … CONTENT` on a record id).
- [ ] **Richer type mapping.** Replace the string-match type mapper
      ([derive lib.rs](crates/somnia-derive/src/lib.rs)) with real type analysis
      and add `decimal`, `duration`, `bytes`, typed nested objects, arrays of
      records, and record-id key types beyond string/uuid/int (array/object ids).

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
