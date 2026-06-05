#[derive(Debug, thiserror::Error)]
pub enum SomniaError {
    #[error("query: {0}")]
    Query(String),
    #[error("deserialization: {0}")]
    Deser(String),
    #[error("missing field '{0}'")]
    MissingField(String),
    #[error("type mismatch for '{field}': expected {expected}, got {got}")]
    TypeMismatch { field: String, expected: String, got: String },
    #[error("connection: {0}")]
    Connection(String),
    #[error("migration: {0}")]
    Migration(String),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("{0}")]
    Message(String),
}

impl SomniaError {
    pub fn query(msg: impl Into<String>) -> Self { Self::Query(msg.into()) }
    pub fn deser(msg: impl Into<String>) -> Self { Self::Deser(msg.into()) }
    pub fn missing(field: impl Into<String>) -> Self { Self::MissingField(field.into()) }
    pub fn msg(msg: impl Into<String>) -> Self { Self::Message(msg.into()) }
    pub fn migration(msg: impl Into<String>) -> Self { Self::Migration(msg.into()) }
}
