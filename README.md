# somnia

[![crates.io](https://img.shields.io/crates/v/somnia.svg)](https://crates.io/crates/somnia)
[![docs.rs](https://img.shields.io/docsrs/somnia)](https://docs.rs/somnia)
[![license](https://img.shields.io/crates/l/somnia.svg)](#license)

**A type-safe [SurrealDB](https://surrealdb.com) ORM for Rust** — a typed query
builder, a `#[derive(SurrealRecord)]` macro, schema generation, and Diesel-style
migrations.

> *somnia* — Latin for "dreams". SurrealDB is *surreal* (dreamlike); somnia is
> where your Rust types dream in SurrealQL.

```toml
[dependencies]
somnia = "0.1"
```

---

## Why

Writing SurrealQL as hand-spliced strings is error-prone: typo'd table names,
unescaped values, record-link mistakes, and projection drift. `somnia` lets your
Rust types describe the schema once and gives you:

- **Typed query building** — `Asset::table().select(...).filter(Asset::name().eq("x"))`
- **`#[derive(SurrealRecord)]`** — typed column accessors, table metadata, and
  schema DDL generated from the struct.
- **Schema as code** — `up()` / `down()` emit `DEFINE TABLE` / `DEFINE FIELD` /
  `REMOVE TABLE` from the Rust type.
- **Diesel-style migrations** — a `Migrator` that applies `up.surql` /
  reverts `down.surql` from timestamped folders, with applied-state tracking.

`somnia` **inlines literals** (with proper escaping) rather than relying on bind
parameters — `to_surrealql()` returns a ready-to-run statement string, which keeps
generated queries transparent and easy to log.

## Quick start

### Define a record

```rust
use somnia::{SurrealRecord, Thing};
use somnia_derive::SurrealRecord;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, SurrealRecord)]
#[table("asset")]
struct Asset {
    #[field(thing)]
    id: Thing<Asset>,
    name: String,
    content_type: Option<String>,
    file_size: Option<i64>,
}
```

### Build queries

```rust
use somnia::{col, field, ident, RecordLink, Returning};

// SELECT with typed columns + function-wrapped projections
let sql = Asset::table()
    .project(vec![
        field("record::id(id)", "id"),
        col("name"),
        field("type::string(created_at)", "created_at"),
    ])
    .filter(Asset::content_type().eq(Some("video/mp4".to_string())))
    .order_desc(ident("created_at"))
    .limit(20)
    .to_surrealql();

// CREATE … with record links
let create = Asset::table()
    .create()
    .record("xyz".to_string())                       // type::record('asset', 'xyz')
    .set_lit("name", "video.mp4".to_string())
    .set_expr("tenant", RecordLink::new("tenant", "default".to_string()))
    .set_raw("created_at", "time::now()")
    .returning(Returning::After)
    .to_surrealql();

// UPDATE / DELETE with RETURN variants
let del = Asset::table()
    .delete()
    .filter(ident("id").eq_expr(RecordLink::new("asset", "xyz".to_string())))
    .returning(Returning::Before)
    .to_surrealql();
```

For SurrealQL that isn't modeled as typed nodes (lambdas, `IF/THEN/ELSE`,
`string::*` chains), use the `Raw(...)` / `field("…raw…", "alias")` escape hatch —
the builder still owns the statement structure, table names, and record links.

### Schema as code

`#[derive(SurrealRecord)]` also implements `SurrealSchema`:

```rust
use somnia::SurrealSchema;

#[derive(Debug, Clone, Serialize, Deserialize, SurrealRecord)]
#[table("asset_version")]
struct AssetVersion {
    #[field(thing)] id: Thing<AssetVersion>,
    #[field(record = "asset")] asset: serde_json::Value,  // record<asset>
    #[field(default = "1")] version_number: i64,
    #[field(ty = "datetime", default = "time::now()")] created_at: String,
}

AssetVersion::up();   // DEFINE TABLE … ; DEFINE FIELD … ;
AssetVersion::down(); // REMOVE TABLE IF EXISTS asset_version;
```

Field attributes: `#[field(thing)]` (record id), `record = "table"`
(`record<table>`), `default = "…"`, `value = "…"`, `ty = "…"` (full type
override), `flexible`, `name = "…"`, `skip`. Table attributes:
`#[table("name")]`, `#[table("name", schemaless, permissions = "NONE")]`.

### Migrations

Lay out migrations Diesel-style — one timestamped folder per migration with
`up.surql` and `down.surql`:

```
migrations/
  2025-01-01-000000_create_asset/
    up.surql
    down.surql
  2025-01-01-000100_seed_defaults/
    up.surql
    down.surql
```

```rust
use somnia::SomniaClient;

let client = SomniaClient::connect("ws://localhost:8000", "root", "root", "ns", "db").await?;
let migrator = client.migrator("migrations");

migrator.run().await?;          // apply all pending up.surql in order
migrator.revert_last().await?;  // run the latest down.surql
for m in migrator.status().await? {
    println!("{} {}", if m.applied { "✓" } else { " " }, m.id);
}
```

Applied migrations are tracked in a `_somnia_migrations` table, so re-running only
applies what's pending.

## Crates

| Crate | Description |
|-------|-------------|
| [`somnia`](crates/somnia) | Umbrella crate: client, migrator, re-exports. Start here. |
| [`somnia-core`](crates/somnia-core) | Query builder, expression tree, `SurrealRecord`/`SurrealSchema` traits. |
| [`somnia-derive`](crates/somnia-derive) | `#[derive(SurrealRecord)]` proc-macro. |

## Status

`0.1.x` — early but tested against SurrealDB 3.x (query builder, derive, schema
generation, and migrator all covered by integration tests that run on an
in-memory engine). The API may evolve before `1.0`.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
