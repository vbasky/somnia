//! Core record types and traits.
//!
//! - [`Key`] / [`Thing`] — a SurrealDB record id (`table:key`) and its typed
//!   wrapper `Thing<T>`.
//! - [`Point`] / [`LineString`] / [`Polygon`] — GeoJSON geometry values.
//! - [`SurrealRecord`] / [`SurrealEdge`] / [`SurrealSchema`] — the traits a record
//!   type implements (normally via `#[derive(SurrealRecord)]`).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The key part of a SurrealDB record ID (the part after `table:`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    String(String),
    Uuid(Uuid),
    Int(i64),
}

impl Key {
    pub fn to_surrealdb(&self) -> surrealdb_types::RecordIdKey {
        match self {
            Key::String(s) => surrealdb_types::RecordIdKey::String(s.clone()),
            Key::Uuid(u) => surrealdb_types::RecordIdKey::Uuid(surrealdb_types::Uuid::from(*u)),
            Key::Int(i) => surrealdb_types::RecordIdKey::Number(*i),
        }
    }

    /// Render this key as a SurrealDB record-id key fragment — the part after
    /// `table:`. Integers render bare; UUIDs and any string that isn't a simple
    /// identifier are wrapped in backticks (with `\` and `` ` `` escaped) so they
    /// parse as a single string id. Without this, a UUID key like
    /// `asset:0190a-…` would be parsed as an arithmetic expression.
    pub fn render_id(&self, buf: &mut String) {
        use std::fmt::Write;
        match self {
            Key::Int(i) => {
                let _ = write!(buf, "{i}");
            }
            Key::Uuid(u) => {
                let _ = write!(buf, "`{u}`");
            }
            Key::String(s) if is_simple_ident(s) => buf.push_str(s),
            Key::String(s) => {
                buf.push('`');
                for c in s.chars() {
                    if c == '\\' || c == '`' {
                        buf.push('\\');
                    }
                    buf.push(c);
                }
                buf.push('`');
            }
        }
    }
}

/// A bare SurrealDB identifier: ASCII letter/underscore followed by
/// letters/digits/underscores. Such record-id keys need no quoting.
fn is_simple_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl From<&str> for Key {
    /// Infers the key kind from the text: a valid UUID becomes [`Key::Uuid`], an
    /// integer becomes [`Key::Int`], and anything else becomes [`Key::String`].
    fn from(s: &str) -> Self {
        if let Ok(u) = Uuid::parse_str(s) {
            Key::Uuid(u)
        } else if let Ok(i) = s.parse::<i64>() {
            Key::Int(i)
        } else {
            Key::String(s.to_owned())
        }
    }
}
impl From<String> for Key {
    fn from(s: String) -> Self {
        Self::from(s.as_str())
    }
}

impl std::str::FromStr for Key {
    type Err = std::convert::Infallible;
    /// Always succeeds — see [`From<&str>`](#impl-From<%26str>-for-Key) for the
    /// inference rules. Lets callers use `"abc".parse::<Key>()`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from(s))
    }
}
impl From<Uuid> for Key {
    fn from(u: Uuid) -> Self {
        Self::Uuid(u)
    }
}
impl From<i64> for Key {
    fn from(i: i64) -> Self {
        Self::Int(i)
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Key::String(s) => write!(f, "{s}"),
            Key::Uuid(u) => write!(f, "{u}"),
            Key::Int(i) => write!(f, "{i}"),
        }
    }
}

// ─── Thing — typed record ID ──────────────────────────────────────────────────

/// A typed SurrealDB record ID. `Thing<Asset>` → `asset:xyz`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Thing<T: SurrealRecord> {
    pub key: Key,
    pub _marker: std::marker::PhantomData<T>,
}

impl<T: SurrealRecord> Thing<T> {
    pub fn new(key: impl Into<Key>) -> Self {
        Self {
            key: key.into(),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn table(&self) -> &'static str {
        T::table_name()
    }
}

impl<T: SurrealRecord> Serialize for Thing<T> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        format!("{}:{}", T::table_name(), self.key).serialize(s)
    }
}

impl<'de, T: SurrealRecord> Deserialize<'de> for Thing<T> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw: String = String::deserialize(d)?;
        let (tb, id_part) = raw
            .split_once(':')
            .ok_or_else(|| serde::de::Error::custom(format!("invalid record id: {raw}")))?;
        if tb != T::table_name() {
            return Err(serde::de::Error::custom(format!(
                "expected record for '{}', got '{}'",
                T::table_name(),
                tb
            )));
        }
        Ok(Thing::new(id_part))
    }
}

// ─── Geometry ─────────────────────────────────────────────────────────────────

// SurrealDB stores geometry as GeoJSON objects (`{ "type": …, "coordinates": … }`),
// not bare coordinate arrays. These types serialize/deserialize accordingly so a
// `Point` round-trips as a real `geometry` value rather than an `array<float>`.

/// A GeoJSON point — `(longitude, latitude)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Point(pub f64, pub f64);

impl Serialize for Point {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("Point", 2)?;
        st.serialize_field("type", "Point")?;
        st.serialize_field("coordinates", &[self.0, self.1])?;
        st.end()
    }
}

impl<'de> Deserialize<'de> for Point {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct G {
            coordinates: [f64; 2],
        }
        let g = G::deserialize(d)?;
        Ok(Point(g.coordinates[0], g.coordinates[1]))
    }
}

/// A GeoJSON line string — an ordered list of `(longitude, latitude)` points.
#[derive(Debug, Clone, PartialEq)]
pub struct LineString(pub Vec<(f64, f64)>);

impl Serialize for LineString {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let coords: Vec<[f64; 2]> = self.0.iter().map(|&(x, y)| [x, y]).collect();
        let mut st = s.serialize_struct("LineString", 2)?;
        st.serialize_field("type", "LineString")?;
        st.serialize_field("coordinates", &coords)?;
        st.end()
    }
}

impl<'de> Deserialize<'de> for LineString {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct G {
            coordinates: Vec<[f64; 2]>,
        }
        let g = G::deserialize(d)?;
        Ok(LineString(
            g.coordinates.into_iter().map(|[x, y]| (x, y)).collect(),
        ))
    }
}

/// A GeoJSON polygon — a list of linear rings of `(longitude, latitude)` points
/// (the first ring is the exterior; any others are holes).
#[derive(Debug, Clone, PartialEq)]
pub struct Polygon(pub Vec<Vec<(f64, f64)>>);

impl Serialize for Polygon {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let coords: Vec<Vec<[f64; 2]>> = self
            .0
            .iter()
            .map(|ring| ring.iter().map(|&(x, y)| [x, y]).collect())
            .collect();
        let mut st = s.serialize_struct("Polygon", 2)?;
        st.serialize_field("type", "Polygon")?;
        st.serialize_field("coordinates", &coords)?;
        st.end()
    }
}

impl<'de> Deserialize<'de> for Polygon {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct G {
            coordinates: Vec<Vec<[f64; 2]>>,
        }
        let g = G::deserialize(d)?;
        Ok(Polygon(
            g.coordinates
                .into_iter()
                .map(|ring| ring.into_iter().map(|[x, y]| (x, y)).collect())
                .collect(),
        ))
    }
}

// ─── SurrealRecord trait ──────────────────────────────────────────────────────

/// A type that maps to a SurrealDB table. Normally implemented by
/// `#[derive(SurrealRecord)]`, which also generates the typed column accessors and
/// the query-builder entry points (`table()`, `all()`).
pub trait SurrealRecord: Sized + Send + Sync + 'static + std::fmt::Debug + Clone {
    /// The SurrealDB table name (e.g. `"post"`).
    fn table_name() -> &'static str;
    /// The record-id field name (the `#[field(thing)]` field; defaults to `"id"`).
    fn primary_key() -> &'static str;
}

/// Marker for edge records.
pub trait SurrealEdge: SurrealRecord {
    fn edge_name() -> &'static str;
}

/// Schema definition for a record type — lets the Rust type be the single
/// source of truth for the SurrealDB schema. Implemented by
/// `#[derive(SurrealRecord)]`; emits idempotent DDL and reversible
/// `up()` / `down()` migrations.
pub trait SurrealSchema: SurrealRecord {
    /// `DEFINE TABLE IF NOT EXISTS <table> …;`
    fn define_table() -> &'static str;
    /// One `DEFINE FIELD IF NOT EXISTS … ;` per non-id field, in declaration order.
    fn define_fields() -> &'static [&'static str];
    /// `REMOVE TABLE IF EXISTS <table>;`
    fn remove_table() -> &'static str;

    /// One `DEFINE INDEX IF NOT EXISTS … ;` per `#[index(...)]` on the type, in
    /// declaration order. Defaults to none for types without indexes.
    fn define_indexes() -> &'static [&'static str] {
        &[]
    }

    /// Migration **up**: create the table, its fields, and its indexes (the full
    /// schema), one statement per line (each already `;`-terminated).
    fn up() -> String {
        let mut out = String::from(Self::define_table());
        for f in Self::define_fields() {
            out.push('\n');
            out.push_str(f);
        }
        for i in Self::define_indexes() {
            out.push('\n');
            out.push_str(i);
        }
        out
    }

    /// Migration **down**: drop the table (which also drops its fields).
    fn down() -> String {
        Self::remove_table().to_string()
    }

    /// Alias for [`up`](Self::up) — the full `DEFINE TABLE`/`DEFINE FIELD` schema.
    fn define_schema() -> String {
        Self::up()
    }
}

#[cfg(test)]
mod tests {
    use super::{Key, LineString, Point, Polygon};
    use std::str::FromStr;

    fn render(k: &Key) -> String {
        let mut s = String::new();
        k.render_id(&mut s);
        s
    }

    #[test]
    fn key_inference() {
        assert_eq!(Key::from("123"), Key::Int(123));
        assert!(matches!(Key::from("not-a-number"), Key::String(_)));
        assert!(matches!(
            Key::from("550e8400-e29b-41d4-a716-446655440000"),
            Key::Uuid(_)
        ));
    }

    #[test]
    fn key_from_str_is_infallible() {
        // `FromStr` mirrors `From<&str>` and never errors.
        assert_eq!(Key::from_str("42").unwrap(), Key::Int(42));
        assert_eq!("hello".parse::<Key>().unwrap(), Key::String("hello".into()));
    }

    #[test]
    fn key_render_id_escapes_non_simple_keys() {
        assert_eq!(render(&Key::Int(7)), "7");
        // simple identifier → bare
        assert_eq!(render(&Key::String("alice".into())), "alice");
        // UUID has dashes → must be backtick-quoted so it isn't parsed as math
        assert_eq!(
            render(&Key::from("550e8400-e29b-41d4-a716-446655440000")),
            "`550e8400-e29b-41d4-a716-446655440000`"
        );
        // special chars → quoted + escaped
        assert_eq!(render(&Key::String("a b/c".into())), "`a b/c`");
        assert_eq!(render(&Key::String("ti`ck".into())), "`ti\\`ck`");
    }

    #[test]
    fn geometry_serializes_as_geojson() {
        assert_eq!(
            serde_json::to_string(&Point(1.0, 2.0)).unwrap(),
            r#"{"type":"Point","coordinates":[1.0,2.0]}"#
        );
        assert_eq!(
            serde_json::to_string(&LineString(vec![(0.0, 0.0), (1.0, 1.0)])).unwrap(),
            r#"{"type":"LineString","coordinates":[[0.0,0.0],[1.0,1.0]]}"#
        );
        assert_eq!(
            serde_json::to_string(&Polygon(vec![vec![(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)]]))
                .unwrap(),
            r#"{"type":"Polygon","coordinates":[[[0.0,0.0],[1.0,0.0],[0.0,1.0]]]}"#
        );
    }

    #[test]
    fn geometry_round_trips() {
        let p = Point(12.5, -3.25);
        let back: Point = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(p, back);

        let poly = Polygon(vec![vec![(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)]]);
        let back: Polygon = serde_json::from_str(&serde_json::to_string(&poly).unwrap()).unwrap();
        assert_eq!(poly, back);
    }
}
