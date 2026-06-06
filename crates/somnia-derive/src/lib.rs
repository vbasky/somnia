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
/// # Field attributes — `#[field(...)]`
///
/// | Attribute | Effect |
/// |-----------|--------|
/// | `#[field(thing)]` | the record-id field; becomes `primary_key`, omitted from `DEFINE FIELD` |
/// | `#[field(record = "table")]` | field type is `record<table>` (a link) |
/// | `#[field(ty = "…")]` | override the full SurrealQL field type |
/// | `#[field(default = "…")]` | `DEFAULT …` clause |
/// | `#[field(value = "…")]` | `VALUE …` clause |
/// | `#[field(flexible)]` | mark the field `FLEXIBLE` |
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
#[proc_macro_derive(SurrealRecord, attributes(table, field))]
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
            let value = f.value.as_ref().map(|v| format!(" VALUE {v}")).unwrap_or_default();
            format!(
                "DEFINE FIELD IF NOT EXISTS {} ON TABLE {table_name} {flex}TYPE {ty}{default}{value};",
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

/// SurrealQL type for a typed-column accessor (used by the query builder).
fn type_to_surreal(ty: &syn::Type) -> String {
    let s = quote!(#ty).to_string();
    if s.contains("String") && !s.contains("Option") {
        return "string".into();
    }
    if s.contains("String") && s.contains("Option") {
        return "option<string>".into();
    }
    if s.contains("i64") {
        return if s.contains("Option") {
            "option<int>".into()
        } else {
            "int".into()
        };
    }
    if s.contains("i32") {
        return if s.contains("Option") {
            "option<int>".into()
        } else {
            "int".into()
        };
    }
    if s.contains("f64") {
        return if s.contains("Option") {
            "option<float>".into()
        } else {
            "float".into()
        };
    }
    if s.contains("bool") {
        return if s.contains("Option") {
            "option<bool>".into()
        } else {
            "bool".into()
        };
    }
    if s.contains("DateTime") || s.contains("Utc") {
        return "datetime".into();
    }
    if s.contains("Uuid") {
        return "uuid".into();
    }
    if s.contains("Thing") {
        return "record".into();
    }
    if s.contains("Vec") || s.contains("Array") {
        return "array".into();
    }
    if s.contains("HashMap") || s.contains("BTreeMap") {
        return "object".into();
    }
    "object".into()
}

/// SurrealQL schema type for a `DEFINE FIELD`. Honors `#[field(record = "tbl")]`
/// (→ `record<tbl>`) and wraps in `option<…>` for `Option<T>` fields.
fn schema_type(ty: &syn::Type, record: Option<&str>, _flexible: bool) -> String {
    let s = quote!(#ty).to_string();
    let is_opt = s.contains("Option");
    let inner = if let Some(tbl) = record {
        format!("record<{tbl}>")
    } else {
        base_surreal_type(&s).to_string()
    };
    if is_opt {
        format!("option<{inner}>")
    } else {
        inner
    }
}

/// Base SurrealQL scalar/compound type, ignoring any `Option<…>` wrapper.
fn base_surreal_type(s: &str) -> &'static str {
    if s.contains("String") {
        return "string";
    }
    if s.contains("Uuid") {
        return "uuid";
    }
    if s.contains("DateTime") || s.contains("Utc") {
        return "datetime";
    }
    if s.contains("bool") {
        return "bool";
    }
    if s.contains("f64") || s.contains("f32") {
        return "float";
    }
    if s.contains("i8")
        || s.contains("i16")
        || s.contains("i32")
        || s.contains("i64")
        || s.contains("u8")
        || s.contains("u16")
        || s.contains("u32")
        || s.contains("u64")
        || s.contains("usize")
        || s.contains("isize")
    {
        return "int";
    }
    if s.contains("Thing") {
        return "record";
    }
    if s.contains("Vec") || s.contains("Array") {
        return "array";
    }
    // serde_json::Value, HashMap/BTreeMap, and anything unrecognized → object.
    "object"
}
