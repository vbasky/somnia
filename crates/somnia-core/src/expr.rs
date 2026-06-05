use std::fmt;

use crate::types::SurrealRecord;

// ═══════════════════════════════════════════════════════════════════════════════
// DynExpr — type-erased expression (the internal transport)
// ═══════════════════════════════════════════════════════════════════════════════

pub trait DynExpr: fmt::Debug + Send + Sync {
    fn render_dyn(&self, buf: &mut String);
}

/// A boxed, type-erased expression. Useful for returning a composed filter from
/// a helper function (e.g. a shared tenant/owner predicate).
pub type DynExprBox = Box<dyn DynExpr>;

impl DynExpr for Box<dyn DynExpr> {
    fn render_dyn(&self, buf: &mut String) {
        (**self).render_dyn(buf);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Expr — typed expression (public API)
// ═══════════════════════════════════════════════════════════════════════════════

pub trait Expr: DynExpr {
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

pub trait SurrealQL: fmt::Debug + Clone + Send + Sync + 'static {
    fn surreal_type() -> &'static str;
    fn render_literal(value: &Self, buf: &mut String);
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
}

impl SurrealQL for bool {
    fn surreal_type() -> &'static str {
        "bool"
    }
    fn render_literal(value: &Self, buf: &mut String) {
        buf.push_str(if *value { "true" } else { "false" });
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
}

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
}

impl<T: crate::types::SurrealRecord> SurrealQL for crate::types::Thing<T> {
    fn surreal_type() -> &'static str {
        "record"
    }
    fn render_literal(value: &Self, buf: &mut String) {
        use std::fmt::Write;
        let _ = write!(buf, "{}:{}", T::table_name(), value.key);
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
}

// ═══════════════════════════════════════════════════════════════════════════════
// Column — typed field reference (e.g., `asset.name`)
// ═══════════════════════════════════════════════════════════════════════════════

pub struct Column<T: SurrealRecord, V: SurrealQL> {
    pub name: &'static str,
    pub surreal_type: &'static str,
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

#[derive(Debug)]
pub struct NotExpr {
    pub(crate) inner: Box<dyn DynExpr>,
}

impl DynExpr for NotExpr {
    fn render_dyn(&self, buf: &mut String) {
        buf.push_str("NOT ");
        self.inner.render_dyn(buf);
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
}

combinators!(Grouped, Func);

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

#[derive(Debug, Clone, Copy)]
pub enum Order {
    Asc,
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
