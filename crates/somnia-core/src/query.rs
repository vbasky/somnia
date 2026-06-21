//! SurrealQL statement builders.
//!
//! Each builder is reached from [`Table`] (itself produced by the derived
//! `Type::table()`) and rendered to a string with `to_surrealql()`:
//!
//! - [`Select`] — `SELECT … FROM …`
//! - [`Create`] — `CREATE …`
//! - [`Insert`] — `INSERT INTO …`
//! - [`Update`] — `UPDATE …` (and `UPSERT …` via [`Table::upsert`])
//! - [`Delete`] — `DELETE …`
//! - [`Relate`] / [`RelateEdge`] — `RELATE a -> edge -> b`
//! - [`Batch`] — several statements joined with `;`
//!
//! Mutations also offer `then_select(...)` to chain a reselect as a batch.

use crate::{
    expr::{Column, DynExpr, Order, Path, Projection, RecordLink, SurrealQL},
    types::{SurrealEdge, SurrealRecord, Thing},
};
use std::collections::BTreeMap;

/// How a mutating statement should return its affected rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Returning {
    /// no RETURN clause
    None,
    /// `RETURN NONE`
    Nothing,
    /// `RETURN BEFORE`
    Before,
    /// `RETURN AFTER`
    After,
    /// `RETURN DIFF`
    Diff,
}

impl Returning {
    fn render(self, buf: &mut String) {
        match self {
            Returning::None => {}
            Returning::Nothing => buf.push_str(" RETURN NONE"),
            Returning::Before => buf.push_str(" RETURN BEFORE"),
            Returning::After => buf.push_str(" RETURN AFTER"),
            Returning::Diff => buf.push_str(" RETURN DIFF"),
        }
    }
}

/// A statement target: either a whole table (`asset`) or a single record link
/// (`type::record('asset', '<id>')`).
enum Target {
    Table(&'static str),
    Record(RecordLink),
}

impl Target {
    fn render(&self, buf: &mut String) {
        match self {
            Target::Table(t) => buf.push_str(t),
            Target::Record(r) => r.render_dyn(buf),
        }
    }
    fn render_params(&self, buf: &mut String, params: &mut BTreeMap<String, serde_json::Value>) {
        match self {
            Target::Table(t) => buf.push_str(t),
            Target::Record(r) => r.render_dyn_params(buf, params),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Table
// ═══════════════════════════════════════════════════════════════════════════════

/// Entry point to the query builder for a record type `T` — the value returned by
/// the derived `T::table()`. Each method starts a statement builder.
pub struct Table<T: SurrealRecord> {
    _marker: std::marker::PhantomData<T>,
}

impl<T: SurrealRecord> Table<T> {
    /// Create a `Table` builder. Prefer the derived `T::table()`.
    pub fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }

    /// Begin a `SELECT * FROM <table>` (pass the derived `T::all()`).
    pub fn select(self, _cols: crate::expr::ColumnSet<T>) -> Select<T> {
        Select::bare()
    }

    /// Select an explicit projection list (`SELECT <fields…> FROM table`).
    pub fn project(self, fields: Vec<Projection>) -> Select<T> {
        let mut s = Select::bare();
        s.projections = fields;
        s
    }

    /// `SELECT <path> AS <alias> FROM table` — project a graph traversal.
    /// A convenience for `project(vec![Projection::aliased(path, alias)])`.
    pub fn project_path(self, path: Path, alias: &'static str) -> Select<T> {
        let mut s = Select::bare();
        s.projections = vec![Projection::aliased(path, alias)];
        s
    }

    /// `SELECT count() FROM table GROUP ALL` (the argument is currently ignored).
    pub fn count(self, _field: &str) -> Select<T> {
        let mut s = Select::bare();
        s.count = true;
        s.group_all = true;
        s
    }

    /// Begin an `INSERT INTO <table> …`.
    pub fn insert(self) -> Insert<T> {
        Insert {
            data: Vec::new(),
            return_fields: vec![],
            returning: Returning::None,
        }
    }
    /// Begin a `CREATE <table> …`.
    pub fn create(self) -> Create<T> {
        Create::for_table()
    }
    /// Begin an `UPDATE <table> …`.
    pub fn update(self) -> Update<T> {
        Update::for_table()
    }
    /// `UPSERT` — update the matching record, or create it if it doesn't exist.
    /// Same builder surface as [`update`](Self::update) (`record`/`set`/`merge`/
    /// `content`/`filter`/`returning`/`then_select`).
    pub fn upsert(self) -> Update<T> {
        Update::for_upsert()
    }
    /// Begin a `DELETE <table> …`.
    pub fn delete(self) -> Delete<T> {
        Delete::for_table()
    }
}

impl<T: SurrealRecord> Default for Table<T> {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SELECT
// ═══════════════════════════════════════════════════════════════════════════════

/// A `SELECT` statement builder: projections, `WHERE`, `ORDER BY`, `LIMIT`,
/// `START`, `FETCH`, `GROUP BY`/`GROUP ALL`, `count()`, and the modifiers
/// `VALUE`/`OMIT`/`SPLIT`/`WITH`/`TIMEOUT`/`EXPLAIN`.
pub struct Select<T: SurrealRecord> {
    _marker: std::marker::PhantomData<T>,
    projections: Vec<Projection>,
    value: bool,
    omit: Vec<String>,
    with: Option<String>,
    filter: Option<Box<dyn DynExpr>>,
    split: Vec<String>,
    order: Vec<(String, Order)>,
    limit: Option<u32>,
    start: u32,
    fetch: Vec<String>,
    group_by: Vec<String>,
    group_all: bool,
    count: bool,
    count_alias: Option<&'static str>,
    timeout: Option<String>,
    explain: Option<bool>,
    from_sub: Option<Box<Select<T>>>,
}

impl<T: SurrealRecord> Select<T> {
    fn bare() -> Self {
        Select {
            _marker: std::marker::PhantomData,
            projections: Vec::new(),
            value: false,
            omit: Vec::new(),
            with: None,
            filter: None,
            split: Vec::new(),
            order: Vec::new(),
            limit: None,
            start: 0,
            fetch: Vec::new(),
            group_by: Vec::new(),
            group_all: false,
            count: false,
            count_alias: None,
            timeout: None,
            explain: None,
            from_sub: None,
        }
    }

    pub fn filter(mut self, expr: impl DynExpr + 'static) -> Self {
        self.filter = Some(Box::new(expr));
        self
    }
    /// Add a `<path> AS <alias>` graph-traversal projection to the select list.
    /// Appends to any existing projections, so `select(T::all()).with_path(p, "x")`
    /// renders `SELECT *, <path> AS x` (a `*` is emitted only when the list is
    /// otherwise empty).
    pub fn with_path(mut self, path: Path, alias: &'static str) -> Self {
        if self.projections.is_empty() {
            self.projections
                .push(Projection::new(crate::expr::Raw("*".to_string())));
        }
        self.projections.push(Projection::aliased(path, alias));
        self
    }
    pub fn limit(mut self, n: u32) -> Self {
        self.limit = Some(n);
        self
    }
    pub fn start(mut self, n: u32) -> Self {
        self.start = n;
        self
    }
    pub fn fetch(mut self, field: &str) -> Self {
        self.fetch.push(field.to_string());
        self
    }
    pub fn group_by<C: DynExpr>(mut self, col: C) -> Self {
        let mut buf = String::new();
        col.render_dyn(&mut buf);
        self.group_by.push(buf);
        self
    }
    /// `GROUP ALL` (whole-table aggregate, e.g. with `count()`).
    pub fn group_all(mut self) -> Self {
        self.group_all = true;
        self
    }
    /// Alias for the `count()` projection: `SELECT count() AS <alias>`.
    pub fn count_as(mut self, alias: &'static str) -> Self {
        self.count = true;
        self.count_alias = Some(alias);
        self
    }

    /// `SELECT VALUE …` — return bare values instead of field-wrapping objects.
    /// Pair with a single projection (e.g. `project(vec![col("name")]).value()`).
    pub fn value(mut self) -> Self {
        self.value = true;
        self
    }
    /// `OMIT <field>` — exclude a field from a `SELECT *`.
    pub fn omit(mut self, field: &str) -> Self {
        self.omit.push(field.to_string());
        self
    }
    /// `SPLIT <field>` — fan one row out into multiple rows by an array field.
    pub fn split(mut self, field: &str) -> Self {
        self.split.push(field.to_string());
        self
    }
    /// `WITH INDEX <a, b>` — force the planner to use the named index(es).
    pub fn with_index<I, S>(mut self, indexes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let list = indexes
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        self.with = Some(format!("WITH INDEX {list}"));
        self
    }
    /// `WITH NOINDEX` — force a table scan (ignore indexes).
    pub fn with_no_index(mut self) -> Self {
        self.with = Some("WITH NOINDEX".to_string());
        self
    }
    /// `TIMEOUT <duration>` — abort the query after the given duration (e.g. `"5s"`).
    pub fn timeout(mut self, duration: impl Into<String>) -> Self {
        self.timeout = Some(duration.into());
        self
    }
    /// `SELECT … FROM (<subquery>)` — read from a subquery instead of the base
    /// table. The subquery (a `Select<T>` of the same record type) renders
    /// parenthesized in place of the table name.
    pub fn from_subquery(mut self, sub: Select<T>) -> Self {
        self.from_sub = Some(Box::new(sub));
        self
    }

    /// `EXPLAIN` — return the query plan instead of results.
    pub fn explain(mut self) -> Self {
        self.explain = Some(false);
        self
    }
    /// `EXPLAIN FULL` — return the query plan with execution detail.
    pub fn explain_full(mut self) -> Self {
        self.explain = Some(true);
        self
    }

    pub fn order_by<C: DynExpr>(mut self, col: C, dir: Order) -> Self {
        let mut buf = String::new();
        col.render_dyn(&mut buf);
        self.order.push((buf, dir));
        self
    }

    pub fn order_asc<C: DynExpr>(self, col: C) -> Self {
        self.order_by(col, Order::Asc)
    }
    pub fn order_desc<C: DynExpr>(self, col: C) -> Self {
        self.order_by(col, Order::Desc)
    }

    fn render_select_list(&self, q: &mut String) {
        if self.count {
            q.push_str("count()");
            if let Some(a) = self.count_alias {
                q.push_str(" AS ");
                q.push_str(a);
            }
        } else if self.projections.is_empty() {
            q.push('*');
        } else {
            for (i, p) in self.projections.iter().enumerate() {
                if i > 0 {
                    q.push_str(", ");
                }
                p.render(q);
            }
        }
    }

    fn render_select_list_params(
        &self,
        q: &mut String,
        params: &mut BTreeMap<String, serde_json::Value>,
    ) {
        if self.count {
            q.push_str("count()");
            if let Some(a) = self.count_alias {
                q.push_str(" AS ");
                q.push_str(a);
            }
        } else if self.projections.is_empty() {
            q.push('*');
        } else {
            for (i, p) in self.projections.iter().enumerate() {
                if i > 0 {
                    q.push_str(", ");
                }
                p.render_params(q, params);
            }
        }
    }

    /// Shared renderer for both inline and `$param` modes. When `param_mode` is
    /// set, literals render as `$pN` placeholders collected into `params`;
    /// otherwise they render inline (and `params` is ignored). A single map is
    /// threaded through so a nested subquery's params merge into the parent's.
    fn render(
        &self,
        q: &mut String,
        params: &mut BTreeMap<String, serde_json::Value>,
        param_mode: bool,
    ) {
        q.push_str("SELECT ");
        if self.value {
            q.push_str("VALUE ");
        }
        if param_mode {
            self.render_select_list_params(q, params);
        } else {
            self.render_select_list(q);
        }
        if !self.omit.is_empty() {
            q.push_str(" OMIT ");
            q.push_str(&self.omit.join(", "));
        }
        q.push_str(" FROM ");
        match &self.from_sub {
            Some(sub) => {
                q.push('(');
                sub.render(q, params, param_mode);
                q.push(')');
            }
            None => q.push_str(T::table_name()),
        }
        if let Some(w) = &self.with {
            q.push(' ');
            q.push_str(w);
        }
        if let Some(ref f) = self.filter {
            q.push_str(" WHERE ");
            if param_mode {
                f.render_dyn_params(q, params);
            } else {
                f.render_dyn(q);
            }
        }
        for (i, s) in self.split.iter().enumerate() {
            q.push_str(if i == 0 { " SPLIT " } else { ", " });
            q.push_str(s);
        }
        for (i, (col, dir)) in self.order.iter().enumerate() {
            q.push_str(if i == 0 { " ORDER BY " } else { ", " });
            q.push_str(&format!("{col} {dir}"));
        }
        for (i, g) in self.group_by.iter().enumerate() {
            q.push_str(if i == 0 { " GROUP BY " } else { ", " });
            q.push_str(g);
        }
        if self.group_all {
            q.push_str(" GROUP ALL");
        }
        if self.start > 0 {
            q.push_str(&format!(" START {}", self.start));
        }
        if let Some(n) = self.limit {
            q.push_str(&format!(" LIMIT {n}"));
        }
        for f in &self.fetch {
            q.push_str(&format!(" FETCH {f}"));
        }
        if let Some(t) = &self.timeout {
            q.push_str(" TIMEOUT ");
            q.push_str(t);
        }
        match self.explain {
            Some(true) => q.push_str(" EXPLAIN FULL"),
            Some(false) => q.push_str(" EXPLAIN"),
            None => {}
        }
    }

    pub fn to_surrealql(&self) -> String {
        let mut q = String::new();
        let mut sink = BTreeMap::new();
        self.render(&mut q, &mut sink, false);
        q
    }

    /// Render the statement with `$param` placeholders instead of inlined
    /// literals, returning the SQL string and a map of parameter name to value.
    /// Literal values become numbered `$p0`, `$p1`, …; explicit [`Param`](crate::expr::Param)
    /// wrappers use their declared name.
    pub fn to_surrealql_with_params(&self) -> (String, BTreeMap<String, serde_json::Value>) {
        let mut params = BTreeMap::new();
        let mut q = String::new();
        self.render(&mut q, &mut params, true);
        (q, params)
    }
}

impl<T: SurrealRecord> std::fmt::Debug for Select<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Select")
            .field("sql", &self.to_surrealql())
            .finish()
    }
}

/// A `Select` is usable as an expression — rendered parenthesized — so it can be
/// embedded as a subquery: a scalar/`IN` operand in a `WHERE`, a projection, or a
/// `SET`/`FROM` value. Params from the subquery merge into the parent's map.
impl<T: SurrealRecord> DynExpr for Select<T> {
    fn render_dyn(&self, buf: &mut String) {
        let mut sink = BTreeMap::new();
        buf.push('(');
        self.render(buf, &mut sink, false);
        buf.push(')');
    }
    fn render_dyn_params(
        &self,
        buf: &mut String,
        params: &mut BTreeMap<String, serde_json::Value>,
    ) {
        buf.push('(');
        self.render(buf, params, true);
        buf.push(')');
    }
}

impl<T: SurrealRecord> std::fmt::Display for Select<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_surrealql())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// INSERT
// ═══════════════════════════════════════════════════════════════════════════════

/// An `INSERT INTO <table> …` builder. Records are serialized inline as object
/// literals; rendering requires `T: serde::Serialize`.
pub struct Insert<T: SurrealRecord> {
    data: Vec<T>,
    return_fields: Vec<&'static str>,
    returning: Returning,
}

impl<T: SurrealRecord> Insert<T> {
    pub fn content(mut self, record: T) -> Self {
        self.data.push(record);
        self
    }
    /// Add a field to the `RETURN <projection>` list. Multiple calls accumulate
    /// (`RETURN id, name`). Takes precedence over [`returning`](Self::returning).
    pub fn return_field(mut self, field: &'static str) -> Self {
        self.return_fields.push(field);
        self
    }
    /// Set a `RETURN NONE|BEFORE|AFTER|DIFF` clause (used when no explicit
    /// [`return_field`](Self::return_field) projection is given).
    pub fn returning(mut self, r: Returning) -> Self {
        self.returning = r;
        self
    }
    pub fn data(&self) -> &[T] {
        &self.data
    }

    /// Render `INSERT INTO <table> <object|array> [RETURN …]`, serializing the
    /// queued record(s) inline as SurrealQL object literals (JSON is a valid
    /// subset). A single record renders as `{ … }`, multiple as `[ {…}, {…} ]`.
    /// A `RETURN` projection (from [`return_field`](Self::return_field)) renders
    /// the field list; otherwise the [`returning`](Self::returning) variant.
    pub fn to_surrealql(&self) -> String
    where
        T: serde::Serialize,
    {
        let body = match self.data.as_slice() {
            [] => "[]".to_string(),
            [one] => serde_json::to_string(one).unwrap_or_else(|_| "{}".to_string()),
            many => serde_json::to_string(many).unwrap_or_else(|_| "[]".to_string()),
        };
        let mut q = format!("INSERT INTO {} {}", T::table_name(), body);
        if !self.return_fields.is_empty() {
            q.push_str(" RETURN ");
            q.push_str(&self.return_fields.join(", "));
        } else {
            self.returning.render(&mut q);
        }
        q
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// UPDATE
// ═══════════════════════════════════════════════════════════════════════════════

enum SetVal {
    /// `SET k = <expr>`
    Assign(String, Box<dyn DynExpr>),
    /// `MERGE <expr>`
    Merge(Box<dyn DynExpr>),
    /// `CONTENT <expr>` (full replace)
    Content(Box<dyn DynExpr>),
}

impl SetVal {
    fn render(&self, buf: &mut String, set_pairs: &mut Vec<String>) {
        match self {
            SetVal::Assign(k, v) => {
                let mut val_buf = String::new();
                v.render_dyn(&mut val_buf);
                set_pairs.push(format!("{k} = {val_buf}"));
            }
            SetVal::Merge(v) => {
                let mut val_buf = String::new();
                v.render_dyn(&mut val_buf);
                buf.push_str(" MERGE ");
                buf.push_str(&val_buf);
            }
            SetVal::Content(v) => {
                let mut val_buf = String::new();
                v.render_dyn(&mut val_buf);
                buf.push_str(" CONTENT ");
                buf.push_str(&val_buf);
            }
        }
    }
    fn render_params(
        &self,
        buf: &mut String,
        set_pairs: &mut Vec<String>,
        params: &mut BTreeMap<String, serde_json::Value>,
    ) {
        match self {
            SetVal::Assign(k, v) => {
                let mut val_buf = String::new();
                v.render_dyn_params(&mut val_buf, params);
                set_pairs.push(format!("{k} = {val_buf}"));
            }
            SetVal::Merge(v) => {
                let mut val_buf = String::new();
                v.render_dyn_params(&mut val_buf, params);
                buf.push_str(" MERGE ");
                buf.push_str(&val_buf);
            }
            SetVal::Content(v) => {
                let mut val_buf = String::new();
                v.render_dyn_params(&mut val_buf, params);
                buf.push_str(" CONTENT ");
                buf.push_str(&val_buf);
            }
        }
    }
}

/// An `UPDATE`/`UPSERT` builder: `SET` / `MERGE` / `CONTENT`, an optional `WHERE`,
/// and `RETURN`. Built via [`Table::update`] or [`Table::upsert`].
pub struct Update<T: SurrealRecord> {
    _marker: std::marker::PhantomData<T>,
    verb: &'static str,
    target: Target,
    filter: Option<Box<dyn DynExpr>>,
    sets: Vec<SetVal>,
    returning: Returning,
}

impl<T: SurrealRecord> Update<T> {
    pub(crate) fn for_table() -> Self {
        Self::with_verb("UPDATE")
    }

    /// An `UPSERT` statement — same builder surface as `UPDATE`, but creates the
    /// record if it doesn't exist. Built via [`Table::upsert`].
    pub(crate) fn for_upsert() -> Self {
        Self::with_verb("UPSERT")
    }

    fn with_verb(verb: &'static str) -> Self {
        Self {
            _marker: std::marker::PhantomData,
            verb,
            target: Target::Table(T::table_name()),
            filter: None,
            sets: Vec::new(),
            returning: Returning::None,
        }
    }

    /// Target a single record: `UPDATE type::record('table', <id>)`.
    pub fn record<V: SurrealQL>(mut self, id: V) -> Self {
        self.target = Target::Record(RecordLink::new(T::table_name(), id));
        self
    }

    pub fn filter(mut self, expr: impl DynExpr + 'static) -> Self {
        self.filter = Some(Box::new(expr));
        self
    }

    /// `SET col = <literal>`.
    pub fn set<C: SurrealQL>(mut self, col: Column<T, C>, value: C) -> Self {
        self.sets.push(SetVal::Assign(
            col.name.to_string(),
            Box::new(crate::expr::Literal(value)),
        ));
        self
    }
    /// `SET col = <literal>` by raw column name.
    pub fn set_lit<C: SurrealQL>(mut self, col: &str, value: C) -> Self {
        self.sets.push(SetVal::Assign(
            col.to_string(),
            Box::new(crate::expr::Literal(value)),
        ));
        self
    }
    /// `SET col = <expr>` — e.g. a record link, `time::now()`, NONE, `use_count + 1`.
    pub fn set_expr(mut self, col: &str, expr: impl DynExpr + 'static) -> Self {
        self.sets
            .push(SetVal::Assign(col.to_string(), Box::new(expr)));
        self
    }
    /// `SET col = <raw SurrealQL>`.
    pub fn set_raw(mut self, col: &str, raw: impl Into<String>) -> Self {
        self.sets.push(SetVal::Assign(
            col.to_string(),
            Box::new(crate::expr::Raw(raw.into())),
        ));
        self
    }
    /// `MERGE <expr>` — deep-merge the given object into the record.
    pub fn merge(mut self, expr: impl DynExpr + 'static) -> Self {
        self.sets.push(SetVal::Merge(Box::new(expr)));
        self
    }
    /// `CONTENT <expr>` — full-replace the record's content (upsert by record id).
    pub fn content(mut self, expr: impl DynExpr + 'static) -> Self {
        self.sets.push(SetVal::Content(Box::new(expr)));
        self
    }
    pub fn returning(mut self, r: Returning) -> Self {
        self.returning = r;
        self
    }

    /// Follow this `UPDATE` with a reselecting [`Select`], joined as a `;`-separated
    /// batch. See [`Create::then_select`] for motivation.
    pub fn then_select(self, select: Select<T>) -> String {
        format!("{};\n{}", self.to_surrealql(), select.to_surrealql())
    }

    /// Like [`then_select`](Self::then_select) but renders with `$param` placeholders.
    pub fn then_select_params(
        self,
        select: Select<T>,
    ) -> (String, BTreeMap<String, serde_json::Value>) {
        let (mut_q, mut params) = self.to_surrealql_with_params();
        let (sel_q, sel_params) = select.to_surrealql_with_params();
        params.extend(sel_params);
        (format!("{mut_q};\n{sel_q}"), params)
    }

    pub fn to_surrealql(&self) -> String {
        let mut q = String::from(self.verb);
        q.push(' ');
        self.target.render(&mut q);
        // SurrealQL order: SET/MERGE/CONTENT first, then WHERE, then RETURN.
        let mut set_pairs = Vec::new();
        let mut trait_buf = String::new();
        for s in &self.sets {
            s.render(&mut trait_buf, &mut set_pairs);
        }
        if !trait_buf.is_empty() {
            q.push_str(&trait_buf);
        } else if !set_pairs.is_empty() {
            q.push_str(" SET ");
            q.push_str(&set_pairs.join(", "));
        }
        if let Some(ref f) = self.filter {
            q.push_str(" WHERE ");
            f.render_dyn(&mut q);
        }
        self.returning.render(&mut q);
        q
    }

    /// Render with `$param` placeholders instead of inlined literals.
    pub fn to_surrealql_with_params(&self) -> (String, BTreeMap<String, serde_json::Value>) {
        let mut params = BTreeMap::new();
        let mut q = String::from(self.verb);
        q.push(' ');
        self.target.render_params(&mut q, &mut params);
        let mut set_pairs = Vec::new();
        let mut trait_buf = String::new();
        for s in &self.sets {
            s.render_params(&mut trait_buf, &mut set_pairs, &mut params);
        }
        if !trait_buf.is_empty() {
            q.push_str(&trait_buf);
        } else if !set_pairs.is_empty() {
            q.push_str(" SET ");
            q.push_str(&set_pairs.join(", "));
        }
        if let Some(ref f) = self.filter {
            q.push_str(" WHERE ");
            f.render_dyn_params(&mut q, &mut params);
        }
        self.returning.render(&mut q);
        (q, params)
    }
}

impl<T: SurrealRecord> std::fmt::Display for Update<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_surrealql())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CREATE
// ═══════════════════════════════════════════════════════════════════════════════

enum CreateBody {
    /// `CONTENT <expr>`
    Content(Box<dyn DynExpr>),
    /// `SET a = x, b = y`
    Set(Vec<(String, Box<dyn DynExpr>)>),
}

/// `CREATE <target> [CONTENT … | SET …] [RETURN …]`.
pub struct Create<T: SurrealRecord> {
    _marker: std::marker::PhantomData<T>,
    target: Target,
    body: CreateBody,
    returning: Returning,
}

impl<T: SurrealRecord> Create<T> {
    pub(crate) fn for_table() -> Self {
        Self {
            _marker: std::marker::PhantomData,
            target: Target::Table(T::table_name()),
            body: CreateBody::Set(Vec::new()),
            returning: Returning::None,
        }
    }

    /// Target a single record id: `CREATE type::record('table', <id>)`.
    pub fn record<V: SurrealQL>(mut self, id: V) -> Self {
        self.target = Target::Record(RecordLink::new(T::table_name(), id));
        self
    }

    /// `CONTENT <expr>` — replaces any accumulated SET pairs.
    pub fn content(mut self, expr: impl DynExpr + 'static) -> Self {
        self.body = CreateBody::Content(Box::new(expr));
        self
    }

    /// `SET col = <literal>`.
    pub fn set_lit<C: SurrealQL>(mut self, col: &str, value: C) -> Self {
        self.push_set(col, Box::new(crate::expr::Literal(value)));
        self
    }
    /// `SET col = <expr>`.
    pub fn set_expr(mut self, col: &str, expr: impl DynExpr + 'static) -> Self {
        self.push_set(col, Box::new(expr));
        self
    }
    /// `SET col = <raw SurrealQL>`.
    pub fn set_raw(mut self, col: &str, raw: impl Into<String>) -> Self {
        self.push_set(col, Box::new(crate::expr::Raw(raw.into())));
        self
    }

    fn push_set(&mut self, col: &str, expr: Box<dyn DynExpr>) {
        match &mut self.body {
            CreateBody::Set(v) => v.push((col.to_string(), expr)),
            CreateBody::Content(_) => {
                self.body = CreateBody::Set(vec![(col.to_string(), expr)]);
            }
        }
    }

    pub fn returning(mut self, r: Returning) -> Self {
        self.returning = r;
        self
    }

    /// Follow this `CREATE` with a reselecting [`Select`], joined as a `;`-separated
    /// batch. The select is rendered immediately, producing a complete SurrealQL
    /// string ready for `db.query()`.
    ///
    /// This replaces the manual `Batch::new().push(create).push(select).to_surrealql()`
    /// pattern for mutate-then-reselect workflows.
    pub fn then_select(self, select: Select<T>) -> String {
        format!("{};\n{}", self.to_surrealql(), select.to_surrealql())
    }

    /// Like [`then_select`](Self::then_select) but renders with `$param` placeholders.
    pub fn then_select_params(
        self,
        select: Select<T>,
    ) -> (String, BTreeMap<String, serde_json::Value>) {
        let (mut_q, mut params) = self.to_surrealql_with_params();
        let (sel_q, sel_params) = select.to_surrealql_with_params();
        params.extend(sel_params);
        (format!("{mut_q};\n{sel_q}"), params)
    }

    pub fn to_surrealql(&self) -> String {
        let mut q = String::from("CREATE ");
        self.target.render(&mut q);
        match &self.body {
            CreateBody::Content(c) => {
                q.push_str(" CONTENT ");
                c.render_dyn(&mut q);
            }
            CreateBody::Set(pairs) if !pairs.is_empty() => {
                q.push_str(" SET ");
                q.push_str(
                    &pairs
                        .iter()
                        .map(|(k, v)| {
                            let mut val = String::new();
                            v.render_dyn(&mut val);
                            format!("{k} = {val}")
                        })
                        .collect::<Vec<_>>()
                        .join(", "),
                );
            }
            CreateBody::Set(_) => {}
        }
        self.returning.render(&mut q);
        q
    }

    /// Render with `$param` placeholders instead of inlined literals.
    pub fn to_surrealql_with_params(&self) -> (String, BTreeMap<String, serde_json::Value>) {
        let mut params = BTreeMap::new();
        let mut q = String::from("CREATE ");
        self.target.render_params(&mut q, &mut params);
        match &self.body {
            CreateBody::Content(c) => {
                q.push_str(" CONTENT ");
                c.render_dyn_params(&mut q, &mut params);
            }
            CreateBody::Set(pairs) if !pairs.is_empty() => {
                q.push_str(" SET ");
                q.push_str(
                    &pairs
                        .iter()
                        .map(|(k, v)| {
                            let mut val = String::new();
                            v.render_dyn_params(&mut val, &mut params);
                            format!("{k} = {val}")
                        })
                        .collect::<Vec<_>>()
                        .join(", "),
                );
            }
            CreateBody::Set(_) => {}
        }
        self.returning.render(&mut q);
        (q, params)
    }
}

impl<T: SurrealRecord> std::fmt::Display for Create<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_surrealql())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DELETE
// ═══════════════════════════════════════════════════════════════════════════════

/// A `DELETE <target> [WHERE …] [RETURN …]` builder.
pub struct Delete<T: SurrealRecord> {
    _marker: std::marker::PhantomData<T>,
    target: Target,
    filter: Option<Box<dyn DynExpr>>,
    returning: Returning,
}

impl<T: SurrealRecord> Delete<T> {
    pub(crate) fn for_table() -> Self {
        Self {
            _marker: std::marker::PhantomData,
            target: Target::Table(T::table_name()),
            filter: None,
            returning: Returning::None,
        }
    }
    /// Target a single record: `DELETE type::record('table', <id>)`.
    pub fn record<V: SurrealQL>(mut self, id: V) -> Self {
        self.target = Target::Record(RecordLink::new(T::table_name(), id));
        self
    }
    pub fn filter(mut self, expr: impl DynExpr + 'static) -> Self {
        self.filter = Some(Box::new(expr));
        self
    }
    pub fn returning(mut self, r: Returning) -> Self {
        self.returning = r;
        self
    }

    /// Follow this `DELETE` with a reselecting [`Select`], joined as a `;`-separated
    /// batch. See [`Create::then_select`] for motivation.
    pub fn then_select(self, select: Select<T>) -> String {
        format!("{};\n{}", self.to_surrealql(), select.to_surrealql())
    }

    /// Like [`then_select`](Self::then_select) but renders with `$param` placeholders.
    pub fn then_select_params(
        self,
        select: Select<T>,
    ) -> (String, BTreeMap<String, serde_json::Value>) {
        let (mut_q, mut params) = self.to_surrealql_with_params();
        let (sel_q, sel_params) = select.to_surrealql_with_params();
        params.extend(sel_params);
        (format!("{mut_q};\n{sel_q}"), params)
    }

    pub fn to_surrealql(&self) -> String {
        let mut q = String::from("DELETE ");
        self.target.render(&mut q);
        if let Some(ref f) = self.filter {
            q.push_str(" WHERE ");
            f.render_dyn(&mut q);
        }
        self.returning.render(&mut q);
        q
    }

    /// Render with `$param` placeholders instead of inlined literals.
    pub fn to_surrealql_with_params(&self) -> (String, BTreeMap<String, serde_json::Value>) {
        let mut params = BTreeMap::new();
        let mut q = String::from("DELETE ");
        self.target.render_params(&mut q, &mut params);
        if let Some(ref f) = self.filter {
            q.push_str(" WHERE ");
            f.render_dyn_params(&mut q, &mut params);
        }
        self.returning.render(&mut q);
        (q, params)
    }
}

impl<T: SurrealRecord> std::fmt::Display for Delete<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_surrealql())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Batch — multiple statements joined by `;` (mutate-then-reselect pattern)
// ═══════════════════════════════════════════════════════════════════════════════

/// Concatenates SurrealQL statements with `;` separators. The store's typical
/// pattern is a mutation followed by a SELECT that re-projects the row.
#[derive(Default)]
pub struct Batch {
    statements: Vec<String>,
}

impl Batch {
    pub fn new() -> Self {
        Self {
            statements: Vec::new(),
        }
    }
    pub fn push(mut self, stmt: impl ToString) -> Self {
        self.statements.push(stmt.to_string());
        self
    }
    pub fn to_surrealql(&self) -> String {
        self.statements.join(";\n")
    }
    /// Number of statements (useful for `.take(n)` indexing on the response).
    pub fn len(&self) -> usize {
        self.statements.len()
    }
    pub fn is_empty(&self) -> bool {
        self.statements.is_empty()
    }
}

impl std::fmt::Display for Batch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_surrealql())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Transaction — BEGIN … COMMIT/CANCEL (atomic multi-statement)
// ═══════════════════════════════════════════════════════════════════════════════

/// Wraps statements in a SurrealDB transaction —
/// `BEGIN TRANSACTION; … ; COMMIT TRANSACTION;`. Either every statement applies
/// or none do: SurrealDB rolls the whole block back if any statement errors, and
/// [`cancel`](Self::cancel) terminates with `CANCEL TRANSACTION` to roll back
/// explicitly. Unlike [`Batch`] (a plain `;`-joined sequence), a transaction is
/// atomic.
///
/// Push already-rendered statements (`to_surrealql()` output); each is
/// `;`-terminated automatically.
#[derive(Default)]
pub struct Transaction {
    statements: Vec<String>,
    cancel: bool,
}

impl Transaction {
    pub fn new() -> Self {
        Self::default()
    }
    /// Add a statement to the transaction body.
    pub fn push(mut self, stmt: impl ToString) -> Self {
        self.statements.push(stmt.to_string());
        self
    }
    /// Terminate with `CANCEL TRANSACTION` (roll back) instead of `COMMIT`.
    pub fn cancel(mut self) -> Self {
        self.cancel = true;
        self
    }
    pub fn to_surrealql(&self) -> String {
        let mut out = String::from("BEGIN TRANSACTION;\n");
        for s in &self.statements {
            out.push_str(s);
            if !s.trim_end().ends_with(';') {
                out.push(';');
            }
            out.push('\n');
        }
        out.push_str(if self.cancel {
            "CANCEL TRANSACTION;"
        } else {
            "COMMIT TRANSACTION;"
        });
        out
    }
    /// Number of statements in the transaction body (excludes BEGIN/COMMIT).
    pub fn len(&self) -> usize {
        self.statements.len()
    }
    pub fn is_empty(&self) -> bool {
        self.statements.is_empty()
    }
}

impl std::fmt::Display for Transaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_surrealql())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// RELATE — graph edges
// ═══════════════════════════════════════════════════════════════════════════════

/// Render a record's id as `table:<escaped-key>` into `buf`.
fn record_id(thing: &Thing<impl SurrealRecord>, buf: &mut String) {
    buf.push_str(thing.table());
    buf.push(':');
    thing.key.render_id(buf);
}

/// Return a record's id as a `table:<escaped-key>` string.
fn record_id_string(thing: &Thing<impl SurrealRecord>) -> String {
    let mut s = String::new();
    record_id(thing, &mut s);
    s
}

/// Builds a graph edge statement `RELATE a -> edge -> b` for an edge type `E`.
/// For edges that carry their own fields, see [`RelateEdge`].
pub struct Relate<E: SurrealEdge> {
    _marker: std::marker::PhantomData<E>,
}

impl<E: SurrealEdge> Relate<E> {
    pub fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }

    pub fn to_surrealql(
        from: &Thing<impl SurrealRecord>,
        to: &Thing<impl SurrealRecord>,
    ) -> String {
        let mut q = String::from("RELATE ");
        record_id(from, &mut q);
        q.push_str(" -> ");
        q.push_str(E::edge_name());
        q.push_str(" -> ");
        record_id(to, &mut q);
        q
    }
}

impl<E: SurrealEdge> Default for Relate<E> {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// RELATE with content
// ═══════════════════════════════════════════════════════════════════════════════

/// Build a RELATE query with edge content.
///
/// ```ignore
/// RelateEdge::<Follows>::from(user).to(other).content(Follows { since: now }).build()
/// ```
pub struct RelateEdge<E: SurrealEdge> {
    _marker: std::marker::PhantomData<E>,
    from_label: String,
    to_label: String,
    content_json: Option<serde_json::Value>,
    return_fields: Vec<&'static str>,
    returning: Returning,
}

impl<E: SurrealEdge> RelateEdge<E> {
    pub fn from(from: &Thing<impl SurrealRecord>) -> Self {
        Self {
            _marker: std::marker::PhantomData,
            from_label: record_id_string(from),
            to_label: String::new(),
            content_json: None,
            return_fields: Vec::new(),
            returning: Returning::None,
        }
    }

    pub fn to(mut self, to: &Thing<impl SurrealRecord>) -> Self {
        self.to_label = record_id_string(to);
        self
    }

    /// Attach content to the edge record.
    pub fn content(mut self, edge: &impl serde::Serialize) -> Self {
        self.content_json = serde_json::to_value(edge).ok();
        self
    }

    /// Add a field to the `RETURN <projection>` list (e.g. `RETURN id`). Multiple
    /// calls accumulate; takes precedence over [`returning`](Self::returning).
    pub fn return_field(mut self, field: &'static str) -> Self {
        self.return_fields.push(field);
        self
    }
    /// Set a `RETURN NONE|BEFORE|AFTER|DIFF` clause on the edge creation.
    pub fn returning(mut self, r: Returning) -> Self {
        self.returning = r;
        self
    }

    pub fn build(&self) -> String {
        let mut q = format!(
            "RELATE {} -> {} -> {}",
            self.from_label,
            E::edge_name(),
            self.to_label
        );
        if let Some(ref c) = self.content_json {
            q.push_str(&format!(
                " CONTENT {}",
                serde_json::to_string(c).unwrap_or_default()
            ));
        }
        if !self.return_fields.is_empty() {
            q.push_str(" RETURN ");
            q.push_str(&self.return_fields.join(", "));
        } else {
            self.returning.render(&mut q);
        }
        q
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// LET — session-scoped variable assignment
// ═══════════════════════════════════════════════════════════════════════════════

/// Builds a `LET $var = <expr>` statement for session-scoped variables.
/// The variable is available in subsequent queries within the same session.
///
/// ```ignore
/// LetVar::new("limit", 10u32).to_surrealql();      // LET $limit = 10;
/// LetVar::new("ts", Raw("time::now()")).to_surrealql(); // LET $ts = time::now();
/// ```
pub struct LetVar {
    name: String,
    value: Box<dyn DynExpr>,
}

impl LetVar {
    /// Create a `LET $name = <expr>` statement.
    pub fn new(name: impl Into<String>, value: impl DynExpr + 'static) -> Self {
        Self {
            name: name.into(),
            value: Box::new(value),
        }
    }

    /// Create a `LET $name = <literal>` statement.
    pub fn literal<V: SurrealQL>(name: impl Into<String>, value: V) -> Self {
        Self {
            name: name.into(),
            value: Box::new(crate::expr::Literal(value)),
        }
    }

    pub fn to_surrealql(&self) -> String {
        let mut q = format!("LET ${} = ", self.name);
        self.value.render_dyn(&mut q);
        q
    }

    /// Render with `$param` placeholders (the `LET` value becomes a `$param`).
    pub fn to_surrealql_with_params(&self) -> (String, BTreeMap<String, serde_json::Value>) {
        let mut params = BTreeMap::new();
        let mut q = format!("LET ${} = ", self.name);
        self.value.render_dyn_params(&mut q, &mut params);
        (q, params)
    }
}

impl std::fmt::Display for LetVar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_surrealql())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DEFINE INDEX
// ═══════════════════════════════════════════════════════════════════════════════

/// The kind of a `DEFINE INDEX` — what trails the field list.
enum IndexKind {
    /// a plain (non-unique) index — no trailing clause
    Plain,
    /// `UNIQUE`
    Unique,
    /// a verbatim trailing clause, e.g. `SEARCH ANALYZER ascii BM25 HIGHLIGHTS`
    /// or `HNSW DIMENSION 128 DIST COSINE` — the escape hatch for full-text and
    /// vector indexes whose exact options depend on the engine build.
    Raw(String),
}

/// Builds a `DEFINE INDEX` statement — plain, composite, `UNIQUE`, full-text
/// (`SEARCH`), or vector (`HNSW`/`MTREE`) indexes.
///
/// ```ignore
/// // DEFINE INDEX IF NOT EXISTS email_idx ON TABLE user FIELDS email UNIQUE
/// DefineIndex::new("email_idx", "user").field("email").unique().to_surrealql();
///
/// // composite, vector
/// DefineIndex::new("name_idx", "user").fields(["first", "last"]).to_surrealql();
/// DefineIndex::new("emb_idx", "doc").field("embedding").hnsw(128, "COSINE").to_surrealql();
/// ```
pub struct DefineIndex {
    name: String,
    table: String,
    fields: Vec<String>,
    kind: IndexKind,
    if_not_exists: bool,
    comment: Option<String>,
    concurrently: bool,
}

impl DefineIndex {
    /// Begin `DEFINE INDEX IF NOT EXISTS <name> ON TABLE <table>`.
    pub fn new(name: impl Into<String>, table: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            table: table.into(),
            fields: Vec::new(),
            kind: IndexKind::Plain,
            if_not_exists: true,
            comment: None,
            concurrently: false,
        }
    }

    /// Add one indexed field/column.
    pub fn field(mut self, name: impl Into<String>) -> Self {
        self.fields.push(name.into());
        self
    }
    /// Add several indexed fields/columns (a composite index).
    pub fn fields<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.fields.extend(names.into_iter().map(Into::into));
        self
    }

    /// Mark the index `UNIQUE`.
    pub fn unique(mut self) -> Self {
        self.kind = IndexKind::Unique;
        self
    }
    /// A full-text `SEARCH ANALYZER <analyzer>` index. Append further options
    /// (`BM25`, `HIGHLIGHTS`, …) with [`raw`](Self::raw) if your engine needs them.
    pub fn search(mut self, analyzer: &str) -> Self {
        self.kind = IndexKind::Raw(format!("SEARCH ANALYZER {analyzer}"));
        self
    }
    /// An `HNSW` vector index of the given dimension and distance function
    /// (e.g. `"COSINE"`, `"EUCLIDEAN"`).
    pub fn hnsw(mut self, dimension: u32, dist: &str) -> Self {
        self.kind = IndexKind::Raw(format!("HNSW DIMENSION {dimension} DIST {dist}"));
        self
    }
    /// An `MTREE` vector index of the given dimension and distance function.
    pub fn mtree(mut self, dimension: u32, dist: &str) -> Self {
        self.kind = IndexKind::Raw(format!("MTREE DIMENSION {dimension} DIST {dist}"));
        self
    }
    /// Set a verbatim trailing clause (the escape hatch for index options somnia
    /// doesn't model), e.g. `"SEARCH ANALYZER ascii BM25 HIGHLIGHTS"`.
    pub fn raw(mut self, tail: impl Into<String>) -> Self {
        self.kind = IndexKind::Raw(tail.into());
        self
    }

    /// Drop the `IF NOT EXISTS` guard.
    pub fn overwrite(mut self) -> Self {
        self.if_not_exists = false;
        self
    }
    /// Attach a `COMMENT '<text>'`.
    pub fn comment(mut self, text: impl Into<String>) -> Self {
        self.comment = Some(text.into());
        self
    }
    /// Build the index `CONCURRENTLY` (non-blocking).
    pub fn concurrently(mut self) -> Self {
        self.concurrently = true;
        self
    }

    pub fn to_surrealql(&self) -> String {
        let guard = if self.if_not_exists {
            "IF NOT EXISTS "
        } else {
            ""
        };
        let mut q = format!(
            "DEFINE INDEX {guard}{} ON TABLE {} FIELDS {}",
            self.name,
            self.table,
            self.fields.join(", "),
        );
        match &self.kind {
            IndexKind::Plain => {}
            IndexKind::Unique => q.push_str(" UNIQUE"),
            IndexKind::Raw(tail) => {
                q.push(' ');
                q.push_str(tail);
            }
        }
        if let Some(c) = &self.comment {
            let escaped = c.replace('\\', "\\\\").replace('\'', "\\'");
            q.push_str(&format!(" COMMENT '{escaped}'"));
        }
        if self.concurrently {
            q.push_str(" CONCURRENTLY");
        }
        q
    }

    /// `REMOVE INDEX IF EXISTS <name> ON TABLE <table>` — the inverse statement.
    pub fn remove(name: &str, table: &str) -> String {
        format!("REMOVE INDEX IF EXISTS {name} ON TABLE {table}")
    }
}

impl std::fmt::Display for DefineIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_surrealql())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DEFINE EVENT / FUNCTION / ANALYZER / PARAM
// ═══════════════════════════════════════════════════════════════════════════════

fn guard(if_not_exists: bool) -> &'static str {
    if if_not_exists {
        "IF NOT EXISTS "
    } else {
        ""
    }
}

/// `DEFINE EVENT <name> ON TABLE <table> WHEN <cond> THEN <block>` — a trigger
/// that fires on `CREATE`/`UPDATE`/`DELETE`. `$event`, `$before`, `$after`,
/// `$value` are available inside `when`/`then`.
///
/// ```ignore
/// DefineEvent::new("on_publish", "post")
///     .when("$event = 'UPDATE' AND $after.published = true")
///     .then("{ CREATE notification SET post = $after.id }")
///     .to_surrealql();
/// ```
pub struct DefineEvent {
    name: String,
    table: String,
    when: String,
    then: String,
    if_not_exists: bool,
}

impl DefineEvent {
    pub fn new(name: impl Into<String>, table: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            table: table.into(),
            when: String::new(),
            then: String::new(),
            if_not_exists: true,
        }
    }
    /// The `WHEN <condition>` guard (raw SurrealQL).
    pub fn when(mut self, cond: impl Into<String>) -> Self {
        self.when = cond.into();
        self
    }
    /// The `THEN <block>` body (raw SurrealQL, typically a `{ … }` block).
    pub fn then(mut self, block: impl Into<String>) -> Self {
        self.then = block.into();
        self
    }
    /// Drop the `IF NOT EXISTS` guard.
    pub fn overwrite(mut self) -> Self {
        self.if_not_exists = false;
        self
    }
    pub fn to_surrealql(&self) -> String {
        format!(
            "DEFINE EVENT {}{} ON TABLE {} WHEN {} THEN {}",
            guard(self.if_not_exists),
            self.name,
            self.table,
            self.when,
            self.then
        )
    }
    /// `REMOVE EVENT IF EXISTS <name> ON TABLE <table>`.
    pub fn remove(name: &str, table: &str) -> String {
        format!("REMOVE EVENT IF EXISTS {name} ON TABLE {table}")
    }
}

impl std::fmt::Display for DefineEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_surrealql())
    }
}

/// `DEFINE FUNCTION fn::<name>(<args>) -> <ret> { <body> }` — a user-defined
/// SurrealQL function. The `fn::` prefix is added automatically.
///
/// ```ignore
/// DefineFunction::new("greet")
///     .arg("name", "string")
///     .returns("string")
///     .body("RETURN 'hi ' + $name;")
///     .to_surrealql();
/// ```
pub struct DefineFunction {
    name: String,
    args: Vec<(String, String)>,
    returns: Option<String>,
    body: String,
    if_not_exists: bool,
}

impl DefineFunction {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            args: Vec::new(),
            returns: None,
            body: String::new(),
            if_not_exists: true,
        }
    }
    /// Add a typed argument — `$name: type`.
    pub fn arg(mut self, name: impl Into<String>, ty: impl Into<String>) -> Self {
        self.args.push((name.into(), ty.into()));
        self
    }
    /// Declared return type (`-> <ty>`).
    pub fn returns(mut self, ty: impl Into<String>) -> Self {
        self.returns = Some(ty.into());
        self
    }
    /// The function body (raw SurrealQL statements, e.g. `RETURN …;`).
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = body.into();
        self
    }
    pub fn overwrite(mut self) -> Self {
        self.if_not_exists = false;
        self
    }
    pub fn to_surrealql(&self) -> String {
        let args = self
            .args
            .iter()
            .map(|(n, t)| format!("${n}: {t}"))
            .collect::<Vec<_>>()
            .join(", ");
        let ret = self
            .returns
            .as_ref()
            .map(|r| format!(" -> {r}"))
            .unwrap_or_default();
        format!(
            "DEFINE FUNCTION {}fn::{}({}){} {{ {} }}",
            guard(self.if_not_exists),
            self.name,
            args,
            ret,
            self.body
        )
    }
    /// `REMOVE FUNCTION IF EXISTS fn::<name>`.
    pub fn remove(name: &str) -> String {
        format!("REMOVE FUNCTION IF EXISTS fn::{name}")
    }
}

impl std::fmt::Display for DefineFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_surrealql())
    }
}

/// `DEFINE ANALYZER <name> TOKENIZERS <toks> FILTERS <filters>` — a full-text
/// tokenizer + filter pipeline (referenced by a `SEARCH` index).
pub struct DefineAnalyzer {
    name: String,
    tokenizers: Vec<String>,
    filters: Vec<String>,
    if_not_exists: bool,
}

impl DefineAnalyzer {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            tokenizers: Vec::new(),
            filters: Vec::new(),
            if_not_exists: true,
        }
    }
    /// Set the tokenizers (e.g. `["class"]`, `["blank", "punct"]`).
    pub fn tokenizers<I, S>(mut self, toks: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.tokenizers = toks.into_iter().map(Into::into).collect();
        self
    }
    /// Set the filters (e.g. `["lowercase", "ascii", "snowball(english)"]`).
    pub fn filters<I, S>(mut self, filters: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.filters = filters.into_iter().map(Into::into).collect();
        self
    }
    pub fn overwrite(mut self) -> Self {
        self.if_not_exists = false;
        self
    }
    pub fn to_surrealql(&self) -> String {
        let mut q = format!("DEFINE ANALYZER {}{}", guard(self.if_not_exists), self.name);
        if !self.tokenizers.is_empty() {
            q.push_str(" TOKENIZERS ");
            q.push_str(&self.tokenizers.join(", "));
        }
        if !self.filters.is_empty() {
            q.push_str(" FILTERS ");
            q.push_str(&self.filters.join(", "));
        }
        q
    }
    /// `REMOVE ANALYZER IF EXISTS <name>`.
    pub fn remove(name: &str) -> String {
        format!("REMOVE ANALYZER IF EXISTS {name}")
    }
}

impl std::fmt::Display for DefineAnalyzer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_surrealql())
    }
}

/// `DEFINE PARAM $<name> VALUE <value>` — a database-scoped parameter. The `$`
/// prefix is added automatically.
pub struct DefineParam {
    name: String,
    value: String,
    if_not_exists: bool,
}

impl DefineParam {
    /// Begin a `DEFINE PARAM` with a raw SurrealQL value expression.
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            if_not_exists: true,
        }
    }
    /// Set the value from a typed literal instead of a raw string.
    pub fn value_lit<V: SurrealQL>(mut self, value: V) -> Self {
        let mut buf = String::new();
        V::render_literal(&value, &mut buf);
        self.value = buf;
        self
    }
    pub fn overwrite(mut self) -> Self {
        self.if_not_exists = false;
        self
    }
    pub fn to_surrealql(&self) -> String {
        format!(
            "DEFINE PARAM {}${} VALUE {}",
            guard(self.if_not_exists),
            self.name,
            self.value
        )
    }
    /// `REMOVE PARAM IF EXISTS $<name>`.
    pub fn remove(name: &str) -> String {
        format!("REMOVE PARAM IF EXISTS ${name}")
    }
}

impl std::fmt::Display for DefineParam {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_surrealql())
    }
}
