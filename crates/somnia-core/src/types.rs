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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Point(pub f64, pub f64);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineString(pub Vec<(f64, f64)>);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Polygon(pub Vec<Vec<(f64, f64)>>);

// ─── SurrealRecord trait ──────────────────────────────────────────────────────

pub trait SurrealRecord: Sized + Send + Sync + 'static + std::fmt::Debug + Clone {
    fn table_name() -> &'static str;
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

    /// Migration **up**: create the table and all its fields (the full schema),
    /// one statement per line (each already `;`-terminated).
    fn up() -> String {
        let mut out = String::from(Self::define_table());
        for f in Self::define_fields() {
            out.push('\n');
            out.push_str(f);
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
    use super::Key;
    use std::str::FromStr;

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
}
