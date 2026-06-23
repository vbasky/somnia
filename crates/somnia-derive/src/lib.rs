//! Derive macros for [somnia](https://docs.rs/somnia), the type-safe SurrealDB ORM.
//!
//! This crate provides the `SurrealRecord` derive, which turns a plain Rust struct
//! into a typed SurrealDB record — generating the table name, typed column
//! accessors for the query builder, and the schema DDL (`DEFINE TABLE` /
//! `DEFINE FIELD`).
//!
//! You normally don't depend on this crate directly; use the re-export from the
//! `somnia` umbrella crate: `use somnia::SurrealRecord;`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields, Meta};

/// Derive typed SurrealDB record metadata, column accessors, and schema DDL.
///
/// Applied to a named-field struct, `#[derive(SurrealRecord)]` generates:
///
/// - **`SurrealRecord`** — `table_name()` and `primary_key()`.
/// - **`SurrealSchema`** — `define_table()` / `define_fields()` / `remove_table()`
///   (the `DEFINE TABLE` / `DEFINE FIELD` / `REMOVE TABLE` DDL), plus the
///   reversible `up()` / `down()` migration helpers built from them.
/// - **Inherent associated functions**:
///   - `Type::table()` — entry point to the typed query builder (`Table<Type>`).
///   - `Type::all()` — the `*` column set for `SELECT`.
///   - `Type::<field>()` — a typed column accessor per field (e.g. `Post::title()`),
///     returning a `Column<Type, FieldTy>` for use in filters and projections.
///
/// # Container attributes — `#[table(...)]`
///
/// | Form | Effect |
/// |------|--------|
/// | `#[table("name")]` | table name (defaults to the lowercased struct name) |
/// | `#[table("name", schemaless)]` | emit `SCHEMALESS` (default `SCHEMAFULL`) |
/// | `#[table("name", permissions = "NONE")]` | table `PERMISSIONS` clause (default `FULL`) |
///
/// # Container attributes — `#[index(...)]` (repeatable)
///
/// Each `#[index(...)]` emits one `DEFINE INDEX` (included in `up()` after the
/// fields, and in `define_indexes()`):
///
/// | Option | Effect |
/// |--------|--------|
/// | `name = "…"` | index name (required) |
/// | `fields = "a, b"` / `columns = "…"` | indexed field list (required) |
/// | `unique` | `UNIQUE` constraint |
/// | `search = "analyzer"` | full-text `FULLTEXT ANALYZER …` index |
/// | `hnsw = N` / `mtree = N` (+ `dist = "COSINE"`) | vector index of dimension `N` |
/// | `raw = "…"` | verbatim trailing clause (escape hatch) |
/// | `comment = "…"`, `concurrently`, `overwrite` | `COMMENT` / `CONCURRENTLY` / drop `IF NOT EXISTS` |
///
/// # Field attributes — `#[field(...)]`
///
/// | Attribute | Effect |
/// |-----------|--------|
/// | `#[field(thing)]` | the record-id field; becomes `primary_key`, omitted from `DEFINE FIELD` |
/// | `#[field(record = "table")]` | field type is `record<table>` (a link) |
/// | `#[field(ty = "…")]` | override the full SurrealQL field type |
/// | `#[field(default = "…")]` | `DEFAULT …` clause |
/// | `#[field(value = "…")]` | `VALUE …` clause |
/// | `#[field(assert = "…")]` | `ASSERT …` validation expression |
/// | `#[field(readonly)]` | mark the field `READONLY` |
/// | `#[field(permissions = "…")]` | `PERMISSIONS …` clause (e.g. `FOR select FULL FOR update NONE`) |
/// | `#[field(flexible)]` | mark the field `FLEXIBLE` |
/// | `#[field(reference)]` / `#[field(reference = "cascade")]` | record link is a `REFERENCE` (optional `ON DELETE` `reject`/`ignore`/`cascade`/`unset`) |
/// | `#[field(name = "…")]` | use a DB column name different from the Rust field |
/// | `#[field(skip)]` | omit the field entirely |
///
/// # Example
///
/// ```ignore
/// use somnia::{SurrealRecord, Thing};
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Debug, Clone, Serialize, Deserialize, SurrealRecord)]
/// #[table("post")]
/// struct Post {
///     #[field(thing)]
///     id: Thing<Post>,
///     title: String,
///     published_at: Option<String>,
/// }
///
/// assert_eq!(Post::table_name(), "post");
/// let sql = Post::table()
///     .select(Post::all())
///     .filter(Post::title().eq("hi".to_string()))
///     .to_surrealql();
/// ```
///
/// # Panics
///
/// Compile-time error if applied to anything other than a struct with named fields.
#[proc_macro_derive(SurrealRecord, attributes(table, field, index))]
pub fn derive_surreal_record(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let table = parse_table_attr(&input.attrs)
        .unwrap_or_else(|| TableAttr::named(name.to_string().to_lowercase()));
    let table_name = table.name.clone();

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(fs) => &fs.named,
            _ => panic!("SurrealRecord only supports named fields"),
        },
        _ => panic!("SurrealRecord can only be derived on structs"),
    };

    let mut field_defs = Vec::new();
    let mut primary = "id".to_string();

    for field in fields {
        let field_name = field.ident.as_ref().unwrap();
        let db_name =
            find_field_attr(&field.attrs, "name").unwrap_or_else(|| field_name.to_string());
        let is_thing = has_field_attr(&field.attrs, "thing");
        let is_skip = has_field_attr(&field.attrs, "skip");
        if is_skip {
            continue;
        }
        if is_thing {
            primary = db_name.clone();
        }

        let ty = &field.ty;
        let surreal_type = type_to_surreal(ty);

        field_defs.push(FieldDef {
            accessor_name: field_name.clone(),
            db_name,
            surreal_type,
            ty: ty.clone(),
            is_thing,
            record: find_field_attr(&field.attrs, "record"),
            ty_override: find_field_attr(&field.attrs, "ty"),
            default: find_field_attr(&field.attrs, "default"),
            value: find_field_attr(&field.attrs, "value"),
            flexible: has_field_attr(&field.attrs, "flexible"),
            assert: find_field_attr(&field.attrs, "assert"),
            readonly: has_field_attr(&field.attrs, "readonly"),
            permissions: find_field_attr(&field.attrs, "permissions"),
            reference: parse_reference_clause(&field.attrs),
        });
    }

    // Typed accessor functions: Asset::name() -> Column<Asset, String>
    let accessors: Vec<_> = field_defs.iter().map(|f| {
        let fn_name = &f.accessor_name;
        let db = &f.db_name;
        let st = &f.surreal_type;
        let ty = &f.ty;
        quote! {
            #[allow(non_snake_case)]
            pub fn #fn_name() -> ::somnia_core::Column<Self, #ty> {
                ::somnia_core::Column { name: #db, surreal_type: #st, _marker: ::std::marker::PhantomData }
            }
        }
    }).collect();

    // ColumnSet::all()
    let all_metas: Vec<_> = field_defs
        .iter()
        .map(|f| {
            let db = &f.db_name;
            let st = &f.surreal_type;
            quote! { ::somnia_core::ColumnMeta { name: #db, surreal_type: #st } }
        })
        .collect();

    // ─── Schema DDL (the Rust type is the source of truth) ──────────────────────
    let schemafull = if table.schemaless {
        "SCHEMALESS"
    } else {
        "SCHEMAFULL"
    };
    let table_ddl = format!(
        "DEFINE TABLE IF NOT EXISTS {table_name} {schemafull} PERMISSIONS {};",
        table.permissions,
    );
    let remove_ddl = format!("REMOVE TABLE IF EXISTS {table_name};");

    // One DEFINE INDEX per `#[index(...)]` on the struct, in declaration order.
    let index_ddls: Vec<String> = parse_index_attrs(&input.attrs)
        .into_iter()
        .map(|ix| ix.to_ddl(&table_name))
        .collect();

    // One DEFINE FIELD per non-id field, in declaration order.
    let field_ddls: Vec<String> = field_defs.iter()
        .filter(|f| !f.is_thing)
        .map(|f| {
            let ty = match &f.ty_override {
                Some(t) => t.clone(),
                None => schema_type(&f.ty, f.record.as_deref(), f.flexible),
            };
            let flex = if f.flexible { "FLEXIBLE " } else { "" };
            let default = f.default.as_ref().map(|d| format!(" DEFAULT {d}")).unwrap_or_default();
            let readonly = if f.readonly { " READONLY" } else { "" };
            let value = f.value.as_ref().map(|v| format!(" VALUE {v}")).unwrap_or_default();
            let assert = f.assert.as_ref().map(|a| format!(" ASSERT {a}")).unwrap_or_default();
            let permissions = f.permissions.as_ref().map(|p| format!(" PERMISSIONS {p}")).unwrap_or_default();
            let reference = f.reference.clone().unwrap_or_default();
            format!(
                "DEFINE FIELD IF NOT EXISTS {} ON TABLE {table_name} {flex}TYPE {ty}{default}{readonly}{value}{assert}{reference}{permissions};",
                f.db_name,
            )
        })
        .collect();

    let gen = quote! {
        impl ::somnia_core::SurrealRecord for #name {
            fn table_name() -> &'static str { #table_name }
            fn primary_key() -> &'static str { #primary }
        }

        impl ::somnia_core::SurrealSchema for #name {
            fn define_table() -> &'static str { #table_ddl }
            fn define_fields() -> &'static [&'static str] { &[ #(#field_ddls),* ] }
            fn remove_table() -> &'static str { #remove_ddl }
            fn define_indexes() -> &'static [&'static str] { &[ #(#index_ddls),* ] }
        }

        impl #name {
            #(#accessors)*

            pub fn all() -> ::somnia_core::ColumnSet<Self> {
                static COLS: &[::somnia_core::ColumnMeta] = &[#(#all_metas),*];
                ::somnia_core::ColumnSet { cols: COLS, _marker: ::std::marker::PhantomData }
            }

            pub fn table() -> ::somnia_core::Table<Self> {
                ::somnia_core::Table::new()
            }
        }
    };

    gen.into()
}

/// Derive `SurrealEdge` for an edge record, so the
/// `impl SurrealEdge { fn edge_name() … }` no longer needs to be hand-written.
///
/// The edge name comes from `#[table("name")]` (same as `SurrealRecord`), or the
/// lowercased struct name if omitted. Derive it alongside `SurrealRecord`:
///
/// ```ignore
/// #[derive(SurrealRecord, SurrealEdge, Serialize, Deserialize)]
/// #[table("follows")]
/// struct Follows {
///     #[field(thing)] id: Thing<Follows>,
/// }
/// assert_eq!(Follows::edge_name(), "follows");
/// ```
#[proc_macro_derive(SurrealEdge, attributes(table))]
pub fn derive_surreal_edge(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let edge_name = parse_table_attr(&input.attrs)
        .map(|t| t.name)
        .unwrap_or_else(|| name.to_string().to_lowercase());
    quote! {
        impl ::somnia_core::SurrealEdge for #name {
            fn edge_name() -> &'static str { #edge_name }
        }
    }
    .into()
}

struct FieldDef {
    accessor_name: syn::Ident,
    db_name: String,
    surreal_type: String,
    ty: syn::Type,
    is_thing: bool,
    record: Option<String>,
    ty_override: Option<String>,
    default: Option<String>,
    value: Option<String>,
    flexible: bool,
    assert: Option<String>,
    readonly: bool,
    permissions: Option<String>,
    /// Pre-rendered ` REFERENCE [ON DELETE …]` clause from `#[field(reference …)]`.
    reference: Option<String>,
}

/// Parsed `#[table(...)]` options.
struct TableAttr {
    name: String,
    schemaless: bool,
    permissions: String,
}

impl TableAttr {
    fn named(name: String) -> Self {
        Self {
            name,
            schemaless: false,
            permissions: "FULL".to_string(),
        }
    }
}

/// Parse `#[table("name")]` or `#[table("name", schemaless, permissions = "NONE")]`.
fn parse_table_attr(attrs: &[syn::Attribute]) -> Option<TableAttr> {
    use syn::parse::ParseStream;
    for attr in attrs {
        if !attr.path().is_ident("table") {
            continue;
        }
        let Meta::List(ml) = &attr.meta else {
            continue;
        };
        let parsed = ml.parse_args_with(|input: ParseStream| {
            let mut t = TableAttr::named(String::new());
            // Optional leading bare string literal → table name.
            if input.peek(syn::LitStr) {
                let name: syn::LitStr = input.parse()?;
                t.name = name.value();
            }
            while !input.is_empty() {
                if input.peek(syn::Token![,]) {
                    let _: syn::Token![,] = input.parse()?;
                }
                if input.is_empty() {
                    break;
                }
                let ident: syn::Ident = input.parse()?;
                if ident == "schemaless" {
                    t.schemaless = true;
                } else if ident == "schemafull" {
                    t.schemaless = false;
                } else if ident == "permissions" || ident == "name" {
                    let _: syn::Token![=] = input.parse()?;
                    let val: syn::LitStr = input.parse()?;
                    if ident == "permissions" {
                        t.permissions = val.value();
                    } else {
                        t.name = val.value();
                    }
                }
            }
            Ok(t)
        });
        if let Ok(t) = parsed {
            if !t.name.is_empty() {
                return Some(t);
            }
        }
    }
    None
}

/// A parsed `#[index(...)]` container attribute.
struct IndexAttr {
    name: String,
    fields: Vec<String>,
    unique: bool,
    /// verbatim trailing clause (SEARCH/HNSW/MTREE/raw); wins over `unique`
    tail: Option<String>,
    comment: Option<String>,
    concurrently: bool,
    if_not_exists: bool,
}

impl IndexAttr {
    /// Render the `;`-terminated `DEFINE INDEX` DDL for this index on `table`,
    /// matching the runtime `DefineIndex` builder's format.
    fn to_ddl(&self, table: &str) -> String {
        let guard = if self.if_not_exists {
            "IF NOT EXISTS "
        } else {
            ""
        };
        let mut q = format!(
            "DEFINE INDEX {guard}{} ON TABLE {table} FIELDS {}",
            self.name,
            self.fields.join(", "),
        );
        if let Some(tail) = &self.tail {
            q.push(' ');
            q.push_str(tail);
        } else if self.unique {
            q.push_str(" UNIQUE");
        }
        if let Some(c) = &self.comment {
            let escaped = c.replace('\\', "\\\\").replace('\'', "\\'");
            q.push_str(&format!(" COMMENT '{escaped}'"));
        }
        if self.concurrently {
            q.push_str(" CONCURRENTLY");
        }
        q.push(';');
        q
    }
}

/// Parse every `#[index(name = "…", fields = "a, b", unique)]` on the struct.
///
/// Recognized options: `name` (str, required), `fields`/`columns` (comma-list,
/// required), `unique` (flag), `search` (analyzer str), `hnsw`/`mtree` (int
/// dimension) with optional `dist` (str), `raw` (verbatim trailing clause),
/// `comment` (str), `concurrently` (flag), `overwrite` (flag — drop the guard).
fn parse_index_attrs(attrs: &[syn::Attribute]) -> Vec<IndexAttr> {
    use syn::parse::ParseStream;
    let mut out = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("index") {
            continue;
        }
        let Meta::List(ml) = &attr.meta else {
            continue;
        };
        let parsed = ml.parse_args_with(|input: ParseStream| {
            let mut name = String::new();
            let mut fields = Vec::new();
            let mut unique = false;
            let mut search: Option<String> = None;
            let mut vector: Option<(&'static str, u64)> = None; // (HNSW|MTREE, dim)
            let mut dist: Option<String> = None;
            let mut raw: Option<String> = None;
            let mut comment: Option<String> = None;
            let mut concurrently = false;
            let mut if_not_exists = true;

            while !input.is_empty() {
                if input.peek(syn::Token![,]) {
                    let _: syn::Token![,] = input.parse()?;
                    continue;
                }
                let ident: syn::Ident = input.parse()?;
                match ident.to_string().as_str() {
                    "unique" => unique = true,
                    "concurrently" => concurrently = true,
                    "overwrite" => if_not_exists = false,
                    "hnsw" | "mtree" => {
                        let _: syn::Token![=] = input.parse()?;
                        let n: syn::LitInt = input.parse()?;
                        let kind = if ident == "hnsw" { "HNSW" } else { "MTREE" };
                        vector = Some((kind, n.base10_parse()?));
                    }
                    key => {
                        let _: syn::Token![=] = input.parse()?;
                        let val: syn::LitStr = input.parse()?;
                        let v = val.value();
                        match key {
                            "name" => name = v,
                            "fields" | "columns" => {
                                fields = v
                                    .split(',')
                                    .map(|s| s.trim().to_string())
                                    .filter(|s| !s.is_empty())
                                    .collect();
                            }
                            "search" => search = Some(v),
                            "dist" => dist = Some(v),
                            "raw" => raw = Some(v),
                            "comment" => comment = Some(v),
                            _ => {} // ignore unknown keys
                        }
                    }
                }
            }

            // Tail precedence: raw > search > vector. `unique` is handled at render.
            let tail = raw
                .or_else(|| search.map(|a| format!("FULLTEXT ANALYZER {a}")))
                .or_else(|| {
                    vector.map(|(kind, dim)| match &dist {
                        Some(d) => format!("{kind} DIMENSION {dim} DIST {d}"),
                        None => format!("{kind} DIMENSION {dim}"),
                    })
                });

            Ok(IndexAttr {
                name,
                fields,
                unique,
                tail,
                comment,
                concurrently,
                if_not_exists,
            })
        });
        if let Ok(ix) = parsed {
            if !ix.name.is_empty() && !ix.fields.is_empty() {
                out.push(ix);
            }
        }
    }
    out
}

fn find_field_attr(attrs: &[syn::Attribute], key: &str) -> Option<String> {
    for attr in attrs {
        if attr.path().is_ident("field") {
            if let Meta::List(ml) = &attr.meta {
                let nested: syn::punctuated::Punctuated<syn::Meta, syn::Token![,]> = ml
                    .parse_args_with(syn::punctuated::Punctuated::parse_terminated)
                    .ok()?;
                for meta in nested {
                    if let syn::Meta::NameValue(nv) = meta {
                        if nv.path.is_ident(key) {
                            if let syn::Expr::Lit(lit) = &nv.value {
                                if let syn::Lit::Str(s) = &lit.lit {
                                    return Some(s.value());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

fn has_field_attr(attrs: &[syn::Attribute], key: &str) -> bool {
    for attr in attrs {
        if attr.path().is_ident("field") {
            if let Meta::List(ml) = &attr.meta {
                let nested: syn::punctuated::Punctuated<syn::Meta, syn::Token![,]> =
                    match ml.parse_args_with(syn::punctuated::Punctuated::parse_terminated) {
                        Ok(n) => n,
                        Err(_) => continue,
                    };
                for meta in nested {
                    if meta.path().is_ident(key) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Build the ` REFERENCE [ON DELETE …]` clause for a record-link field:
/// - `#[field(reference)]` → ` REFERENCE` (engine default delete strategy)
/// - `#[field(reference = "cascade")]` → ` REFERENCE ON DELETE CASCADE`
///   (`reject`/`ignore`/`cascade`/`unset`; any other value passes through after
///   `ON DELETE`, e.g. `"then fn::cleanup()"`).
fn parse_reference_clause(attrs: &[syn::Attribute]) -> Option<String> {
    if let Some(strategy) = find_field_attr(attrs, "reference") {
        let s = strategy.trim();
        let clause = match s.to_uppercase().as_str() {
            kw @ ("REJECT" | "IGNORE" | "CASCADE" | "UNSET") => {
                format!(" REFERENCE ON DELETE {kw}")
            }
            _ => format!(" REFERENCE ON DELETE {s}"),
        };
        return Some(clause);
    }
    if has_field_attr(attrs, "reference") {
        return Some(" REFERENCE".to_string());
    }
    None
}

/// SurrealQL type for a typed-column accessor (used by the query builder).
fn type_to_surreal(ty: &syn::Type) -> String {
    surreal_type_of(ty)
}

/// SurrealQL schema type for a `DEFINE FIELD`. A `#[field(record = "tbl")]`
/// override forces `record<tbl>` (wrapped in `option<…>` for an `Option` field);
/// otherwise the type is mapped structurally by [`surreal_type_of`].
fn schema_type(ty: &syn::Type, record: Option<&str>, _flexible: bool) -> String {
    if let Some(tbl) = record {
        let inner = format!("record<{tbl}>");
        return if is_option(ty) {
            format!("option<{inner}>")
        } else {
            inner
        };
    }
    surreal_type_of(ty)
}

/// Recursively map a Rust type to its SurrealQL type name by inspecting the
/// `syn::Type` structure (path segments + generic arguments) rather than
/// substring-matching the stringified type. This makes nesting precise —
/// `Option<Vec<String>>` → `option<array<string>>` — and avoids false matches
/// from look-alike type names.
fn surreal_type_of(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(tp) => {
            let Some(seg) = tp.path.segments.last() else {
                return "object".into();
            };
            match seg.ident.to_string().as_str() {
                "Option" => match first_generic(seg) {
                    Some(inner) => format!("option<{}>", surreal_type_of(inner)),
                    None => "object".into(),
                },
                "Vec" | "VecDeque" => match first_generic(seg) {
                    Some(inner) => format!("array<{}>", surreal_type_of(inner)),
                    None => "array".into(),
                },
                "HashSet" | "BTreeSet" => match first_generic(seg) {
                    Some(inner) => format!("array<{}>", surreal_type_of(inner)),
                    None => "array".into(),
                },
                "String" | "str" => "string".into(),
                "bool" => "bool".into(),
                "f32" | "f64" => "float".into(),
                "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u16" | "u32" | "u64"
                | "u128" | "usize" => "int".into(),
                "u8" => "int".into(),
                "Decimal" => "decimal".into(),
                "Duration" => "duration".into(),
                "Uuid" => "uuid".into(),
                "DateTime" | "NaiveDateTime" | "NaiveDate" => "datetime".into(),
                "Thing" | "RecordId" => "record".into(),
                "HashMap" | "BTreeMap" => "object".into(),
                // serde_json::Value and anything unrecognized → object.
                _ => "object".into(),
            }
        }
        syn::Type::Reference(r) => surreal_type_of(&r.elem),
        syn::Type::Slice(s) => format!("array<{}>", surreal_type_of(&s.elem)),
        syn::Type::Array(a) => format!("array<{}>", surreal_type_of(&a.elem)),
        syn::Type::Group(g) => surreal_type_of(&g.elem),
        syn::Type::Paren(p) => surreal_type_of(&p.elem),
        _ => "object".into(),
    }
}

/// The first generic type argument of a path segment (e.g. the `T` in `Vec<T>`).
fn first_generic(seg: &syn::PathSegment) -> Option<&syn::Type> {
    if let syn::PathArguments::AngleBracketed(ab) = &seg.arguments {
        for arg in &ab.args {
            if let syn::GenericArgument::Type(t) = arg {
                return Some(t);
            }
        }
    }
    None
}

/// Whether the outermost type constructor is `Option`.
fn is_option(ty: &syn::Type) -> bool {
    matches!(ty, syn::Type::Path(tp)
        if tp.path.segments.last().is_some_and(|s| s.ident == "Option"))
}
