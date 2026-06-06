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
    expr::{Column, DynExpr, Order, Projection, RecordLink, SurrealQL},
    types::{SurrealEdge, SurrealRecord, Thing},
};

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
/// `START`, `FETCH`, `GROUP BY`/`GROUP ALL`, and `count()`.
pub struct Select<T: SurrealRecord> {
    _marker: std::marker::PhantomData<T>,
    projections: Vec<Projection>,
    filter: Option<Box<dyn DynExpr>>,
    order: Vec<(String, Order)>,
    limit: Option<u32>,
    start: u32,
    fetch: Vec<String>,
    group_by: Vec<String>,
    group_all: bool,
    count: bool,
    count_alias: Option<&'static str>,
}

impl<T: SurrealRecord> Select<T> {
    fn bare() -> Self {
        Select {
            _marker: std::marker::PhantomData,
            projections: Vec::new(),
            filter: None,
            order: Vec::new(),
            limit: None,
            start: 0,
            fetch: Vec::new(),
            group_by: Vec::new(),
            group_all: false,
            count: false,
            count_alias: None,
        }
    }

    pub fn filter(mut self, expr: impl DynExpr + 'static) -> Self {
        self.filter = Some(Box::new(expr));
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

    pub fn to_surrealql(&self) -> String {
        let mut q = String::from("SELECT ");
        self.render_select_list(&mut q);
        q.push_str(" FROM ");
        q.push_str(T::table_name());
        if let Some(ref f) = self.filter {
            q.push_str(" WHERE ");
            f.render_dyn(&mut q);
        }
        for (i, (col, dir)) in self.order.iter().enumerate() {
            if i == 0 {
                q.push_str(" ORDER BY ");
            } else {
                q.push_str(", ");
            }
            q.push_str(&format!("{col} {dir}"));
        }
        for (i, g) in self.group_by.iter().enumerate() {
            if i == 0 {
                q.push_str(" GROUP BY ");
            } else {
                q.push_str(", ");
            }
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
        q
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
}

impl<T: SurrealRecord> Insert<T> {
    pub fn content(mut self, record: T) -> Self {
        self.data.push(record);
        self
    }
    pub fn return_field(mut self, field: &'static str) -> Self {
        self.return_fields.push(field);
        self
    }
    pub fn data(&self) -> &[T] {
        &self.data
    }

    /// Render `INSERT INTO <table> <object|array> [RETURN AFTER]`, serializing the
    /// queued record(s) inline as SurrealQL object literals (JSON is a valid
    /// subset). A single record renders as `{ … }`, multiple as `[ {…}, {…} ]`.
    pub fn to_surrealql(&self) -> String
    where
        T: serde::Serialize,
    {
        let body = match self.data.as_slice() {
            [] => "[]".to_string(),
            [one] => serde_json::to_string(one).unwrap_or_else(|_| "{}".to_string()),
            many => serde_json::to_string(many).unwrap_or_else(|_| "[]".to_string()),
        };
        let returning = if self.return_fields.is_empty() {
            ""
        } else {
            " RETURN AFTER"
        };
        format!("INSERT INTO {} {}{}", T::table_name(), body, returning)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// UPDATE
// ═══════════════════════════════════════════════════════════════════════════════

enum SetVal {
    /// `SET k = v` where v is a rendered expression
    Assign(String, String),
    /// `MERGE <expr>`
    Merge(String),
    /// `CONTENT <expr>` (full replace)
    Content(String),
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
        let mut buf = String::new();
        C::render_literal(&value, &mut buf);
        self.sets.push(SetVal::Assign(col.name.to_string(), buf));
        self
    }
    /// `SET col = <literal>` by raw column name.
    pub fn set_lit<C: SurrealQL>(mut self, col: &str, value: C) -> Self {
        let mut buf = String::new();
        C::render_literal(&value, &mut buf);
        self.sets.push(SetVal::Assign(col.to_string(), buf));
        self
    }
    /// `SET col = <expr>` — e.g. a record link, `time::now()`, NONE, `use_count + 1`.
    pub fn set_expr(mut self, col: &str, expr: impl DynExpr) -> Self {
        let mut buf = String::new();
        expr.render_dyn(&mut buf);
        self.sets.push(SetVal::Assign(col.to_string(), buf));
        self
    }
    /// `SET col = <raw SurrealQL>`.
    pub fn set_raw(mut self, col: &str, raw: impl Into<String>) -> Self {
        self.sets.push(SetVal::Assign(col.to_string(), raw.into()));
        self
    }
    /// `MERGE <expr>` — deep-merge the given object into the record.
    pub fn merge(mut self, expr: impl DynExpr) -> Self {
        let mut buf = String::new();
        expr.render_dyn(&mut buf);
        self.sets.push(SetVal::Merge(buf));
        self
    }
    /// `CONTENT <expr>` — full-replace the record's content (upsert by record id).
    pub fn content(mut self, expr: impl DynExpr) -> Self {
        let mut buf = String::new();
        expr.render_dyn(&mut buf);
        self.sets.push(SetVal::Content(buf));
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

    pub fn to_surrealql(&self) -> String {
        let mut q = String::from(self.verb);
        q.push(' ');
        self.target.render(&mut q);
        // SurrealQL order: SET/MERGE/CONTENT first, then WHERE, then RETURN.
        let mut set_pairs = Vec::new();
        let mut merge_clause = None;
        let mut content_clause = None;
        for s in &self.sets {
            match s {
                SetVal::Assign(k, v) => set_pairs.push(format!("{k} = {v}")),
                SetVal::Merge(v) => merge_clause = Some(v.clone()),
                SetVal::Content(v) => content_clause = Some(v.clone()),
            }
        }
        if let Some(c) = content_clause {
            q.push_str(" CONTENT ");
            q.push_str(&c);
        } else if let Some(m) = merge_clause {
            q.push_str(" MERGE ");
            q.push_str(&m);
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
    Content(String),
    /// `SET a = x, b = y`
    Set(Vec<(String, String)>),
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
    pub fn content(mut self, expr: impl DynExpr) -> Self {
        let mut buf = String::new();
        expr.render_dyn(&mut buf);
        self.body = CreateBody::Content(buf);
        self
    }

    /// `SET col = <literal>`.
    pub fn set_lit<C: SurrealQL>(mut self, col: &str, value: C) -> Self {
        let mut buf = String::new();
        C::render_literal(&value, &mut buf);
        self.push_set(col, buf);
        self
    }
    /// `SET col = <expr>`.
    pub fn set_expr(mut self, col: &str, expr: impl DynExpr) -> Self {
        let mut buf = String::new();
        expr.render_dyn(&mut buf);
        self.push_set(col, buf);
        self
    }
    /// `SET col = <raw SurrealQL>`.
    pub fn set_raw(mut self, col: &str, raw: impl Into<String>) -> Self {
        self.push_set(col, raw.into());
        self
    }

    fn push_set(&mut self, col: &str, rendered: String) {
        match &mut self.body {
            CreateBody::Set(v) => v.push((col.to_string(), rendered)),
            CreateBody::Content(_) => {
                self.body = CreateBody::Set(vec![(col.to_string(), rendered)]);
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

    pub fn to_surrealql(&self) -> String {
        let mut q = String::from("CREATE ");
        self.target.render(&mut q);
        match &self.body {
            CreateBody::Content(c) => {
                q.push_str(" CONTENT ");
                q.push_str(c);
            }
            CreateBody::Set(pairs) if !pairs.is_empty() => {
                q.push_str(" SET ");
                q.push_str(
                    &pairs
                        .iter()
                        .map(|(k, v)| format!("{k} = {v}"))
                        .collect::<Vec<_>>()
                        .join(", "),
                );
            }
            CreateBody::Set(_) => {}
        }
        self.returning.render(&mut q);
        q
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
}

impl<E: SurrealEdge> RelateEdge<E> {
    pub fn from(from: &Thing<impl SurrealRecord>) -> Self {
        Self {
            _marker: std::marker::PhantomData,
            from_label: record_id_string(from),
            to_label: String::new(),
            content_json: None,
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
        q
    }
}
