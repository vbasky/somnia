//! The expression tree used inside `WHERE`/`SET`/projection clauses.
//!
//! Values render to SurrealQL via the [`SurrealQL`] trait (literals) and the
//! [`DynExpr`] trait (composed expressions). The building blocks include typed
//! [`Column`] accessors, the untyped [`Ident`], the [`Raw`] escape hatch,
//! [`RecordLink`] (`type::record(...)`), [`Func`] calls, and comparison/logical
//! operators that combine with `.and()` / `.or()`.

use std::collections::BTreeMap;
use std::fmt;

use crate::types::SurrealRecord;

// ═══════════════════════════════════════════════════════════════════════════════
// DynExpr — type-erased expression (the internal transport)
// ═══════════════════════════════════════════════════════════════════════════════

/// A type-erased expression that can render itself into a SurrealQL buffer.
/// Implemented by every expression node; the common currency of the builder.
pub trait DynExpr: fmt::Debug + Send + Sync {
    /// Append this expression's SurrealQL to `buf` with all values inlined.
    fn render_dyn(&self, buf: &mut String);

    /// Render with `$param` placeholders, collecting values into `params`.
    /// Default falls back to inline rendering; [`Literal`] nodes override this
    /// to emit `$pN` placeholders and collect their value.
    fn render_dyn_params(
        &self,
        buf: &mut String,
        _params: &mut BTreeMap<String, serde_json::Value>,
    ) {
        self.render_dyn(buf);
    }
}

/// A boxed, type-erased expression. Useful for returning a composed filter from
/// a helper function (e.g. a shared tenant/owner predicate).
pub type DynExprBox = Box<dyn DynExpr>;

impl DynExpr for Box<dyn DynExpr> {
    fn render_dyn(&self, buf: &mut String) {
        (**self).render_dyn(buf);
    }
    fn render_dyn_params(
        &self,
        buf: &mut String,
        params: &mut BTreeMap<String, serde_json::Value>,
    ) {
        (**self).render_dyn_params(buf, params);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Expr — typed expression (public API)
// ═══════════════════════════════════════════════════════════════════════════════

/// A [`DynExpr`] that also reports a SurrealQL type hint. Auto-implemented for
/// every `DynExpr` (returning `"any"`); concrete nodes may override the hint.
pub trait Expr: DynExpr {
    /// A best-effort SurrealQL type name for this expression.
    fn ty_hint(&self) -> &'static str;
}

// Any DynExpr is automatically an Expr with a default ty_hint
// (used internally; concrete types override this)
impl<E: DynExpr> Expr for E {
    fn ty_hint(&self) -> &'static str {
        "any"
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SurrealQL — literal value rendering
// ═══════════════════════════════════════════════════════════════════════════════

/// A Rust type that can be rendered as a SurrealQL literal (the right-hand side of
/// comparisons, `SET` values, etc.). Implemented for the common scalar types,
/// `Option<T>`, `serde_json::Value`, the geometry types, and `Thing<T>`.
pub trait SurrealQL: fmt::Debug + Clone + Send + Sync + 'static {
    /// The SurrealQL type name (e.g. `"string"`, `"datetime"`).
    fn surreal_type() -> &'static str;
    /// Append the literal form of `value` to `buf` (with any needed quoting/escaping).
    fn render_literal(value: &Self, buf: &mut String);
    /// Convert `value` to a JSON parameter for `$param` binding. Used by
    /// [`to_surrealql_with_params`](crate::query::Select::to_surrealql_with_params).
    fn to_param_value(value: &Self) -> serde_json::Value;
}

impl SurrealQL for String {
    fn surreal_type() -> &'static str {
        "string"
    }
    fn render_literal(value: &Self, buf: &mut String) {
        let escaped = value.replace('\\', "\\\\").replace('\'', "\\'");
        buf.push('\'');
        buf.push_str(&escaped);
        buf.push('\'');
    }
    fn to_param_value(value: &Self) -> serde_json::Value {
        serde_json::Value::String(value.clone())
    }
}

impl SurrealQL for bool {
    fn surreal_type() -> &'static str {
        "bool"
    }
    fn render_literal(value: &Self, buf: &mut String) {
        buf.push_str(if *value { "true" } else { "false" });
    }
    fn to_param_value(value: &Self) -> serde_json::Value {
        serde_json::Value::Bool(*value)
    }
}

macro_rules! surreal_display {
    ($t:ty, $name:literal) => {
        impl SurrealQL for $t {
            fn surreal_type() -> &'static str {
                $name
            }
            fn render_literal(value: &Self, buf: &mut String) {
                use std::fmt::Write;
                let _ = write!(buf, "{value}");
            }
            fn to_param_value(value: &Self) -> serde_json::Value {
                serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
            }
        }
    };
}
surreal_display!(i64, "int");
surreal_display!(i32, "int");
surreal_display!(i16, "int");
surreal_display!(i8, "int");
surreal_display!(f64, "float");
surreal_display!(f32, "float");
surreal_display!(u32, "int");
surreal_display!(u64, "int");
surreal_display!(u16, "int");
surreal_display!(u8, "int");

impl SurrealQL for chrono::DateTime<chrono::Utc> {
    fn surreal_type() -> &'static str {
        "datetime"
    }
    fn render_literal(value: &Self, buf: &mut String) {
        // SurrealDB 2.0+ requires the `d` prefix on datetime literals. A bare
        // quoted string is a `string`, not a `datetime`, so `created_at > '…'`
        // would compare against the wrong type; `created_at > d'…'` is correct.
        buf.push_str("d'");
        buf.push_str(&value.to_rfc3339());
        buf.push('\'');
    }
    fn to_param_value(value: &Self) -> serde_json::Value {
        serde_json::Value::String(value.to_rfc3339())
    }
}

impl SurrealQL for uuid::Uuid {
    fn surreal_type() -> &'static str {
        "uuid"
    }
    fn render_literal(value: &Self, buf: &mut String) {
        use std::fmt::Write;
        // SurrealDB 2.0+ requires the `u` prefix on uuid literals; a bare quoted
        // string is a `string`, not a `uuid`.
        buf.push_str("u'");
        let _ = write!(buf, "{value}");
        buf.push('\'');
    }
    fn to_param_value(value: &Self) -> serde_json::Value {
        serde_json::Value::String(value.to_string())
    }
}

impl SurrealQL for serde_json::Value {
    fn surreal_type() -> &'static str {
        "object"
    }
    fn render_literal(value: &Self, buf: &mut String) {
        // JSON is a syntactic subset of SurrealQL value literals (objects,
        // arrays, numbers, bools, null, double-quoted strings all parse), so
        // the serialized form is a valid inline literal.
        use std::fmt::Write;
        let _ = write!(buf, "{value}");
    }
    fn to_param_value(value: &Self) -> serde_json::Value {
        value.clone()
    }
}

// Geometry literals render as GeoJSON objects (a valid SurrealQL object literal),
// e.g. `{"type":"Point","coordinates":[1.0,2.0]}`.
macro_rules! geometry_surrealql {
    ($t:ident, $name:literal) => {
        impl SurrealQL for crate::types::$t {
            fn surreal_type() -> &'static str {
                $name
            }
            fn render_literal(value: &Self, buf: &mut String) {
                if let Ok(s) = serde_json::to_string(value) {
                    buf.push_str(&s);
                }
            }
            fn to_param_value(value: &Self) -> serde_json::Value {
                serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
            }
        }
    };
}
geometry_surrealql!(Point, "geometry<point>");
geometry_surrealql!(LineString, "geometry<line>");
geometry_surrealql!(Polygon, "geometry<polygon>");

impl<T: SurrealQL> SurrealQL for Option<T> {
    fn surreal_type() -> &'static str {
        T::surreal_type()
    }
    fn render_literal(value: &Self, buf: &mut String) {
        match value {
            Some(v) => T::render_literal(v, buf),
            None => buf.push_str("NONE"),
        }
    }
    fn to_param_value(value: &Self) -> serde_json::Value {
        match value {
            Some(v) => T::to_param_value(v),
            None => serde_json::Value::Null,
        }
    }
}

impl<V: SurrealQL> SurrealQL for Vec<V> {
    fn surreal_type() -> &'static str {
        // The element type is lost at this level; the derive emits the precise
        // `array<…>` for `DEFINE FIELD`. This hint is informational only.
        "array"
    }
    fn render_literal(value: &Self, buf: &mut String) {
        buf.push('[');
        for (i, v) in value.iter().enumerate() {
            if i > 0 {
                buf.push_str(", ");
            }
            V::render_literal(v, buf);
        }
        buf.push(']');
    }
    fn to_param_value(value: &Self) -> serde_json::Value {
        serde_json::Value::Array(value.iter().map(|v| V::to_param_value(v)).collect())
    }
}

impl SurrealQL for std::time::Duration {
    fn surreal_type() -> &'static str {
        "duration"
    }
    fn render_literal(value: &Self, buf: &mut String) {
        use std::fmt::Write;
        // SurrealDB duration literal: a concatenation of unit-tagged components
        // (e.g. `1s500ms`). Whole seconds + sub-second nanoseconds round-trips any
        // `Duration` and parses cleanly.
        let secs = value.as_secs();
        let nanos = value.subsec_nanos();
        if secs == 0 && nanos == 0 {
            buf.push_str("0ns");
            return;
        }
        if secs > 0 {
            let _ = write!(buf, "{secs}s");
        }
        if nanos > 0 {
            let _ = write!(buf, "{nanos}ns");
        }
    }
    fn to_param_value(value: &Self) -> serde_json::Value {
        let secs = value.as_secs();
        let nanos = value.subsec_nanos();
        if secs == 0 && nanos == 0 {
            serde_json::Value::String("0ns".into())
        } else if nanos == 0 {
            serde_json::Value::String(format!("{secs}s"))
        } else {
            serde_json::Value::String(format!("{secs}s{nanos}ns"))
        }
    }
}

impl<T: crate::types::SurrealRecord> SurrealQL for crate::types::Thing<T> {
    fn surreal_type() -> &'static str {
        "record"
    }
    fn render_literal(value: &Self, buf: &mut String) {
        buf.push_str(T::table_name());
        buf.push(':');
        value.key.render_id(buf);
    }
    fn to_param_value(value: &Self) -> serde_json::Value {
        let mut s = String::new();
        value.key.render_id(&mut s);
        serde_json::Value::String(format!("{}:{}", T::table_name(), s))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Literal — wraps a SurrealQL value as an expression
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct Literal<V: SurrealQL>(pub V);

impl<V: SurrealQL> DynExpr for Literal<V> {
    fn render_dyn(&self, buf: &mut String) {
        V::render_literal(&self.0, buf);
    }
    fn render_dyn_params(
        &self,
        buf: &mut String,
        params: &mut BTreeMap<String, serde_json::Value>,
    ) {
        let name = format!("p{}", params.len());
        buf.push('$');
        buf.push_str(&name);
        params.insert(name, V::to_param_value(&self.0));
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Param — explicit named parameter (`$name` placeholder)
// ═══════════════════════════════════════════════════════════════════════════════

/// A named SurrealQL parameter `$name`. In inline mode renders as a literal;
/// in param mode (`to_surrealql_with_params`) renders as `$name` and collects
/// the value. Use when you want to reference the same value in multiple places
/// or give params meaningful names.
///
/// ```ignore
/// let title = Param::new("search", "hello");
/// Post::table()
///     .project(Post::all())
///     .filter(Post::title().eq(title.clone()).or(Post::body().contains(title)))
///     .to_surrealql_with_params();
/// // → ("SELECT * FROM post WHERE title = $search OR body CONTAINS $search",
/// //    {"search": "hello"})
/// ```
#[derive(Debug, Clone)]
pub struct Param<V: SurrealQL> {
    name: String,
    value: V,
}

impl<V: SurrealQL> Param<V> {
    /// Create a named parameter placeholder with the given name and value.
    pub fn new(name: impl Into<String>, value: V) -> Self {
        Self {
            name: name.into(),
            value,
        }
    }
}

impl<V: SurrealQL> DynExpr for Param<V> {
    fn render_dyn(&self, buf: &mut String) {
        // In inline mode, render the literal — the name is irrelevant.
        V::render_literal(&self.value, buf);
    }
    fn render_dyn_params(
        &self,
        buf: &mut String,
        params: &mut BTreeMap<String, serde_json::Value>,
    ) {
        buf.push('$');
        buf.push_str(&self.name);
        params
            .entry(self.name.clone())
            .or_insert_with(|| V::to_param_value(&self.value));
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Column — typed field reference (e.g., `asset.name`)
// ═══════════════════════════════════════════════════════════════════════════════

/// A typed reference to a record field — e.g. `Post::title()` yields a
/// `Column<Post, String>`. Generated by `#[derive(SurrealRecord)]` and used to
/// build type-checked filters (`.eq(...)`, `.gt(...)`, …) and projections.
pub struct Column<T: SurrealRecord, V: SurrealQL> {
    /// The database column name.
    pub name: &'static str,
    /// The SurrealQL type name for this column.
    pub surreal_type: &'static str,
    #[doc(hidden)]
    pub _marker: std::marker::PhantomData<(T, V)>,
}

impl<T: SurrealRecord, V: SurrealQL> std::fmt::Debug for Column<T, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Column").field("name", &self.name).finish()
    }
}

impl<T: SurrealRecord, V: SurrealQL> Clone for Column<T, V> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: SurrealRecord, V: SurrealQL> Copy for Column<T, V> {}

impl<T: SurrealRecord, V: SurrealQL> DynExpr for Column<T, V> {
    fn render_dyn(&self, buf: &mut String) {
        buf.push_str(self.name);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Ident — an untyped column/field reference usable in WHERE clauses
// ═══════════════════════════════════════════════════════════════════════════════

/// An untyped identifier (column or field path) for building filter expressions
/// where a typed [`Column`] accessor is unavailable (e.g. record-link fields the
/// derive doesn't expose, or `tenant.slug` paths). Mirrors `Column`'s operators.
#[derive(Debug, Clone, Copy)]
pub struct Ident(pub &'static str);

/// Construct an [`Ident`] for a field name.
pub fn ident(name: &'static str) -> Ident {
    Ident(name)
}

impl Ident {
    fn dyn_box(&self) -> Box<dyn DynExpr> {
        Box::new(Raw(self.0.to_string()))
    }

    pub fn eq<V: SurrealQL>(&self, v: V) -> EqExpr {
        EqExpr {
            left: self.dyn_box(),
            right: Box::new(Literal(v)),
        }
    }
    pub fn ne<V: SurrealQL>(&self, v: V) -> NeExpr {
        NeExpr {
            left: self.dyn_box(),
            right: Box::new(Literal(v)),
        }
    }
    pub fn gt<V: SurrealQL>(&self, v: V) -> GtExpr {
        GtExpr {
            left: self.dyn_box(),
            right: Box::new(Literal(v)),
        }
    }
    pub fn lt<V: SurrealQL>(&self, v: V) -> LtExpr {
        LtExpr {
            left: self.dyn_box(),
            right: Box::new(Literal(v)),
        }
    }
    pub fn gte<V: SurrealQL>(&self, v: V) -> GteExpr {
        GteExpr {
            left: self.dyn_box(),
            right: Box::new(Literal(v)),
        }
    }
    pub fn lte<V: SurrealQL>(&self, v: V) -> LteExpr {
        LteExpr {
            left: self.dyn_box(),
            right: Box::new(Literal(v)),
        }
    }
    pub fn contains<V: SurrealQL>(&self, v: V) -> ContainsExpr {
        ContainsExpr {
            haystack: self.dyn_box(),
            needle: Box::new(Literal(v)),
        }
    }
    /// `field CONTAINS <expr>` — e.g. array-membership test with a [`Param`] or
    /// other expression.
    pub fn contains_expr(&self, expr: impl DynExpr + 'static) -> ContainsExpr {
        ContainsExpr {
            haystack: self.dyn_box(),
            needle: Box::new(expr),
        }
    }
    /// Compare to an arbitrary expression — e.g. `asset = type::record('asset', …)`.
    pub fn eq_expr(&self, rhs: impl DynExpr + 'static) -> EqExpr {
        EqExpr {
            left: self.dyn_box(),
            right: Box::new(rhs),
        }
    }
    pub fn ne_expr(&self, rhs: impl DynExpr + 'static) -> NeExpr {
        NeExpr {
            left: self.dyn_box(),
            right: Box::new(rhs),
        }
    }
    /// `field IN <expr>` — e.g. an array literal or a parenthesized subquery.
    pub fn in_expr(&self, rhs: impl DynExpr + 'static) -> InExpr {
        InExpr {
            left: self.dyn_box(),
            right: Box::new(rhs),
        }
    }
    /// `field NOT IN <expr>`.
    pub fn not_in_expr(&self, rhs: impl DynExpr + 'static) -> NotInExpr {
        NotInExpr {
            left: self.dyn_box(),
            right: Box::new(rhs),
        }
    }
    /// `field IS NONE`.
    pub fn is_none(&self) -> Raw {
        Raw(format!("{} IS NONE", self.0))
    }
}

impl DynExpr for Ident {
    fn render_dyn(&self, buf: &mut String) {
        buf.push_str(self.0);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Raw — verbatim SurrealQL escape hatch (lambdas, IF/THEN/ELSE, field paths…)
// ═══════════════════════════════════════════════════════════════════════════════

/// A verbatim SurrealQL fragment. Use for expressions somnia does not model as
/// typed nodes (e.g. `IF x != NONE THEN … END`, lambdas, `tenant.slug`).
#[derive(Debug, Clone)]
pub struct Raw(pub String);

impl Raw {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl DynExpr for Raw {
    fn render_dyn(&self, buf: &mut String) {
        buf.push_str(&self.0);
    }
}

/// SurrealDB's `NONE`.
#[derive(Debug, Clone)]
pub struct NoneLit;
impl DynExpr for NoneLit {
    fn render_dyn(&self, buf: &mut String) {
        buf.push_str("NONE");
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// RecordLink — `type::record('table', <key>)`
// ═══════════════════════════════════════════════════════════════════════════════

/// Builds a SurrealDB record link `type::record('table', <key>)`. The key is any
/// literal value (typically the bare UUID/string id of the related row).
#[derive(Debug)]
pub struct RecordLink {
    table: &'static str,
    key: Box<dyn DynExpr>,
}

impl RecordLink {
    /// `type::record('table', '<key literal>')`.
    pub fn new<V: SurrealQL>(table: &'static str, key: V) -> Self {
        Self {
            table,
            key: Box::new(Literal(key)),
        }
    }
    /// `type::record('table', <key expr>)` — key rendered from an arbitrary expr.
    pub fn from_expr(table: &'static str, key: impl DynExpr + 'static) -> Self {
        Self {
            table,
            key: Box::new(key),
        }
    }
}

impl DynExpr for RecordLink {
    fn render_dyn(&self, buf: &mut String) {
        buf.push_str("type::record('");
        buf.push_str(self.table);
        buf.push_str("', ");
        self.key.render_dyn(buf);
        buf.push(')');
    }
    fn render_dyn_params(
        &self,
        buf: &mut String,
        params: &mut BTreeMap<String, serde_json::Value>,
    ) {
        buf.push_str("type::record('");
        buf.push_str(self.table);
        buf.push_str("', ");
        self.key.render_dyn_params(buf, params);
        buf.push(')');
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Path — graph traversal (`->edge->table`, `<-edge<-table`, `<->edge<->table`)
// ═══════════════════════════════════════════════════════════════════════════════

/// Direction of a single graph hop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    /// outgoing edge — `->`
    Out,
    /// incoming edge — `<-`
    In,
    /// either direction — `<->`
    Both,
}

impl Dir {
    fn arrow(self) -> &'static str {
        match self {
            Dir::Out => "->",
            Dir::In => "<-",
            Dir::Both => "<->",
        }
    }
}

/// One hop in a graph [`Path`]: a direction, an edge table, an optional
/// destination table, and an optional `WHERE` filter on the edge.
#[derive(Debug)]
struct Step {
    dir: Dir,
    edge: &'static str,
    dest: Option<&'static str>,
    filter: Option<Box<dyn DynExpr>>,
}

impl Step {
    fn render(&self, buf: &mut String) {
        buf.push_str(self.dir.arrow());
        match &self.filter {
            // `->(edge WHERE <expr>)`
            Some(f) => {
                buf.push('(');
                buf.push_str(self.edge);
                buf.push_str(" WHERE ");
                f.render_dyn(buf);
                buf.push(')');
            }
            None => buf.push_str(self.edge),
        }
        if let Some(dest) = self.dest {
            buf.push_str(self.dir.arrow());
            buf.push_str(dest);
        }
    }
    fn render_params(&self, buf: &mut String, params: &mut BTreeMap<String, serde_json::Value>) {
        buf.push_str(self.dir.arrow());
        match &self.filter {
            Some(f) => {
                buf.push('(');
                buf.push_str(self.edge);
                buf.push_str(" WHERE ");
                f.render_dyn_params(buf, params);
                buf.push(')');
            }
            None => buf.push_str(self.edge),
        }
        if let Some(dest) = self.dest {
            buf.push_str(self.dir.arrow());
            buf.push_str(dest);
        }
    }
}

/// The trailing field accessor on a [`Path`] — `.field` or `.*`.
#[derive(Debug)]
enum Tail {
    Field(String),
    All,
}

/// A graph-traversal expression — e.g. `->wrote->post`, `<-wrote<-user`, or a
/// multi-hop chain with an optional `.field`/`.*` accessor at the end.
///
/// A `Path` is a [`DynExpr`], so it works anywhere the builder takes an
/// expression: as a `SELECT` projection (`Projection::aliased(path, "posts")`),
/// inside a `WHERE` filter, or as a `SET` value. With no start it is relative to
/// the statement's `FROM` table; [`Path::from_record`] anchors it to a record.
///
/// ```ignore
/// // ->wrote->post.title AS titles
/// let p = Path::out::<Wrote>().to::<Post>().field("title");
/// Post::table().project(vec![Projection::aliased(p, "titles")]);
/// ```
#[derive(Debug)]
pub struct Path {
    start: Option<Box<dyn DynExpr>>,
    recurse: Option<String>,
    steps: Vec<Step>,
    tail: Option<Tail>,
}

impl Path {
    fn from_step(dir: Dir, edge: &'static str) -> Self {
        Self {
            start: None,
            recurse: None,
            steps: vec![Step {
                dir,
                edge,
                dest: None,
                filter: None,
            }],
            tail: None,
        }
    }

    /// Start an outgoing hop over edge `E` — `->edge`.
    pub fn out<E: crate::types::SurrealEdge>() -> Self {
        Self::from_step(Dir::Out, E::edge_name())
    }
    /// Start an incoming hop over edge `E` — `<-edge`.
    pub fn inn<E: crate::types::SurrealEdge>() -> Self {
        Self::from_step(Dir::In, E::edge_name())
    }
    /// Start a bidirectional hop over edge `E` — `<->edge`.
    pub fn both<E: crate::types::SurrealEdge>() -> Self {
        Self::from_step(Dir::Both, E::edge_name())
    }

    /// Start an outgoing hop over a raw edge name — `->edge`.
    pub fn out_edge(edge: &'static str) -> Self {
        Self::from_step(Dir::Out, edge)
    }
    /// Start an incoming hop over a raw edge name — `<-edge`.
    pub fn in_edge(edge: &'static str) -> Self {
        Self::from_step(Dir::In, edge)
    }
    /// Start a bidirectional hop over a raw edge name — `<->edge`.
    pub fn both_edge(edge: &'static str) -> Self {
        Self::from_step(Dir::Both, edge)
    }

    /// Anchor an existing path to a starting record literal — `<record><path>`
    /// (e.g. `user:tobie->wrote->post`). Pass a [`Thing`](crate::types::Thing).
    pub fn from_record<V: SurrealQL>(mut self, start: V) -> Self {
        self.start = Some(Box::new(Literal(start)));
        self
    }

    /// Anchor an existing path to a starting expression — e.g. a
    /// [`RecordLink`] (`type::record('user', $id)->wrote->post`).
    pub fn from_expr(mut self, start: impl DynExpr + 'static) -> Self {
        self.start = Some(Box::new(start));
        self
    }

    fn last_mut(&mut self) -> &mut Step {
        self.steps.last_mut().expect("path always has ≥1 step")
    }

    /// Constrain the destination of the most recent hop to table `T` — `->edge->table`.
    pub fn to<T: SurrealRecord>(mut self) -> Self {
        self.last_mut().dest = Some(T::table_name());
        self
    }
    /// Constrain the destination of the most recent hop to a raw table name.
    pub fn to_table(mut self, table: &'static str) -> Self {
        self.last_mut().dest = Some(table);
        self
    }

    /// Filter the most recent hop's edge — `->(edge WHERE <expr>)`.
    pub fn where_(mut self, expr: impl DynExpr + 'static) -> Self {
        self.last_mut().filter = Some(Box::new(expr));
        self
    }

    /// Chain another outgoing hop over edge `E`.
    pub fn then_out<E: crate::types::SurrealEdge>(self) -> Self {
        self.push_step(Dir::Out, E::edge_name())
    }
    /// Chain another incoming hop over edge `E`.
    pub fn then_in<E: crate::types::SurrealEdge>(self) -> Self {
        self.push_step(Dir::In, E::edge_name())
    }
    /// Chain another bidirectional hop over edge `E`.
    pub fn then_both<E: crate::types::SurrealEdge>(self) -> Self {
        self.push_step(Dir::Both, E::edge_name())
    }
    /// Chain another hop over a raw edge name, outgoing.
    pub fn then_out_edge(self, edge: &'static str) -> Self {
        self.push_step(Dir::Out, edge)
    }
    /// Chain another hop over a raw edge name, incoming.
    pub fn then_in_edge(self, edge: &'static str) -> Self {
        self.push_step(Dir::In, edge)
    }

    fn push_step(mut self, dir: Dir, edge: &'static str) -> Self {
        self.steps.push(Step {
            dir,
            edge,
            dest: None,
            filter: None,
        });
        self
    }

    /// Repeat the path recursively, unbounded — `@.{..}<path>`. Combine with
    /// [`from_record`](Self::from_record) to anchor the recursion at a record
    /// (`person:tobie.{..}->knows->person`).
    pub fn recurse_all(mut self) -> Self {
        self.recurse = Some("..".to_string());
        self
    }
    /// Recurse up to `max` hops — `@.{..max}<path>`.
    pub fn recurse_up_to(mut self, max: u32) -> Self {
        self.recurse = Some(format!("..{max}"));
        self
    }
    /// Recurse between `min` and `max` hops — `@.{min..max}<path>`.
    pub fn recurse_range(mut self, min: u32, max: u32) -> Self {
        self.recurse = Some(format!("{min}..{max}"));
        self
    }
    /// Recurse exactly `n` hops — `@.{n}<path>`.
    pub fn recurse_exact(mut self, n: u32) -> Self {
        self.recurse = Some(format!("{n}"));
        self
    }

    /// Append a field accessor — `<path>.field`.
    pub fn field(mut self, name: impl Into<String>) -> Self {
        self.tail = Some(Tail::Field(name.into()));
        self
    }
    /// Append the all-fields accessor — `<path>.*`.
    pub fn all(mut self) -> Self {
        self.tail = Some(Tail::All);
        self
    }

    /// `<path> CONTAINS <value>` — e.g. membership test over a traversed list.
    pub fn contains<V: SurrealQL>(self, value: V) -> ContainsExpr {
        ContainsExpr {
            haystack: Box::new(self),
            needle: Box::new(Literal(value)),
        }
    }
    /// `<path> = <expr>`.
    pub fn eq_expr(self, rhs: impl DynExpr + 'static) -> EqExpr {
        EqExpr {
            left: Box::new(self),
            right: Box::new(rhs),
        }
    }
}

impl DynExpr for Path {
    fn render_dyn(&self, buf: &mut String) {
        match (&self.start, &self.recurse) {
            // anchored recursion: `<record>.{range}<path>`
            (Some(start), Some(range)) => {
                start.render_dyn(buf);
                buf.push_str(".{");
                buf.push_str(range);
                buf.push('}');
            }
            // relative recursion: `@.{range}<path>` (the `@` is the recursion point)
            (None, Some(range)) => {
                buf.push_str("@.{");
                buf.push_str(range);
                buf.push('}');
            }
            // no recursion: optional record anchor, then the hops
            (Some(start), None) => start.render_dyn(buf),
            (None, None) => {}
        }
        for step in &self.steps {
            step.render(buf);
        }
        match &self.tail {
            Some(Tail::Field(f)) => {
                buf.push('.');
                buf.push_str(f);
            }
            Some(Tail::All) => buf.push_str(".*"),
            None => {}
        }
    }
    fn render_dyn_params(
        &self,
        buf: &mut String,
        params: &mut BTreeMap<String, serde_json::Value>,
    ) {
        match (&self.start, &self.recurse) {
            (Some(start), Some(range)) => {
                start.render_dyn_params(buf, params);
                buf.push_str(".{");
                buf.push_str(range);
                buf.push('}');
            }
            (None, Some(range)) => {
                buf.push_str("@.{");
                buf.push_str(range);
                buf.push('}');
            }
            (Some(start), None) => start.render_dyn_params(buf, params),
            (None, None) => {}
        }
        for step in &self.steps {
            step.render_params(buf, params);
        }
        match &self.tail {
            Some(Tail::Field(f)) => {
                buf.push('.');
                buf.push_str(f);
            }
            Some(Tail::All) => buf.push_str(".*"),
            None => {}
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Func — `name(arg, arg, …)` (record::id, type::string, string::lowercase, …)
// ═══════════════════════════════════════════════════════════════════════════════

/// A SurrealQL function call `name(args…)`.
#[derive(Debug)]
pub struct Func {
    name: &'static str,
    args: Vec<Box<dyn DynExpr>>,
}

impl Func {
    pub fn new(name: &'static str, args: Vec<Box<dyn DynExpr>>) -> Self {
        Self { name, args }
    }
    /// Single-argument function over a bare column/identifier, e.g.
    /// `record::id(id)` → `Func::of("record::id", "id")`.
    pub fn of(name: &'static str, ident: &'static str) -> Self {
        Self {
            name,
            args: vec![Box::new(Raw(ident.to_string()))],
        }
    }
}

impl DynExpr for Func {
    fn render_dyn(&self, buf: &mut String) {
        buf.push_str(self.name);
        buf.push('(');
        for (i, a) in self.args.iter().enumerate() {
            if i > 0 {
                buf.push_str(", ");
            }
            a.render_dyn(buf);
        }
        buf.push(')');
    }
    fn render_dyn_params(
        &self,
        buf: &mut String,
        params: &mut BTreeMap<String, serde_json::Value>,
    ) {
        buf.push_str(self.name);
        buf.push('(');
        for (i, a) in self.args.iter().enumerate() {
            if i > 0 {
                buf.push_str(", ");
            }
            a.render_dyn_params(buf, params);
        }
        buf.push(')');
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Binary expression nodes
// ═══════════════════════════════════════════════════════════════════════════════

macro_rules! binop {
    ($name:ident, $op:literal) => {
        #[derive(Debug)]
        pub struct $name {
            pub(crate) left: Box<dyn DynExpr>,
            pub(crate) right: Box<dyn DynExpr>,
        }
        impl DynExpr for $name {
            fn render_dyn(&self, buf: &mut String) {
                self.left.render_dyn(buf);
                buf.push(' ');
                buf.push_str($op);
                buf.push(' ');
                self.right.render_dyn(buf);
            }
            fn render_dyn_params(
                &self,
                buf: &mut String,
                params: &mut BTreeMap<String, serde_json::Value>,
            ) {
                self.left.render_dyn_params(buf, params);
                buf.push(' ');
                buf.push_str($op);
                buf.push(' ');
                self.right.render_dyn_params(buf, params);
            }
        }
    };
}

binop!(EqExpr, "=");
binop!(NeExpr, "!=");
binop!(GtExpr, ">");
binop!(LtExpr, "<");
binop!(GteExpr, ">=");
binop!(LteExpr, "<=");
binop!(AndExpr, "AND");
binop!(OrExpr, "OR");
binop!(InExpr, "IN");
binop!(NotInExpr, "NOT IN");

#[derive(Debug)]
pub struct NotExpr {
    pub(crate) inner: Box<dyn DynExpr>,
}

impl DynExpr for NotExpr {
    fn render_dyn(&self, buf: &mut String) {
        buf.push_str("NOT ");
        self.inner.render_dyn(buf);
    }
    fn render_dyn_params(
        &self,
        buf: &mut String,
        params: &mut BTreeMap<String, serde_json::Value>,
    ) {
        buf.push_str("NOT ");
        self.inner.render_dyn_params(buf, params);
    }
}

#[derive(Debug)]
pub struct ContainsExpr {
    pub(crate) haystack: Box<dyn DynExpr>,
    pub(crate) needle: Box<dyn DynExpr>,
}

impl DynExpr for ContainsExpr {
    fn render_dyn(&self, buf: &mut String) {
        self.haystack.render_dyn(buf);
        buf.push_str(" CONTAINS ");
        self.needle.render_dyn(buf);
    }
    fn render_dyn_params(
        &self,
        buf: &mut String,
        params: &mut BTreeMap<String, serde_json::Value>,
    ) {
        self.haystack.render_dyn_params(buf, params);
        buf.push_str(" CONTAINS ");
        self.needle.render_dyn_params(buf, params);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Column operator methods (Diesel-style: asset.name.eq("foo"))
// ═══════════════════════════════════════════════════════════════════════════════

impl<T: SurrealRecord, V: SurrealQL> Column<T, V> {
    pub fn eq(&self, value: V) -> EqExpr {
        EqExpr {
            left: self.dyn_box(),
            right: Box::new(Literal(value)),
        }
    }
    pub fn ne(&self, value: V) -> NeExpr {
        NeExpr {
            left: self.dyn_box(),
            right: Box::new(Literal(value)),
        }
    }
    pub fn gt(&self, value: V) -> GtExpr {
        GtExpr {
            left: self.dyn_box(),
            right: Box::new(Literal(value)),
        }
    }
    pub fn lt(&self, value: V) -> LtExpr {
        LtExpr {
            left: self.dyn_box(),
            right: Box::new(Literal(value)),
        }
    }
    pub fn gte(&self, value: V) -> GteExpr {
        GteExpr {
            left: self.dyn_box(),
            right: Box::new(Literal(value)),
        }
    }
    pub fn lte(&self, value: V) -> LteExpr {
        LteExpr {
            left: self.dyn_box(),
            right: Box::new(Literal(value)),
        }
    }
    pub fn contains(&self, value: V) -> ContainsExpr {
        ContainsExpr {
            haystack: self.dyn_box(),
            needle: Box::new(Literal(value)),
        }
    }
    /// `column CONTAINS <expr>` — e.g. array-membership test with an expression.
    pub fn contains_expr(&self, expr: impl DynExpr + 'static) -> ContainsExpr {
        ContainsExpr {
            haystack: self.dyn_box(),
            needle: Box::new(expr),
        }
    }

    /// Compare this column to an arbitrary expression — e.g. a [`RecordLink`]
    /// (`asset = type::record('asset', …)`) or [`NoneLit`] (`tenant = NONE`).
    pub fn eq_expr(&self, rhs: impl DynExpr + 'static) -> EqExpr {
        EqExpr {
            left: self.dyn_box(),
            right: Box::new(rhs),
        }
    }
    pub fn ne_expr(&self, rhs: impl DynExpr + 'static) -> NeExpr {
        NeExpr {
            left: self.dyn_box(),
            right: Box::new(rhs),
        }
    }
    /// `column IN <expr>` — e.g. an array literal or a parenthesized subquery.
    pub fn in_expr(&self, rhs: impl DynExpr + 'static) -> InExpr {
        InExpr {
            left: self.dyn_box(),
            right: Box::new(rhs),
        }
    }
    /// `column NOT IN <expr>`.
    pub fn not_in_expr(&self, rhs: impl DynExpr + 'static) -> NotInExpr {
        NotInExpr {
            left: self.dyn_box(),
            right: Box::new(rhs),
        }
    }
    /// `column IS NONE`.
    pub fn is_none(&self) -> Raw {
        Raw(format!("{} IS NONE", self.name))
    }

    fn dyn_box(&self) -> Box<dyn DynExpr> {
        Box::new(Self {
            name: self.name,
            surreal_type: self.surreal_type,
            _marker: self._marker,
        })
    }
}

// Combinators: (a = 1).and(b = 2).or(c = 3). Available on every expression node.
macro_rules! combinators {
    ($($t:ty),* $(,)?) => {$(
        impl $t {
            pub fn and(self, other: impl DynExpr + 'static) -> AndExpr {
                AndExpr { left: Box::new(self), right: Box::new(other) }
            }
            pub fn or(self, other: impl DynExpr + 'static) -> OrExpr {
                OrExpr { left: Box::new(self), right: Box::new(other) }
            }
        }
    )*};
}
combinators!(
    EqExpr,
    NeExpr,
    GtExpr,
    LtExpr,
    GteExpr,
    LteExpr,
    AndExpr,
    OrExpr,
    ContainsExpr,
    InExpr,
    NotInExpr,
    NotExpr,
    Raw
);

/// Wraps an expression in parentheses: `(<expr>)`. Use to force grouping/precedence.
#[derive(Debug)]
pub struct Grouped(pub Box<dyn DynExpr>);

impl Grouped {
    pub fn new(inner: impl DynExpr + 'static) -> Self {
        Self(Box::new(inner))
    }
}

impl DynExpr for Grouped {
    fn render_dyn(&self, buf: &mut String) {
        buf.push('(');
        self.0.render_dyn(buf);
        buf.push(')');
    }
    fn render_dyn_params(
        &self,
        buf: &mut String,
        params: &mut BTreeMap<String, serde_json::Value>,
    ) {
        buf.push('(');
        self.0.render_dyn_params(buf, params);
        buf.push(')');
    }
}

combinators!(Grouped, Func, Path);

// ═══════════════════════════════════════════════════════════════════════════════
// Projection — a SELECT field, optionally `<expr> AS alias`
// ═══════════════════════════════════════════════════════════════════════════════

/// A single SELECT-list entry. Either a bare expression or `<expr> AS <alias>`.
#[derive(Debug)]
pub struct Projection {
    expr: Box<dyn DynExpr>,
    alias: Option<&'static str>,
}

impl Projection {
    /// A bare field/expression with no alias.
    pub fn new(expr: impl DynExpr + 'static) -> Self {
        Self {
            expr: Box::new(expr),
            alias: None,
        }
    }
    /// `<expr> AS <alias>`.
    pub fn aliased(expr: impl DynExpr + 'static, alias: &'static str) -> Self {
        Self {
            expr: Box::new(expr),
            alias: Some(alias),
        }
    }
    pub fn render(&self, buf: &mut String) {
        self.expr.render_dyn(buf);
        if let Some(a) = self.alias {
            buf.push_str(" AS ");
            buf.push_str(a);
        }
    }
    pub fn render_params(
        &self,
        buf: &mut String,
        params: &mut BTreeMap<String, serde_json::Value>,
    ) {
        self.expr.render_dyn_params(buf, params);
        if let Some(a) = self.alias {
            buf.push_str(" AS ");
            buf.push_str(a);
        }
    }
}

/// A bare column name as a projection: `name`.
pub fn col(name: &'static str) -> Projection {
    Projection::new(Raw(name.to_string()))
}

/// `<raw> AS <alias>` — verbatim expression with an alias.
pub fn field(raw: &'static str, alias: &'static str) -> Projection {
    Projection::aliased(Raw(raw.to_string()), alias)
}

// ═══════════════════════════════════════════════════════════════════════════════
// ColumnSet — `*` selector generated by derive macro
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct ColumnMeta {
    pub name: &'static str,
    pub surreal_type: &'static str,
}

/// Select-all (`*`) column list. Generated by `#[derive(SurrealRecord)]`.
pub struct ColumnSet<T: SurrealRecord> {
    pub cols: &'static [ColumnMeta],
    pub _marker: std::marker::PhantomData<T>,
}

impl<T: SurrealRecord> std::fmt::Debug for ColumnSet<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ColumnSet")
            .field("cols", &self.cols)
            .finish()
    }
}

impl<T: SurrealRecord> DynExpr for ColumnSet<T> {
    fn render_dyn(&self, buf: &mut String) {
        buf.push('*');
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ORDER BY
// ═══════════════════════════════════════════════════════════════════════════════

/// Sort direction for `ORDER BY`.
#[derive(Debug, Clone, Copy)]
pub enum Order {
    /// Ascending (`ASC`).
    Asc,
    /// Descending (`DESC`).
    Desc,
}

impl Order {
    pub fn render_suffix(&self) -> &'static str {
        match self {
            Order::Asc => "ASC",
            Order::Desc => "DESC",
        }
    }
}

impl std::fmt::Display for Order {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.render_suffix())
    }
}
