# somnia

![somnia — type-safe SurrealDB ORM for Rust](https://raw.githubusercontent.com/vbasky/somnia/main/docs/banner.png)

[![crates.io](https://img.shields.io/crates/v/somnia.svg)](https://crates.io/crates/somnia)
[![docs.rs](https://img.shields.io/docsrs/somnia)](https://docs.rs/somnia)
[![CI](https://github.com/vbasky/somnia/actions/workflows/ci.yml/badge.svg)](https://github.com/vbasky/somnia/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.95-blue.svg)](#status)
[![license](https://img.shields.io/crates/l/somnia.svg)](#license)

**A type-safe [SurrealDB](https://surrealdb.com) ORM for Rust** — a typed query
builder, a `#[derive(SurrealRecord)]` macro, schema generation, and Diesel-style
migrations.

> *somnia* — Latin for "dreams". SurrealDB is *surreal* (dreamlike); somnia is
> where your Rust types dream in SurrealQL.

```toml
[dependencies]
somnia = "0.7"
```

---

## Why

Writing SurrealQL as hand-spliced strings is error-prone: typo'd table names,
unescaped values, record-link mistakes, and projection drift. `somnia` lets your
Rust types describe the schema once and gives you:

- **Typed query building** — `Post::table().select(...).filter(Post::title().eq("hello"))`
- **Graph traversal** — query across `RELATE` edges with typed paths
  (`Path::out::<Wrote>().to::<Post>()`), including recursive `@.{..}` paths.
- **`#[derive(SurrealRecord)]`** — typed column accessors, table metadata, and
  schema DDL generated from the struct.
- **Schema as code** — `up()` / `down()` emit `DEFINE TABLE` / `DEFINE FIELD` /
  `DEFINE INDEX` / `REMOVE TABLE` from the Rust type.
- **Diesel-style migrations** — a `Migrator` that applies `up.surql` /
  reverts `down.surql` from timestamped folders, with applied-state tracking.
- **The rest of SurrealQL, typed** — atomic transactions, `$param` binding,
  subqueries, `IF`/`FOR` control flow, and `DEFINE EVENT`/`FUNCTION`/`ANALYZER`/
  `PARAM` — so you rarely drop to `Raw(...)`.

`somnia` **inlines literals** (with proper escaping) rather than relying on bind
parameters — `to_surrealql()` returns a ready-to-run statement string, which keeps
generated queries transparent and easy to log.

## Quick start

### Define a record

```rust
use somnia::{SurrealRecord, Thing};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, SurrealRecord)]
#[table("post")]
struct Post {
    #[field(thing)]
    id: Thing<Post>,
    title: String,
    body: String,
    published_at: Option<String>,
}
```

### Build queries

```rust
use somnia::{col, field, ident, RecordLink, Returning};

// SELECT with typed columns + function-wrapped projections
let sql = Post::table()
    .project(vec![
        field("record::id(id)", "id"),
        col("title"),
        field("type::string(published_at)", "published_at"),
    ])
    .filter(Post::published_at().ne(None))
    .order_desc(ident("published_at"))
    .limit(20)
    .to_surrealql();

// CREATE … with record links
let create = Post::table()
    .create()
    .record("post-1".to_string())
    .set_lit("title", "Hello, world".to_string())
    .set_expr("author", RecordLink::new("author", "bob".to_string()))
    .set_raw("published_at", "time::now()")
    .returning(Returning::After)
    .to_surrealql();

// UPSERT — update the record if it exists, otherwise create it
let upserted = Post::table()
    .upsert()
    .record("post-1".to_string())
    .set_lit("title", "Hello again".to_string())
    .returning(Returning::After)
    .to_surrealql();

// CREATE then SELECT back with typed projections
let batch = Post::table()
    .create()
    .record("post-1".to_string())
    .set_lit("title", "Hello, world".to_string())
    .set_expr("author", RecordLink::new("author", "bob".to_string()))
    .set_raw("published_at", "time::now()")
    .returning(Returning::After)
    .then_select(
        Post::table()
            .project(vec![
                field("record::id(id)", "id"),
                col("title"),
                field("type::string(published_at)", "published_at"),
            ])
            .limit(1),
    );

// UPDATE / DELETE with RETURN variants
let del = Post::table()
    .delete()
    .filter(ident("id").eq_expr(RecordLink::new("post", "post-1".to_string())))
    .returning(Returning::Before)
    .to_surrealql();

// Graph traversal across RELATE edges (`Wrote`/`Knows` are `SurrealEdge` types)
use somnia::Path;

// SELECT ->wrote->post.title AS titles FROM author
let titles = Author::table()
    .project_path(Path::out::<Wrote>().to::<Post>().field("title"), "titles")
    .to_surrealql();

// Recursive paths: every author within 3 "knows" hops
let network = Author::table()
    .project_path(Path::out::<Knows>().to::<Author>().recurse_up_to(3), "network")
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
#[table("comment")]
struct Comment {
    #[field(thing)] id: Thing<Comment>,
    #[field(record = "post")] post: serde_json::Value,
    body: String,
    #[field(ty = "datetime", default = "time::now()")] created_at: String,
}

Comment::up();   // DEFINE TABLE … ; DEFINE FIELD … ;
Comment::down(); // REMOVE TABLE IF EXISTS comment;
```

Field attributes: `#[field(thing)]` (record id), `record = "table"`
(`record<table>`), `default = "…"`, `value = "…"`, `ty = "…"` (full type
override), `flexible`, `name = "…"`, `skip`. Table attributes:
`#[table("name")]`, `#[table("name", schemaless, permissions = "NONE")]`.

Field types are mapped from the Rust type — including typed arrays
(`Vec<T>` → `array<…>`), `Option<…>`, records, `duration`, and `decimal`.

Add indexes with a repeatable container attribute; they're emitted by `up()`
(after the fields) and exposed via `SurrealSchema::define_indexes()`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, SurrealRecord)]
#[table("member")]
#[index(name = "member_email_unique", fields = "email", unique)]
struct Member {
    #[field(thing)] id: Thing<Member>,
    email: String,
}
```

For ad-hoc or richer indexes (full-text `SEARCH`, vector `HNSW`/`MTREE`), use the
`DefineIndex` builder directly.

### Migrations

Lay out migrations Diesel-style — one timestamped folder per migration with
`up.surql` and `down.surql`:

```bash
migrations/
  2025-01-01-000000_create_posts/
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

## More query power

somnia models much of SurrealDB's surface as typed builders, so you rarely fall
back to raw strings. Every builder renders to a plain SurrealQL string via
`to_surrealql()` — shown in the `//` comments below — so nothing is hidden. The
examples build on the `Post` and `Comment` structs from [Quick start](#quick-start).

### Parameters and `LET`

By default somnia inlines values as escaped literals. For statement reuse or
binary-safe values, switch to `$param` binding with `to_surrealql_with_params()`,
which returns the SQL plus a map of bound values; execute it with
`client.query_with_params(...)`:

```rust
let (sql, params) = Post::table()
    .select(Post::all())
    .filter(Post::title().eq("hello".to_string()))
    .limit(10)
    .to_surrealql_with_params();
// sql    == "SELECT * FROM post WHERE title = $p0 LIMIT 10"
// params == { "p0": "hello" }

let rows: Vec<Post> = client.query_with_params(&sql, &params).await?;
```

Bind one value to a name and reuse it across a query with `Param`, and declare a
session variable with `LetVar`:

```rust
use somnia::{Param, LetVar, Raw};

// `$q` appears twice in the SQL but is bound once
let q = Param::new("q", "rust".to_string());
let (sql, params) = Post::table()
    .select(Post::all())
    .filter(Post::title().eq_expr(q.clone()).or(Post::body().contains_expr(q)))
    .to_surrealql_with_params();
// "SELECT * FROM post WHERE title = $q OR body CONTAINS $q"   with  { "q": "rust" }

let now = LetVar::new("now", Raw("time::now()".into())).to_surrealql();
// "LET $now = time::now()"
```

### Transactions

`Transaction` wraps statements in `BEGIN … COMMIT` so they apply **atomically** —
SurrealDB rolls the whole block back if any statement errors. `.cancel()` ends
with `CANCEL TRANSACTION` to roll back explicitly:

```rust
use somnia::Transaction;

let tx = Transaction::new()
    .push(Post::table().create().record("p1".to_string()).set_lit("title", "Hi".to_string()))
    .push("UPDATE stats SET posts += 1")
    .to_surrealql();
// BEGIN TRANSACTION;
// CREATE type::record('post', 'p1') SET title = 'Hi';
// UPDATE stats SET posts += 1;
// COMMIT TRANSACTION;

client.query::<Post>(&tx).await?; // all-or-nothing
```

### Subqueries and `IN`

A `Select` is itself an expression, so you can nest one inside a `WHERE … IN`,
use it as a scalar, or read `FROM` it. Columns and idents gain `in_expr` /
`not_in_expr`:

```rust
use somnia::{ident, col, Raw};

// posts referenced by recent comments
let recent = Comment::table()
    .project(vec![col("post")])
    .value() // SELECT VALUE post → a bare list of record ids
    .filter(Raw("created_at > time::now() - 1d".into()));

let sql = Post::table()
    .select(Post::all())
    .filter(ident("id").in_expr(recent))
    .to_surrealql();
// SELECT * FROM post
//   WHERE id IN (SELECT VALUE post FROM comment WHERE created_at > time::now() - 1d)

// …or read FROM a subquery
let sql = Post::table()
    .select(Post::all())
    .from_subquery(Post::table().select(Post::all()).filter(ident("published").eq(true)))
    .to_surrealql();
// SELECT * FROM (SELECT * FROM post WHERE published = true)
```

### `SELECT` modifiers

`VALUE` (bare values), `OMIT` (drop fields from `*`), `SPLIT` (fan a row out by an
array field), `WITH INDEX` / `WITH NOINDEX` (planner hints), `TIMEOUT`, and
`EXPLAIN`:

```rust
let bare = Post::table().project(vec![col("title")]).value().to_surrealql();
// SELECT VALUE title FROM post

let sql = Post::table()
    .select(Post::all())
    .omit("body")
    .with_index(["idx_published"])
    .filter(ident("published").eq(true))
    .timeout("5s")
    .to_surrealql();
// SELECT * OMIT body FROM post WITH INDEX idx_published WHERE published = true TIMEOUT 5s

let plan = Post::table().select(Post::all()).explain().to_surrealql();
// SELECT * FROM post EXPLAIN
```

> SurrealDB 3.1 doesn't accept `PARALLEL` as a `SELECT` clause, so somnia doesn't emit it.

### Control flow — `IF` and `FOR`

`IfExpr` is an expression you can drop into a projection, a `SET` value, a
`RETURN`, or a `WHERE`. `For` builds an iterating block:

```rust
use somnia::{IfExpr, For, Raw};

let tier = IfExpr::new(Raw("votes >= 100".into()), Raw("'hot'".into()))
    .else_if(Raw("votes >= 10".into()), Raw("'warm'".into()))
    .else_(Raw("'cold'".into()));
// IF votes >= 100 THEN 'hot' ELSE IF votes >= 10 THEN 'warm' ELSE 'cold' END
// e.g. as a projection: Post::table().project(vec![Projection::aliased(tier, "tier")])

let seed = For::new("n", Raw("[1, 2, 3]".into()))
    .push("CREATE counter SET v = $n")
    .to_surrealql();
// FOR $n IN [1, 2, 3] { CREATE counter SET v = $n; }
```

### Schema DDL beyond tables, fields, and indexes

Builders for the remaining `DEFINE` statements — events, functions, analyzers,
and params — each with a matching `::remove(...)` inverse:

```rust
use somnia::{DefineEvent, DefineFunction, DefineAnalyzer, DefineParam};

DefineEvent::new("on_publish", "post")
    .when("$event = 'UPDATE' AND $after.published = true")
    .then("{ CREATE log SET post = $after.id, at = time::now() }")
    .to_surrealql();
// DEFINE EVENT IF NOT EXISTS on_publish ON TABLE post WHEN … THEN { … }

DefineFunction::new("greet")
    .arg("name", "string")
    .returns("string")
    .body("RETURN 'hi ' + $name;")
    .to_surrealql();
// DEFINE FUNCTION IF NOT EXISTS fn::greet($name: string) -> string { RETURN 'hi ' + $name; }

DefineAnalyzer::new("ascii").tokenizers(["class"]).filters(["lowercase", "ascii"]).to_surrealql();
DefineParam::new("rate", "0.5").to_surrealql();
// DEFINE PARAM IF NOT EXISTS $rate VALUE 0.5
```

Fields gain `ASSERT` (validation), `READONLY`, and `PERMISSIONS` attributes,
which flow into the generated `DEFINE FIELD`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, SurrealRecord)]
#[table("account")]
struct Account {
    #[field(thing)] id: Thing<Account>,
    #[field(assert = "$value >= 0")] balance: i64,
    #[field(readonly)] created_by: String,
    #[field(permissions = "FOR select WHERE id = $auth.id")] secret: String,
}
// DEFINE FIELD … balance … TYPE int ASSERT $value >= 0;
// DEFINE FIELD … created_by … TYPE string READONLY;
// DEFINE FIELD … secret … TYPE string PERMISSIONS FOR select WHERE id = $auth.id;
```

### Graph edges

Define an edge record once and **derive** its `SurrealEdge` impl (the edge name
comes from `#[table(...)]`) — no hand-written `impl SurrealEdge`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, SurrealRecord, SurrealEdge)]
#[table("wrote")]
struct Wrote {
    #[field(thing)] id: Thing<Wrote>,
}
```

Then create edges with `RELATE` and query across them with typed paths — see the
`Path::out::<Wrote>()…` examples under [Build queries](#build-queries) above,
including recursive `@.{..}` traversal.

## Crates

| Crate | Description |
| ------- | ------------- |
| [`somnia`](https://github.com/vbasky/somnia/tree/main/crates/somnia) | Umbrella crate: client, migrator, re-exports. Start here. |
| [`somnia-core`](https://github.com/vbasky/somnia/tree/main/crates/somnia-core) | Query builder, expression tree, `SurrealRecord`/`SurrealSchema` traits. |
| [`somnia-derive`](https://github.com/vbasky/somnia/tree/main/crates/somnia-derive) | `#[derive(SurrealRecord)]` / `#[derive(SurrealEdge)]` proc-macros. |
| [`somnia-cli`](https://github.com/vbasky/somnia/tree/main/crates/somnia-cli) | Diesel-cli-style migration runner (the `somnia` binary). |

## CLI

A standalone migration runner, modeled on `diesel-cli`. Install it with Cargo or
Homebrew (both provide the `somnia` binary):

```bash
cargo install somnia-cli                       # from crates.io
brew tap vbasky/somnia && brew install somnia  # Homebrew (macOS / Linux)
```

Then:

```bash
somnia migration generate create_posts    # scaffold a timestamped up/down folder
somnia migration run                      # apply all pending migrations
somnia migration revert                   # revert the latest
somnia migration redo                     # revert + re-apply the latest
somnia migration list                     # show applied / pending
```

Connection settings are read from flags or environment variables (`--help` for
the full list).

## Status

`0.7.x` — early but tested against SurrealDB 3.x (query builder, derive, schema
generation, and migrator all covered by integration tests that run on an
in-memory engine). The API may evolve before `1.0`. See the
[roadmap](https://github.com/vbasky/somnia/blob/main/ROADMAP.md) for what's covered today and what's planned on the way to
`1.0`.

**MSRV:** Rust **1.95** (set by the SurrealDB 3.x dependency tree). Bumping the
minimum supported Rust version is treated as a minor-version change.

## License

Licensed under the [Apache License, Version 2.0](https://github.com/vbasky/somnia/blob/main/LICENSE).

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
licensed as above, without any additional terms or conditions.
