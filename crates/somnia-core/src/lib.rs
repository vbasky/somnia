//! # somnia-core — typed SurrealQL query builder
//!
//! The core of the [somnia](https://docs.rs/somnia) SurrealDB ORM: a typed query
//! builder, an expression tree, and the record/schema traits. Most users depend on
//! the `somnia` umbrella crate (which re-exports everything here) rather than on
//! `somnia-core` directly.
//!
//! The builder renders ready-to-run SurrealQL strings via `to_surrealql()`, inlining
//! literals with proper escaping rather than relying on bind parameters.
//!
//! ## Modules
//!
//! - [`types`] — [`Key`], [`Thing`], the geometry types, and the
//!   [`SurrealRecord`] / [`SurrealEdge`] / [`SurrealSchema`] traits.
//! - [`expr`] — the expression tree: literals, [`Column`], [`Ident`], [`Raw`],
//!   [`RecordLink`], operators, and the [`SurrealQL`] literal-rendering trait.
//! - [`query`] — statement builders: [`Table`], [`Select`], [`Create`], [`Update`],
//!   [`Delete`], [`Insert`], [`Relate`], and [`Batch`].
//! - [`error`] — the crate error type.
//!
//! ## Example
//!
//! ```ignore
//! // `Post` derives `SurrealRecord`, which generates `table()`, `all()`,
//! // and typed column accessors like `Post::title()`.
//! let sql = Post::table()
//!     .select(Post::all())
//!     .filter(Post::title().eq("hello".to_string()))
//!     .limit(10)
//!     .to_surrealql();
//! ```

pub mod error;
pub mod expr;
pub mod query;
pub mod types;

pub use expr::{
    col, field, ident, Column, ColumnMeta, ColumnSet, DynExpr, DynExprBox, Expr, Func, Grouped,
    Ident, NoneLit, Order, Param, Path, Projection, Raw, RecordLink, SurrealQL,
};
pub use query::{
    Batch, Create, DefineIndex, Delete, Insert, LetVar, Relate, RelateEdge, Returning, Select,
    Table, Transaction, Update,
};
pub use types::{Key, SurrealEdge, SurrealRecord, SurrealSchema, Thing};
